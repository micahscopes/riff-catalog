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

use crate::census_input::CensusRunClass;
use crate::regions::FunctionRegions;
use crate::selection::{
    Selection, opcode_selection, pattern_selection, push_selection, range_selection,
};
use anyhow::{Context, Result};
use riff_catalog_core::{CyclePolicy, DigestRequest, Facet, HashPolicy, ViewMode, digest_graph};
pub use riff_catalog_evm::decode::MEMORY_OPCODES;
use riff_catalog_ingest_trace::bytes::{DetailsRow, Tally, source_body};
use riff_catalog_ingest_trace::stages::{Stage, StageGraph};
use serde::{Deserialize, Serialize};

pub const FE_STAGES_SCHEMA: &str = "riffcat-fe-stages/1";
/// Level of chain graphs ([`StageGraph::chain_graph`]).
pub const STAGE_CHAIN_LEVEL: &str = "fe-stage-chain/1";

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
    /// Sonatina pre-opt instructions lowered from the MIR nodes met. Post-opt
    /// instructions link to MIR directly, so tracing back never meets
    /// pre-opt; this counts them forward from MIR instead.
    pub preopt_from_traced_mir: u64,
    /// Bytes by the trace's instruction category.
    pub by_category: BTreeMap<String, Tally>,
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

/// What `fe-trace-stages` traces besides its three built-in selections (all
/// instructions, memory operations, no-source instructions).
pub struct StageRequest<'a> {
    pub contract: &'a str,
    pub functions: &'a FunctionRegions,
    /// Named byte patterns (`name`, hex with `??` for any byte).
    pub patterns: &'a [(String, String)],
    /// Named pc sets.
    pub pc_sets: &'a [Selection],
    /// EVM run classes from a census; the `runs` largest are traced.
    pub census_runs: &'a [CensusRunClass],
    pub runs: usize,
    /// Trace every function whose name starts with one of these, and its
    /// call sites.
    pub function_prefixes: &'a [String],
    pub top: usize,
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
    /// The selections of a request, with the pcs of the selections named
    /// `free_pointer_clamp` and `backend_spill`.
    pub fn selections(
        &self,
        request: &StageRequest,
    ) -> Result<(Vec<Selection>, BTreeSet<u32>, BTreeSet<u32>)> {
        let code = self.code;
        let mut selections = vec![
            range_selection("all", code, &[(0, code.len() as u32)]),
            opcode_selection("memory_ops", code, &MEMORY_OPCODES),
            Selection {
                name: "no_source".into(),
                pcs: self
                    .rows
                    .iter()
                    .filter(|r| r.has_no_source())
                    .map(|r| r.pc_start)
                    .collect(),
            },
        ];
        let mut clamp = BTreeSet::new();
        for (name, hex) in request.patterns {
            let sel = pattern_selection(name, code, hex)?;
            if name == "free_pointer_clamp" {
                clamp = sel.pcs.clone();
            }
            selections.push(sel);
        }
        let mut spill = BTreeSet::new();
        for sel in request.pc_sets {
            crate::selection::check_instruction_starts(sel, code)?;
            if sel.name == "backend_spill" {
                spill = sel.pcs.clone();
            }
            selections.push(sel.clone());
        }
        let mut runs: Vec<&CensusRunClass> = request.census_runs.iter().collect();
        runs.sort_by_key(|r| std::cmp::Reverse(r.covered_bytes));
        for run in runs.into_iter().take(request.runs) {
            let name = format!(
                "run {} ({} copies x {} bytes)",
                &run.digest[..12.min(run.digest.len())],
                run.ranges.len(),
                run.ranges.first().map_or(0, |r| r.1 - r.0)
            );
            selections.push(range_selection(&name, code, &run.ranges));
        }
        for prefix in request.function_prefixes {
            for f in request
                .functions
                .iter()
                .filter(|f| f.name.starts_with(prefix.as_str()))
            {
                selections.push(range_selection(
                    &format!("function {}", f.name),
                    code,
                    &[(f.start, f.end)],
                ));
                selections.push(push_selection(
                    &format!("call sites of {} (entry label pushes)", f.name),
                    code,
                    f.start,
                ));
            }
        }
        Ok((selections, clamp, spill))
    }

    /// The whole stage report for a request.
    pub fn stage_report(&self, request: &StageRequest) -> Result<FeStagesReport> {
        let (selections, clamp, spill) = self.selections(request)?;
        let (chain_classes, chain_class_count, top_constructs) =
            self.chain_classes(request.top.max(40))?;
        Ok(FeStagesReport {
            schema: FE_STAGES_SCHEMA.into(),
            contract: request.contract.to_string(),
            stage_graph_nodes: self.graph.len(),
            selections: selections
                .iter()
                .map(|s| self.report(s, &clamp, &spill, request.top))
                .collect(),
            expansion_by_body: self.expansion_by_body(),
            chain_class_count,
            chain_classes,
            top_constructs,
            category_by_function: self.category_by_function(request.functions),
        })
    }

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
            preopt_from_traced_mir: 0,
            by_category: BTreeMap::new(),
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
                .add_bytes(f.bytes);
            let category = f.node.and_then(|n| g.category(n)).unwrap_or("(none)");
            report
                .by_category
                .entry(category.into())
                .or_default()
                .add_bytes(f.bytes);
            if let Some(reason) = f.node.and_then(|n| g.gap(n)) {
                report
                    .gap_reasons
                    .entry(reason.into())
                    .or_default()
                    .add_bytes(f.bytes);
            }
            let join = |s: &BTreeSet<String>| {
                if s.is_empty() {
                    "(none)".to_string()
                } else {
                    s.iter().cloned().collect::<Vec<_>>().join("+")
                }
            };
            post.entry(join(&f.postopt_ops))
                .or_default()
                .add_bytes(f.bytes);
            mir.entry(join(&f.mir_ops)).or_default().add_bytes(f.bytes);
            let span = row
                .primary_source
                .as_deref()
                .map(|p| self.span_of(p))
                .unwrap_or_else(|| "(no primary source)".into());
            spans.entry(span).or_default().add_bytes(f.bytes);
            for i in &f.mir_instances {
                instances.entry(i.clone()).or_default().add_bytes(f.bytes);
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
                    .add_bytes(f.bytes);
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
        report.preopt_from_traced_mir = self.preopt_from_mir(&closure);
        report.by_postopt_operation = top(post, n);
        report.by_mir_operation = top(mir, n);
        report.by_primary_span = top(spans, n);
        report.by_mir_instance = top(instances, n);
        report
    }

    fn preopt_from_mir(&self, nodes: &BTreeSet<u32>) -> u64 {
        let g = self.graph;
        let mut preopt = BTreeSet::new();
        for &m in nodes.iter().filter(|m| g.stage(**m) == Stage::Mir) {
            preopt.extend(
                g.lowered_into(m)
                    .iter()
                    .copied()
                    .filter(|p| g.stage(*p) == Stage::PreOpt),
            );
        }
        preopt.len() as u64
    }

    /// Every instruction's mechanism ([`memory_bucket`] without the library
    /// part), for all opcodes, not only memory ones: the Fe or Sonatina
    /// construct the bytes were emitted for. Returns mechanism to pcs.
    pub fn byte_mechanisms(
        &self,
        clamp: &BTreeSet<u32>,
        spill: &BTreeSet<u32>,
    ) -> BTreeMap<String, BTreeSet<u32>> {
        let mut out: BTreeMap<String, BTreeSet<u32>> = BTreeMap::new();
        for row in self.rows {
            let f = self.pc_facts(row);
            let bucket = memory_bucket(
                clamp.contains(&row.pc_start),
                spill.contains(&row.pc_start),
                f.ends_at,
                &f.postopt_ops,
                &f.mir_ops,
                &[],
            );
            let mechanism = bucket.split(" | ").next().unwrap_or(&bucket).to_string();
            out.entry(mechanism).or_default().insert(row.pc_start);
        }
        out
    }

    /// Bytes by the trace's instruction category per function region,
    /// largest functions first.
    pub fn category_by_function(
        &self,
        functions: &FunctionRegions,
    ) -> Vec<(String, u64, BTreeMap<String, u64>)> {
        let mut out: Vec<(String, u64, BTreeMap<String, u64>)> = functions
            .iter()
            .map(|f| {
                let (name, start, end) = (&f.name, &f.start, &f.end);
                let mut cats: BTreeMap<String, u64> = BTreeMap::new();
                let first = self.rows.partition_point(|r| r.pc_start < *start);
                for r in self.rows[first..].iter().take_while(|r| r.pc_start < *end) {
                    let c = self
                        .graph
                        .node(&r.instruction_key)
                        .and_then(|n| self.graph.category(n))
                        .unwrap_or("(none)");
                    *cats.entry(c.into()).or_default() += u64::from(r.pc_end - r.pc_start);
                }
                (name.clone(), cats.values().sum(), cats)
            })
            .collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }

    /// Per Fe source body: the nodes at every stage and the emitted bytes of
    /// this code object whose provenance reaches the body (a node reaching
    /// several bodies counts for each), plus the bytes whose primary source
    /// is in the body (each byte once). Only nodes that feed this code object
    /// count (see the comment in the body).
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
        // Count only what feeds this code object: nodes met tracing back from
        // its instructions, and the pre-opt instructions lowered from those MIR
        // nodes inside the same Sonatina module as the post-opt nodes met.
        let traced = g.trace_back(pcs.keys().copied());
        let module_of = |id: u32| {
            riff_catalog_ingest_trace::bytes::key_parts(g.key(id))
                .map(|(_, owner, _)| owner.to_string())
        };
        let modules: BTreeSet<String> = traced
            .iter()
            .filter(|m| g.stage(**m) == Stage::PostOpt)
            .filter_map(|m| module_of(*m))
            .collect();
        let mut counted: BTreeSet<u32> = traced.clone();
        for &m in traced.iter().filter(|m| g.stage(**m) == Stage::Mir) {
            counted.extend(g.lowered_into(m).iter().copied().filter(|p| {
                g.stage(*p) == Stage::PreOpt && module_of(*p).is_some_and(|o| modules.contains(&o))
            }));
        }
        for id in counted {
            let stage = g.stage(id);
            for &b in reached[id as usize].as_deref().unwrap_or(&[]) {
                let e = entry(&mut rows, &body_names, b);
                match stage {
                    Stage::Hir => e.hir += 1,
                    Stage::Mir => e.mir += 1,
                    Stage::PreOpt => e.preopt += 1,
                    Stage::PostOpt => e.postopt += 1,
                    Stage::Prepared => e.prepared += 1,
                    Stage::Vcode => e.vcode += 1,
                    // Only this code object's instructions have rows; a pc of
                    // another code object met on the way back is not counted.
                    Stage::Bytecode => {
                        if let Some(row) = pcs.get(&id) {
                            e.instructions_reaching += 1;
                            e.bytes_reaching += u64::from(row.pc_end - row.pc_start);
                        }
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
                let e = entry(&mut rows, &body_names, *b);
                let bytes = u64::from(r.pc_end - r.pc_start);
                e.bytes_primary += bytes;
                let category = g
                    .node(&r.instruction_key)
                    .and_then(|n| g.category(n))
                    .unwrap_or("(none)");
                *e.primary_by_category.entry(category.into()).or_default() += bytes;
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
    /// Returns the `n` largest classes of two or more constructs, how many
    /// such classes there are, and the `n` constructs with the most bytes.
    pub fn chain_classes(
        &self,
        n: usize,
    ) -> Result<(Vec<ChainClass>, usize, Vec<ConstructExpansion>)> {
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
        let mut constructs: Vec<ConstructExpansion> = Vec::new();
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
            constructs.push(ConstructExpansion {
                span: self.span_of(g.key(h)),
                chain: address.clone(),
                bytes,
                preopt: self.preopt_from_mir(&members),
                nodes_by_stage: stages.clone(),
            });
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
        constructs.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.span.cmp(&b.span)));
        constructs.truncate(n);
        let mut out: Vec<ChainClass> = classes.into_values().filter(|c| c.constructs > 1).collect();
        out.sort_by(|a, b| {
            b.total_bytes
                .cmp(&a.total_bytes)
                .then_with(|| a.address.cmp(&b.address))
        });
        let total = out.len();
        out.truncate(n);
        for c in &mut out {
            c.bytes_per_construct.sort_unstable();
            c.bytes_per_construct.dedup();
            if c.spans.len() > 8 {
                let mut v: Vec<(String, u64)> = c.spans.clone().into_iter().collect();
                v.sort_by(|a, b| b.1.cmp(&a.1));
                c.spans = v.into_iter().take(8).collect();
            }
        }
        Ok((out, total, constructs))
    }
}

/// One HIR construct's traced chain: its bytes, and the nodes per stage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConstructExpansion {
    pub span: String,
    pub chain: String,
    pub bytes: u64,
    pub preopt: u64,
    pub nodes_by_stage: BTreeMap<String, u64>,
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
    /// `bytes_primary` by the trace's instruction category.
    pub primary_by_category: BTreeMap<String, u64>,
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
    /// Chain classes with two or more constructs (`chain_classes` holds the
    /// largest of them).
    #[serde(default)]
    pub chain_class_count: usize,
    pub chain_classes: Vec<ChainClass>,
    /// The HIR constructs that are the primary source of the most bytes.
    pub top_constructs: Vec<ConstructExpansion>,
    /// Bytes by the trace's instruction category per emitted function.
    pub category_by_function: Vec<(String, u64, BTreeMap<String, u64>)>,
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
        let _ = writeln!(
            out,
            "   nodes by stage: {}; pre-opt from traced MIR {}",
            stages.join(", "),
            s.preopt_from_traced_mir
        );
        let cats: Vec<String> = s
            .by_category
            .iter()
            .map(|(k, t)| format!("{k} {}", t.bytes))
            .collect();
        let _ = writeln!(out, "   bytes by trace category: {}", cats.join(", "));
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
        "\n== expansion by Fe body, by primary bytes (bytes primary; hir, mir, preopt, postopt, vcode; post-opt per pre-opt)"
    );
    let mut by_primary: Vec<&BodyExpansion> = report.expansion_by_body.iter().collect();
    by_primary.sort_by(|a, b| {
        b.bytes_primary
            .cmp(&a.bytes_primary)
            .then_with(|| a.body.cmp(&b.body))
    });
    for b in by_primary.into_iter().take(n) {
        let name: String = b.body.chars().take(90).collect();
        let ratio = if b.preopt > 0 {
            format!("{:.2}", b.postopt as f64 / b.preopt as f64)
        } else {
            "-".into()
        };
        let _ = writeln!(
            out,
            "   {:>6}  {:>4} {:>5} {:>5} {:>5} {:>5}  {ratio:>5}  {name}",
            b.bytes_primary, b.hir, b.mir, b.preopt, b.postopt, b.vcode
        );
    }
    let _ = writeln!(
        out,
        "\n== top constructs by primary bytes (bytes; pre-opt; stages)"
    );
    for c in report.top_constructs.iter().take(n) {
        let _ = writeln!(
            out,
            "   {:>6}  {:>4}  {:?}  {}",
            c.bytes, c.preopt, c.nodes_by_stage, c.span
        );
    }
    let _ = writeln!(out, "\n== bytes by trace category per emitted function");
    for (name, total, cats) in report.category_by_function.iter().take(n) {
        let _ = writeln!(out, "   {total:>6}  {cats:?}  {name}");
    }
    let _ = writeln!(
        out,
        "\n== chain classes (constructs with one expansion shape): {} with 2+ constructs",
        report.chain_class_count
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
    fn a_runtime_pc_lowered_from_another_code_objects_pc_is_not_counted_as_runtime() {
        let key = |kind: &str, owner: &str, local: &str| serde_json::json!({"kind": kind, "owner_key": owner, "local_key": local});
        let pc = key("bytecode.pc", "C:runtime", "pc:0");
        let init = key("bytecode.pc", "C:init", "pc:7");
        let hir = key("hir.expr", "hir-body:b", "0");
        let lines = [
            serde_json::json!({"record": "metadata", "schema_version": 2}),
            serde_json::json!({"record": "fact", "type": "origin_edge", "from": pc,
                "to": init, "label": "copied_from", "introduced_by": "x"}),
            serde_json::json!({"record": "fact", "type": "origin_edge", "from": init,
                "to": hir, "label": "lowered_from", "introduced_by": "x"}),
        ];
        let text: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
        let graph = StageGraph::read(text.join("\n").as_bytes()).unwrap();
        let text = |k: &serde_json::Value| {
            format!(
                "{}\u{1f}{}\u{1f}{}",
                k["kind"].as_str().unwrap(),
                k["owner_key"].as_str().unwrap(),
                k["local_key"].as_str().unwrap()
            )
        };
        let rows = vec![DetailsRow {
            instruction_key: text(&pc),
            code_object: None,
            pc_start: 0,
            pc_end: 1,
            primary_source: Some(text(&hir)),
            all_origins: vec![text(&hir)],
            classification: "source_mapped".into(),
            classification_reason: None,
            confidence: "high".into(),
        }];
        let inputs = StageInputs {
            graph: &graph,
            rows: &rows,
            code: &[0x00],
        };
        let expansion = std::panic::catch_unwind(|| inputs.expansion_by_body())
            .expect("a pc of another code object made expansion_by_body panic");
        assert_eq!(expansion.len(), 1);
        assert_eq!(expansion[0].bytes_reaching, 1);
        assert_eq!(expansion[0].instructions_reaching, 1);
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
