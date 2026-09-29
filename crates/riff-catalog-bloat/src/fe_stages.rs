//! Trace emitted bytes back through the Fe compiler's stages.
//!
//! A selection is a set of emitted instructions (by opcode, byte pattern,
//! run class, function, or attribution). For each selection the report
//! follows the trace's origin edges from every selected `bytecode.pc` back to
//! HIR ([`StageGraph::trace_back`]) and counts what it meets at each stage,
//! which phases introduced the edges, where each instruction's provenance
//! ends, and which Fe source constructs are the primary sources.
//!
//! Chains are content-addressed: for every HIR construct that is the primary
//! source of emitted bytes, the chain from it to those bytes
//! ([`StageGraph::chain_graph`]) is digested by the core engine at level
//! [`STAGE_CHAIN_LEVEL`]. Constructs whose chains share an address expanded
//! the same way through every stage, so they group across call sites.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use anyhow::{Context, Result};
use riff_catalog_core::{CyclePolicy, DigestRequest, Facet, HashPolicy, ViewMode, digest_graph};
use riff_catalog_evm::runs::decode;
use riff_catalog_ingest_trace::bytes::{DetailsRow, source_body};
use riff_catalog_ingest_trace::stages::{Stage, StageGraph};
use serde::{Deserialize, Serialize};

pub const FE_STAGES_SCHEMA: &str = "riffcat-fe-stages/1";
/// Level of chain graphs ([`StageGraph::chain_graph`]).
pub const STAGE_CHAIN_LEVEL: &str = "fe-stage-chain/1";

/// Memory opcodes: MLOAD, MSTORE, MSTORE8, MSIZE, MCOPY.
pub const MEMORY_OPCODES: [u8; 5] = [0x51, 0x52, 0x53, 0x59, 0x5e];

/// A named set of emitted instructions (pc starts).
#[derive(Clone, Debug)]
pub struct Selection {
    pub name: String,
    pub pcs: BTreeSet<u32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Tally {
    pub bytes: u64,
    pub instructions: u64,
}

impl Tally {
    fn add(&mut self, bytes: u64) {
        self.bytes += bytes;
        self.instructions += 1;
    }
}

fn top(map: BTreeMap<String, Tally>, n: usize) -> Vec<(String, Tally)> {
    let mut v: Vec<(String, Tally)> = map.into_iter().collect();
    v.sort_by(|a, b| b.1.bytes.cmp(&a.1.bytes).then_with(|| a.0.cmp(&b.0)));
    v.truncate(n);
    v
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StageReport {
    pub selection: String,
    pub bytes: u64,
    pub instructions: u64,
    /// Distinct nodes met at each stage, tracing back from the selection.
    pub nodes_by_stage: BTreeMap<String, u64>,
    /// Origin edges inside the traced closure, by `label/introduced_by`.
    pub edges_by_phase: BTreeMap<String, u64>,
    /// Bytes by the earliest stage an instruction's provenance reaches.
    pub provenance_ends_at: BTreeMap<String, Tally>,
    /// Bytes by the trace's attribution gap reason (when it has one).
    pub gap_reasons: BTreeMap<String, Tally>,
    /// Bytes by the post-optimization Sonatina operations an instruction
    /// lowers from (joined with `+` when several).
    pub by_postopt_operation: Vec<(String, Tally)>,
    /// Bytes by the MIR statement forms an instruction lowers from.
    pub by_mir_operation: Vec<(String, Tally)>,
    /// Bytes by Fe's primary source, as `file:line`.
    pub by_primary_span: Vec<(String, Tally)>,
    /// Bytes by the MIR runtime instance (the specialized function) an
    /// instruction lowers from.
    pub by_mir_instance: Vec<(String, Tally)>,
    /// Memory operations only: bytes and instructions by origin bucket
    /// ([`memory_bucket`]).
    pub memory_buckets: BTreeMap<String, Tally>,
}

/// Where a memory operation came from: a mechanism and a library,
/// `mechanism | library`. The mechanism is the first rule that holds:
///
/// 1. inside a selection named `free_pointer_clamp`: the allocation clamp;
/// 2. inside a selection named `backend_spill`: a spill store or reload the
///    EVM backend's stack scheduler inserted (tags from Sonatina's memory plan);
/// 3. provenance ends after Sonatina post-opt: other backend code with no
///    Sonatina IR origin;
/// 4. lowers from a post-opt `evm_malloc`: allocation;
/// 5. lowers from a MIR `copy_into`, or a post-opt `evm_mcopy` or `memzero`:
///    an aggregate copy;
/// 6. lowers from post-opt `obj.*` operations: Sonatina stack objects (locals
///    and aggregates the backend placed in memory);
/// 7. lowers from post-opt `mload`/`mstore`: an explicit memory access;
/// 8. lowers only from post-opt `call` or `return`: argument and result
///    transport at internal calls;
/// 9. anything else, named by its post-opt operations.
///
/// The library is ABI when a MIR instance or the primary source body is in
/// Fe's ABI code (`$abi$`, `calldata`, `dyn_array`), `core ptr` for memory
/// buffers, `port` for the port's own code (`Local$`), else another library
/// or unknown.
pub fn memory_bucket(
    in_clamp: bool,
    in_spill: bool,
    ends_at: Stage,
    postopt_ops: &BTreeSet<String>,
    mir_ops: &BTreeSet<String>,
    origins: &[&str],
) -> String {
    let has = |p: &str| postopt_ops.iter().any(|o| o.starts_with(p));
    let mechanism = if in_clamp {
        "allocation: free-pointer clamp".to_string()
    } else if in_spill {
        "backend: spill store or reload (stack scheduler)".to_string()
    } else if ends_at > Stage::PostOpt {
        "backend: other code with no Sonatina IR origin".to_string()
    } else if has("evm_malloc") {
        "allocation: evm_malloc".to_string()
    } else if mir_ops.iter().any(|m| m.contains("copy_into")) || has("evm_mcopy") || has("memzero")
    {
        "aggregate copy (MIR copy_into, mcopy, memzero)".to_string()
    } else if has("obj.") {
        "stack object access (obj.*)".to_string()
    } else if has("mload") || has("mstore") {
        "explicit memory access (mload/mstore)".to_string()
    } else if !postopt_ops.is_empty() && postopt_ops.iter().all(|o| o == "call" || o == "return") {
        "call and return transport (post-opt call/return)".to_string()
    } else if postopt_ops.is_empty() {
        "no post-opt operation".to_string()
    } else {
        format!(
            "other: {}",
            postopt_ops.iter().cloned().collect::<Vec<_>>().join("+")
        )
    };
    let is_abi = |b: &str| b.contains("$abi$") || b.contains("calldata") || b.contains("dyn_array");
    let library = if origins.iter().any(|b| is_abi(b)) {
        "ABI"
    } else if origins.iter().any(|b| b.contains("$ptr$")) {
        "core ptr"
    } else if origins.iter().any(|b| b.contains("Local$")) {
        "port"
    } else if origins.is_empty() {
        "unknown"
    } else {
        "other library"
    };
    format!("{mechanism} | {library}")
}

/// The joined inputs: the stage graph, Fe's attribution rows, and the code.
pub struct StageInputs<'a> {
    pub graph: &'a StageGraph,
    pub rows: &'a [DetailsRow],
    pub code: &'a [u8],
}

struct PcFacts {
    node: Option<u32>,
    bytes: u64,
    ends_at: Stage,
    postopt_ops: BTreeSet<String>,
    mir_ops: BTreeSet<String>,
    mir_instances: BTreeSet<String>,
}

fn mir_instance(key: &str) -> Option<String> {
    let (_, owner, _) = riff_catalog_ingest_trace::bytes::key_parts(key)?;
    let rest = owner.strip_prefix("runtime-instance:semantic:")?;
    Some(rest.split(":params").next().unwrap_or(rest).to_string())
}

impl StageInputs<'_> {
    fn pc_facts(&self, row: &DetailsRow) -> PcFacts {
        let g = self.graph;
        let node = g.node(&row.instruction_key);
        let mut facts = PcFacts {
            node,
            bytes: u64::from(row.pc_end - row.pc_start),
            ends_at: Stage::Bytecode,
            postopt_ops: BTreeSet::new(),
            mir_ops: BTreeSet::new(),
            mir_instances: BTreeSet::new(),
        };
        if let Some(n) = node {
            for m in g.trace_back([n]) {
                let stage = g.stage(m);
                if stage != Stage::Other && stage < facts.ends_at {
                    facts.ends_at = stage;
                }
                match stage {
                    Stage::PostOpt => {
                        if let Some(op) = g.operation(m) {
                            facts.postopt_ops.insert(op);
                        }
                    }
                    Stage::Mir => {
                        if let Some(op) = g.operation(m) {
                            facts.mir_ops.insert(op);
                        }
                        if let Some(i) = mir_instance(g.key(m)) {
                            facts.mir_instances.insert(i);
                        }
                    }
                    _ => {}
                }
            }
        }
        facts
    }

    fn span_of(&self, origin: &str) -> String {
        let g = self.graph;
        g.node(origin)
            .and_then(|n| g.span(n))
            .map(|s| {
                let file = s.file.rsplit('/').next().unwrap_or(&s.file);
                format!("{file}:{}", s.start_line)
            })
            .unwrap_or_else(|| "(no span)".into())
    }

    /// For each range, the first instruction at or after its end (within
    /// `limit` instructions) that lowers from a post-opt operation named
    /// `op`. It ties code with no provenance of its own (such as the
    /// allocation clamp) to the operation it was emitted for.
    pub fn nearest_after(
        &self,
        name: &str,
        ranges: &[(u32, u32)],
        op: &str,
        limit: usize,
    ) -> Selection {
        let g = self.graph;
        let mut pcs = BTreeSet::new();
        for (_, end) in ranges {
            let first = self.rows.partition_point(|r| r.pc_start < *end);
            for row in self.rows[first..].iter().take(limit) {
                let lowers = g.node(&row.instruction_key).is_some_and(|n| {
                    g.trace_back([n]).into_iter().any(|m| {
                        g.stage(m) == Stage::PostOpt && g.operation(m).as_deref() == Some(op)
                    })
                });
                if lowers {
                    pcs.insert(row.pc_start);
                    break;
                }
            }
        }
        Selection {
            name: name.into(),
            pcs,
        }
    }

    /// Trace one selection back through the stages.
    pub fn report(
        &self,
        selection: &Selection,
        clamp: &BTreeSet<u32>,
        spill: &BTreeSet<u32>,
        n: usize,
    ) -> StageReport {
        let g = self.graph;
        let mut report = StageReport {
            selection: selection.name.clone(),
            bytes: 0,
            instructions: 0,
            nodes_by_stage: BTreeMap::new(),
            edges_by_phase: BTreeMap::new(),
            provenance_ends_at: BTreeMap::new(),
            gap_reasons: BTreeMap::new(),
            by_postopt_operation: Vec::new(),
            by_mir_operation: Vec::new(),
            by_primary_span: Vec::new(),
            by_mir_instance: Vec::new(),
            memory_buckets: BTreeMap::new(),
        };
        let mut post: BTreeMap<String, Tally> = BTreeMap::new();
        let mut mir: BTreeMap<String, Tally> = BTreeMap::new();
        let mut spans: BTreeMap<String, Tally> = BTreeMap::new();
        let mut instances: BTreeMap<String, Tally> = BTreeMap::new();
        let mut starts = Vec::new();
        for row in self
            .rows
            .iter()
            .filter(|r| selection.pcs.contains(&r.pc_start))
        {
            let f = self.pc_facts(row);
            report.bytes += f.bytes;
            report.instructions += 1;
            starts.extend(f.node);
            report
                .provenance_ends_at
                .entry(f.ends_at.as_str().into())
                .or_default()
                .add(f.bytes);
            if let Some(reason) = f.node.and_then(|n| g.gap(n)) {
                report
                    .gap_reasons
                    .entry(reason.into())
                    .or_default()
                    .add(f.bytes);
            }
            let join = |s: &BTreeSet<String>| {
                if s.is_empty() {
                    "(none)".to_string()
                } else {
                    s.iter().cloned().collect::<Vec<_>>().join("+")
                }
            };
            post.entry(join(&f.postopt_ops)).or_default().add(f.bytes);
            mir.entry(join(&f.mir_ops)).or_default().add(f.bytes);
            let span = row
                .primary_source
                .as_deref()
                .map(|p| self.span_of(p))
                .unwrap_or_else(|| "(no primary source)".into());
            spans.entry(span).or_default().add(f.bytes);
            for i in &f.mir_instances {
                instances.entry(i.clone()).or_default().add(f.bytes);
            }
            let opcode = self.code.get(row.pc_start as usize).copied().unwrap_or(0);
            if MEMORY_OPCODES.contains(&opcode) {
                let mut origins: Vec<&str> = row
                    .primary_source
                    .as_deref()
                    .and_then(source_body)
                    .into_iter()
                    .collect();
                origins.extend(f.mir_instances.iter().map(String::as_str));
                let bucket = memory_bucket(
                    clamp.contains(&row.pc_start),
                    spill.contains(&row.pc_start),
                    f.ends_at,
                    &f.postopt_ops,
                    &f.mir_ops,
                    &origins,
                );
                report
                    .memory_buckets
                    .entry(bucket)
                    .or_default()
                    .add(f.bytes);
            }
        }
        let closure = g.trace_back(starts);
        for &m in &closure {
            *report
                .nodes_by_stage
                .entry(g.stage(m).as_str().into())
                .or_default() += 1;
            for link in g.links(m) {
                if closure.contains(&link.target) {
                    let phase = link.phase.map_or("-", |p| g.phases[p as usize].as_str());
                    let key = format!("{}/{phase}", g.labels[link.label as usize]);
                    *report.edges_by_phase.entry(key).or_default() += 1;
                }
            }
        }
        report.by_postopt_operation = top(post, n);
        report.by_mir_operation = top(mir, n);
        report.by_primary_span = top(spans, n);
        report.by_mir_instance = top(instances, n);
        report
    }

    /// Per Fe source body: HIR nodes in the body, and the nodes at every
    /// later stage and the emitted bytes whose provenance reaches the body
    /// (a node reaching several bodies counts for each), plus the bytes whose
    /// primary source is in the body (each byte once).
    pub fn expansion_by_body(&self) -> Vec<BodyExpansion> {
        let g = self.graph;
        let n = g.len();
        // Bodies reached by each node, memoized over the acyclic origin graph.
        let mut reached: Vec<Option<Vec<u32>>> = vec![None; n];
        let mut body_ids: HashMap<String, u32> = HashMap::new();
        let mut body_names: Vec<String> = Vec::new();
        let mut body_of_hir = |key: &str| -> Option<u32> {
            let b = source_body(key)?;
            let next = body_names.len() as u32;
            let id = *body_ids.entry(b.to_string()).or_insert_with(|| {
                body_names.push(b.to_string());
                next
            });
            Some(id)
        };
        for id in 0..n as u32 {
            if g.stage(id) == Stage::Hir {
                reached[id as usize] = Some(body_of_hir(g.key(id)).into_iter().collect());
            }
        }
        for start in 0..n as u32 {
            if reached[start as usize].is_some() {
                continue;
            }
            // Iterative post-order DFS.
            let mut stack: Vec<(u32, bool)> = vec![(start, false)];
            let mut on_stack: BTreeSet<u32> = BTreeSet::new();
            while let Some((id, done)) = stack.pop() {
                if reached[id as usize].is_some() {
                    continue;
                }
                if done {
                    let mut set: Vec<u32> = g
                        .links(id)
                        .iter()
                        .filter_map(|l| reached[l.target as usize].as_ref())
                        .flatten()
                        .copied()
                        .collect();
                    set.sort_unstable();
                    set.dedup();
                    reached[id as usize] = Some(set);
                    on_stack.remove(&id);
                    continue;
                }
                on_stack.insert(id);
                stack.push((id, true));
                for l in g.links(id) {
                    if reached[l.target as usize].is_none() && !on_stack.contains(&l.target) {
                        stack.push((l.target, false));
                    }
                }
            }
        }
        let mut rows: BTreeMap<u32, BodyExpansion> = BTreeMap::new();
        fn entry<'r>(
            rows: &'r mut BTreeMap<u32, BodyExpansion>,
            names: &[String],
            b: u32,
        ) -> &'r mut BodyExpansion {
            rows.entry(b).or_insert_with(|| BodyExpansion {
                body: names[b as usize].clone(),
                ..BodyExpansion::default()
            })
        }
        let pcs: HashMap<u32, &DetailsRow> = self
            .rows
            .iter()
            .filter_map(|r| g.node(&r.instruction_key).map(|n| (n, r)))
            .collect();
        for id in 0..n as u32 {
            let stage = g.stage(id);
            if stage == Stage::Bytecode && !pcs.contains_key(&id) {
                continue; // another code object
            }
            for &b in reached[id as usize].as_deref().unwrap_or(&[]) {
                let e = entry(&mut rows, &body_names, b);
                match stage {
                    Stage::Hir => e.hir += 1,
                    Stage::Mir => e.mir += 1,
                    Stage::PreOpt => e.preopt += 1,
                    Stage::PostOpt => e.postopt += 1,
                    Stage::Prepared => e.prepared += 1,
                    Stage::Vcode => e.vcode += 1,
                    Stage::Bytecode => {
                        e.instructions_reaching += 1;
                        e.bytes_reaching += u64::from(pcs[&id].pc_end - pcs[&id].pc_start);
                    }
                    Stage::Other => {}
                }
            }
        }
        for r in self.rows {
            if let Some(b) = r
                .primary_source
                .as_deref()
                .and_then(source_body)
                .and_then(|b| body_ids.get(b))
            {
                entry(&mut rows, &body_names, *b).bytes_primary += u64::from(r.pc_end - r.pc_start);
            }
        }
        let mut out: Vec<BodyExpansion> = rows
            .into_values()
            .filter(|r| r.bytes_reaching > 0 || r.bytes_primary > 0)
            .collect();
        out.sort_by(|a, b| {
            b.bytes_reaching
                .cmp(&a.bytes_reaching)
                .then_with(|| a.body.cmp(&b.body))
        });
        out
    }

    /// Content-address the chain from every HIR construct that is the
    /// primary source of emitted bytes, and group constructs by address.
    pub fn chain_classes(&self, n: usize) -> Result<Vec<ChainClass>> {
        let g = self.graph;
        let mut by_primary: BTreeMap<u32, Vec<&DetailsRow>> = BTreeMap::new();
        for r in self.rows {
            if let Some(h) = r.primary_source.as_deref().and_then(|p| g.node(p)) {
                by_primary.entry(h).or_default().push(r);
            }
        }
        let policy = HashPolicy::new(
            STAGE_CHAIN_LEVEL,
            ViewMode::AnonymousShape,
            CyclePolicy::Reject,
        )?;
        let facet = Facet::structure_only(policy.policy_id());
        let mut classes: BTreeMap<String, ChainClass> = BTreeMap::new();
        for (h, rows) in by_primary {
            let pcs: Vec<u32> = rows
                .iter()
                .filter_map(|r| g.node(&r.instruction_key))
                .collect();
            let forward = g.trace_forward([h]);
            let members: BTreeSet<u32> = g
                .trace_back(pcs)
                .into_iter()
                .filter(|m| forward.contains(m))
                .collect();
            let graph = g.chain_graph(h, &members)?;
            let request = DigestRequest::new(
                graph.graph_key.clone(),
                policy.clone(),
                facet.dimensions.iter().copied(),
            )?;
            let address = digest_graph(&request, &graph)
                .context("digest chain")?
                .hashes
                .facet_address(&facet)?
                .address_digest()
                .to_hex();
            let bytes: u64 = rows.iter().map(|r| u64::from(r.pc_end - r.pc_start)).sum();
            let mut stages: BTreeMap<String, u64> = BTreeMap::new();
            for m in &members {
                *stages.entry(g.stage(*m).as_str().into()).or_default() += 1;
            }
            let class = classes
                .entry(address.clone())
                .or_insert_with(|| ChainClass {
                    address,
                    constructs: 0,
                    bytes_per_construct: Vec::new(),
                    total_bytes: 0,
                    nodes_by_stage: stages,
                    spans: BTreeMap::new(),
                });
            class.constructs += 1;
            class.bytes_per_construct.push(bytes);
            class.total_bytes += bytes;
            *class.spans.entry(self.span_of(g.key(h))).or_default() += 1;
        }
        let mut out: Vec<ChainClass> = classes.into_values().filter(|c| c.constructs > 1).collect();
        out.sort_by(|a, b| {
            b.total_bytes
                .cmp(&a.total_bytes)
                .then_with(|| a.address.cmp(&b.address))
        });
        let total = out.len();
        out.truncate(n);
        if let Some(first) = out.first_mut() {
            first
                .spans
                .insert(format!("(classes with 2+ constructs: {total})"), 0);
        }
        for c in &mut out {
            c.bytes_per_construct.sort_unstable();
            c.bytes_per_construct.dedup();
            if c.spans.len() > 8 {
                let mut v: Vec<(String, u64)> = c.spans.clone().into_iter().collect();
                v.sort_by(|a, b| b.1.cmp(&a.1));
                c.spans = v.into_iter().take(8).collect();
            }
        }
        Ok(out)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BodyExpansion {
    pub body: String,
    pub hir: u64,
    pub mir: u64,
    pub preopt: u64,
    pub postopt: u64,
    pub prepared: u64,
    pub vcode: u64,
    pub instructions_reaching: u64,
    pub bytes_reaching: u64,
    pub bytes_primary: u64,
}

/// HIR constructs whose traced chains have one address at the Structure
/// facet of [`STAGE_CHAIN_LEVEL`]: the same stages, operations and fan-out.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChainClass {
    pub address: String,
    pub constructs: usize,
    /// Distinct byte counts among the constructs (equal Structure does not
    /// force equal bytes: PUSH widths and immediates are Constants).
    pub bytes_per_construct: Vec<u64>,
    pub total_bytes: u64,
    pub nodes_by_stage: BTreeMap<String, u64>,
    /// `file:line` of the constructs, with counts.
    pub spans: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FeStagesReport {
    pub schema: String,
    pub contract: String,
    pub stage_graph_nodes: usize,
    pub selections: Vec<StageReport>,
    pub expansion_by_body: Vec<BodyExpansion>,
    pub chain_classes: Vec<ChainClass>,
}

/// Non-overlapping byte ranges matching `pattern` (hex, `??` any byte),
/// scanning left to right.
pub fn pattern_matches(code: &[u8], pattern: &str) -> Result<Vec<(u32, u32)>> {
    let hex: String = pattern.chars().filter(|c| !c.is_whitespace()).collect();
    anyhow::ensure!(
        hex.len() % 2 == 0 && !hex.is_empty(),
        "pattern must be whole bytes"
    );
    let bytes: Vec<Option<u8>> = (0..hex.len())
        .step_by(2)
        .map(|i| match &hex[i..i + 2] {
            "??" => Ok(None),
            h => u8::from_str_radix(h, 16).map(Some),
        })
        .collect::<Result<_, _>>()
        .context("pattern hex")?;
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + bytes.len() <= code.len() {
        let hit = bytes
            .iter()
            .enumerate()
            .all(|(k, b)| b.is_none_or(|b| code[at + k] == b));
        if hit {
            out.push((at as u32, (at + bytes.len()) as u32));
            at += bytes.len();
        } else {
            at += 1;
        }
    }
    Ok(out)
}

/// pcs of instructions that start inside a match of `pattern`.
pub fn pattern_selection(name: &str, code: &[u8], pattern: &str) -> Result<Selection> {
    Ok(range_selection(
        name,
        code,
        &pattern_matches(code, pattern)?,
    ))
}

/// pcs of the first instruction after each match of `pattern`: the code a
/// pattern with no provenance of its own sits in front of.
pub fn pattern_next_selection(name: &str, code: &[u8], pattern: &str) -> Result<Selection> {
    let starts: BTreeSet<u32> = decode(code).iter().map(|i| i.pc).collect();
    Ok(Selection {
        name: name.into(),
        pcs: pattern_matches(code, pattern)?
            .into_iter()
            .filter_map(|(_, end)| starts.range(end..).next().copied())
            .collect(),
    })
}

/// pcs of every instruction whose opcode is in `opcodes`.
pub fn opcode_selection(name: &str, code: &[u8], opcodes: &[u8]) -> Selection {
    Selection {
        name: name.into(),
        pcs: decode(code)
            .into_iter()
            .filter(|i| opcodes.contains(&i.opcode))
            .map(|i| i.pc)
            .collect(),
    }
}

/// pcs of every instruction inside the given byte ranges.
pub fn range_selection(name: &str, code: &[u8], ranges: &[(u32, u32)]) -> Selection {
    Selection {
        name: name.into(),
        pcs: decode(code)
            .into_iter()
            .filter(|i| ranges.iter().any(|(s, e)| *s <= i.pc && i.pc < *e))
            .map(|i| i.pc)
            .collect(),
    }
}

/// Render the headline numbers.
pub fn render_fe_stages(report: &FeStagesReport, n: usize) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for s in &report.selections {
        let _ = writeln!(
            out,
            "\n== {}: {} bytes, {} instructions",
            s.selection, s.bytes, s.instructions
        );
        let stages: Vec<String> = Stage::PIPELINE
            .iter()
            .map(|st| {
                format!(
                    "{} {}",
                    st.as_str(),
                    s.nodes_by_stage.get(st.as_str()).copied().unwrap_or(0)
                )
            })
            .collect();
        let _ = writeln!(out, "   nodes by stage: {}", stages.join(", "));
        let _ = writeln!(out, "   edges: {:?}", s.edges_by_phase);
        let ends: Vec<String> = s
            .provenance_ends_at
            .iter()
            .map(|(k, t)| format!("{k} {}", t.bytes))
            .collect();
        let _ = writeln!(out, "   provenance ends at (bytes): {}", ends.join(", "));
        if !s.gap_reasons.is_empty() {
            let gaps: Vec<String> = s
                .gap_reasons
                .iter()
                .map(|(k, t)| format!("{k} {}", t.bytes))
                .collect();
            let _ = writeln!(out, "   gap reasons (bytes): {}", gaps.join(", "));
        }
        if !s.memory_buckets.is_empty() {
            let _ = writeln!(out, "   memory operations by origin (instructions, bytes):");
            let mut v: Vec<_> = s.memory_buckets.iter().collect();
            v.sort_by(|a, b| b.1.instructions.cmp(&a.1.instructions));
            for (k, t) in v {
                let _ = writeln!(out, "     {:>6} {:>6}  {k}", t.instructions, t.bytes);
            }
        }
        for (title, rows) in [
            ("post-opt operations", &s.by_postopt_operation),
            ("MIR forms", &s.by_mir_operation),
            ("primary source", &s.by_primary_span),
            ("MIR instance", &s.by_mir_instance),
        ] {
            let _ = writeln!(out, "   by {title}:");
            for (k, t) in rows.iter().take(n) {
                let k: String = k.chars().take(150).collect();
                let _ = writeln!(out, "     {:>6} {:>5}  {k}", t.bytes, t.instructions);
            }
        }
    }
    let _ = writeln!(
        out,
        "\n== expansion by Fe body (bytes reaching; hir, mir, preopt, postopt, vcode; bytes primary)"
    );
    for b in report.expansion_by_body.iter().take(n) {
        let name: String = b.body.chars().take(90).collect();
        let _ = writeln!(
            out,
            "   {:>6}  {:>4} {:>5} {:>5} {:>5} {:>5}  {:>6}  {name}",
            b.bytes_reaching, b.hir, b.mir, b.preopt, b.postopt, b.vcode, b.bytes_primary
        );
    }
    let _ = writeln!(
        out,
        "\n== chain classes (constructs with one expansion shape)"
    );
    for c in report.chain_classes.iter().take(n) {
        let _ = writeln!(
            out,
            "   {:>6} bytes, {:>4} constructs, bytes each {:?}, stages {:?}\n      {:?}",
            c.total_bytes, c.constructs, c.bytes_per_construct, c.nodes_by_stage, c.spans
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_selection_takes_every_instruction_in_a_match() {
        // PUSH1 0x40 MLOAD | PUSH1 0x40 MLOAD with a wildcard on the value.
        let code = [0x60, 0x40, 0x51, 0x00, 0x60, 0x41, 0x51];
        let s = pattern_selection("p", &code, "60 ?? 51").unwrap();
        assert_eq!(s.pcs.into_iter().collect::<Vec<_>>(), vec![0, 2, 4, 6]);
        let next = pattern_next_selection("n", &code, "60 40 51").unwrap();
        assert_eq!(next.pcs.into_iter().collect::<Vec<_>>(), vec![3]);
        let m = opcode_selection("m", &code, &MEMORY_OPCODES);
        assert_eq!(m.pcs.len(), 2);
    }

    #[test]
    fn memory_buckets_follow_the_rules_in_order() {
        let ops = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>();
        let none = ops(&[]);
        assert_eq!(
            memory_bucket(true, false, Stage::Hir, &ops(&["obj.store"]), &none, &[]),
            "allocation: free-pointer clamp | unknown"
        );
        assert!(memory_bucket(false, true, Stage::Hir, &none, &none, &[]).contains("spill"));
        assert!(
            memory_bucket(false, false, Stage::Vcode, &none, &none, &[]).starts_with("backend")
        );
        assert_eq!(
            memory_bucket(
                false,
                false,
                Stage::Hir,
                &ops(&["obj.load"]),
                &ops(&["_ = copy_into"]),
                &["func$Local$seaport$lib$x"]
            ),
            "aggregate copy (MIR copy_into, mcopy, memzero) | port"
        );
        assert_eq!(
            memory_bucket(
                false,
                false,
                Stage::Hir,
                &ops(&["obj.load"]),
                &none,
                &["func$Core$core$lib$abi$fn$x", "func$Local$y"]
            ),
            "stack object access (obj.*) | ABI"
        );
        assert!(
            memory_bucket(false, false, Stage::Hir, &ops(&["call"]), &none, &[])
                .starts_with("call and return")
        );
    }
}
