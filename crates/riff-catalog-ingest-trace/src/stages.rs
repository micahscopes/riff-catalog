//! Stage view of a Fe trace bundle: the origin graph indexed by compiler
//! stage, so an observation at one stage (a set of emitted bytes) can be
//! followed back to every earlier stage, and an earlier construct forward to
//! what it became.
//!
//! This reads the same `origin_node` and `origin_edge` facts as
//! [`crate::ingest_trace_bundle`] (with the same line types and schema
//! check), plus the per-stage facts that say what each node is:
//! `instruction` (MIR statements and Sonatina instructions with their
//! mnemonic), `opcode` and `instruction_extent` (emitted bytes),
//! `source_span` and `source_file` (HIR source locations), `function` (names)
//! and `attribution_gap` (why an instruction has no source).
//!
//! Origin edges point from the later stage to the earlier one (`from`
//! lowered from `to`). The stage order is the pipeline order in
//! [`Stage`]. [`StageGraph::chain_graph`] turns a traced chain into a riff
//! [`Graph`] whose Structure is each node's stage and operation, so a hashing
//! engine can content-address expansion patterns. This crate still does no
//! hashing.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::io::BufRead;

use riff_catalog_schema::{CatalogError, Dimension, EntityKey, Graph, GraphKey, NodeKey};
use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::bytes::key_text;
use crate::bytes::{LedgerError, key_of, key_parts};
use crate::scan::scan_trace;

/// Compiler stages in pipeline order (earliest first).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Hir,
    Mir,
    PreOpt,
    PostOpt,
    Prepared,
    Vcode,
    Bytecode,
    Other,
}

impl Stage {
    pub const PIPELINE: [Stage; 7] = [
        Stage::Hir,
        Stage::Mir,
        Stage::PreOpt,
        Stage::PostOpt,
        Stage::Prepared,
        Stage::Vcode,
        Stage::Bytecode,
    ];

    /// Stage of an origin key kind.
    pub fn of_kind(kind: &str) -> Self {
        if kind.starts_with("hir.") {
            Self::Hir
        } else if kind.starts_with("runtime.") {
            Self::Mir
        } else if kind.starts_with("sonatina.preopt.") {
            Self::PreOpt
        } else if kind.starts_with("sonatina.postopt.") {
            Self::PostOpt
        } else if kind.starts_with("sonatina.evm.prepared.") {
            Self::Prepared
        } else if kind.starts_with("evm.vcode.") {
            Self::Vcode
        } else if kind.starts_with("bytecode.") {
            Self::Bytecode
        } else {
            Self::Other
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Hir => "hir",
            Self::Mir => "mir",
            Self::PreOpt => "sonatina_pre_opt",
            Self::PostOpt => "sonatina_post_opt",
            Self::Prepared => "sonatina_prepared",
            Self::Vcode => "evm_vcode",
            Self::Bytecode => "bytecode",
            Self::Other => "other",
        }
    }
}

/// One origin edge, stored on its later endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OriginLink {
    pub target: u32,
    /// Index into [`StageGraph::labels`].
    pub label: u32,
    /// Index into [`StageGraph::phases`]; `None` when the edge names no phase.
    pub phase: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpanRef {
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
}

/// The origin graph of one trace bundle, indexed by node.
#[derive(Default)]
pub struct StageGraph {
    keys: Vec<String>,
    index: HashMap<String, u32>,
    stage: Vec<Stage>,
    out: Vec<Vec<OriginLink>>,
    into: Vec<Vec<u32>>,
    pub labels: Vec<String>,
    pub phases: Vec<String>,
    label_index: HashMap<String, u32>,
    phase_index: HashMap<String, u32>,
    mnemonic: HashMap<u32, String>,
    immediate: HashMap<u32, String>,
    extent: HashMap<u32, (u32, u32)>,
    span: HashMap<u32, SpanRef>,
    gap: HashMap<u32, String>,
    /// The trace's `instruction_category` per instruction.
    category: HashMap<u32, String>,
    /// Instruction node to its function key (from `instruction` facts).
    function_of: HashMap<u32, String>,
    /// Function key to its name (from `function` facts).
    function_names: HashMap<String, String>,
}

use key_of as wire;

const STAGE_FACTS: [&str; 10] = [
    "source_file",
    "instruction_category",
    "origin_node",
    "origin_edge",
    "instruction",
    "opcode",
    "instruction_extent",
    "source_span",
    "function",
    "attribution_gap",
];

/// Index of `value` in `table`, adding it when new.
fn intern(table: &mut Vec<String>, index: &mut HashMap<String, u32>, value: &str) -> u32 {
    if let Some(&i) = index.get(value) {
        return i;
    }
    let i = table.len() as u32;
    table.push(value.to_string());
    index.insert(value.to_string(), i);
    i
}

impl StageGraph {
    fn id(&mut self, key: String) -> u32 {
        if let Some(&id) = self.index.get(&key) {
            return id;
        }
        let id = self.keys.len() as u32;
        let stage = key_parts(&key).map_or(Stage::Other, |(k, _, _)| Stage::of_kind(k));
        self.index.insert(key.clone(), id);
        self.keys.push(key);
        self.stage.push(stage);
        self.out.push(Vec::new());
        self.into.push(Vec::new());
        id
    }

    /// Read the stage view of a trace bundle.
    pub fn read(trace: impl BufRead) -> Result<Self, LedgerError> {
        let mut g = Self::default();
        let mut files: BTreeMap<String, String> = BTreeMap::new();
        let mut spans: Vec<(u32, String, u32, u32)> = Vec::new();
        scan_trace(
            trace,
            true,
            |kind| STAGE_FACTS.contains(&kind),
            |line, kind, text| -> Result<(), LedgerError> {
                let v: serde_json::Value =
                    serde_json::from_str(text).map_err(|e| LedgerError::Trace {
                        line,
                        message: e.to_string(),
                    })?;
                if kind == "source_file" {
                    let file = wire(&v, "file_key", line)?;
                    let uri = v.get("uri").and_then(|u| u.as_str()).unwrap_or_default();
                    files.insert(file, uri.to_string());
                    return Ok(());
                }
                match kind {
                    "origin_node" => {
                        let key = wire(&v, "key", line)?;
                        g.id(key);
                    }
                    "origin_edge" => {
                        let from = g.id(wire(&v, "from", line)?);
                        let to = g.id(wire(&v, "to", line)?);
                        let label = v.get("label").and_then(|l| l.as_str()).unwrap_or("");
                        let label = intern(&mut g.labels, &mut g.label_index, label);
                        let phase = v
                            .get("introduced_by")
                            .and_then(|p| p.as_str())
                            .map(|p| intern(&mut g.phases, &mut g.phase_index, p));
                        g.out[from as usize].push(OriginLink {
                            target: to,
                            label,
                            phase,
                        });
                        g.into[to as usize].push(from);
                    }
                    "instruction" => {
                        let inst = g.id(wire(&v, "instruction", line)?);
                        if let Some(m) = v.get("mnemonic").and_then(|m| m.as_str()) {
                            g.mnemonic.insert(inst, m.to_string());
                        }
                        if v.get("function").is_some_and(|f| !f.is_null()) {
                            g.function_of.insert(inst, wire(&v, "function", line)?);
                        }
                    }
                    "opcode" => {
                        let pc = g.id(wire(&v, "pc", line)?);
                        if let Some(m) = v.get("opcode").and_then(|m| m.as_str()) {
                            g.mnemonic.insert(pc, m.to_string());
                        }
                        if let Some(i) = v.get("immediate").and_then(|i| i.as_str()) {
                            g.immediate.insert(pc, i.to_string());
                        }
                    }
                    "instruction_extent" => {
                        let pc = g.id(wire(&v, "instruction", line)?);
                        let range = v.get("pc_range");
                        let at = |n: &str| {
                            range
                                .and_then(|r| r.get(n))
                                .and_then(|x| x.as_u64())
                                .map(|x| x as u32)
                        };
                        if let (Some(start), Some(end)) = (at("start"), at("end")) {
                            g.extent.insert(pc, (start, end));
                        }
                    }
                    "source_span" => {
                        let origin = g.id(wire(&v, "origin", line)?);
                        let file = wire(&v, "file", line)?;
                        let n = |k: &str| v.get(k).and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                        spans.push((origin, file, n("start_line"), n("end_line")));
                    }
                    "function" => {
                        let function = wire(&v, "function", line)?;
                        if let Some(name) = v.get("name").and_then(|n| n.as_str()) {
                            g.function_names.insert(function, name.to_string());
                        }
                    }
                    "instruction_category" => {
                        let inst = g.id(wire(&v, "instruction", line)?);
                        if let Some(c) = v.get("category").and_then(|c| c.as_str()) {
                            g.category.insert(inst, c.to_string());
                        }
                    }
                    "attribution_gap" => {
                        let inst = g.id(wire(&v, "instruction", line)?);
                        if let Some(reason) = v.get("reason").and_then(|r| r.as_str()) {
                            g.gap.insert(inst, reason.to_string());
                        }
                    }
                    _ => {}
                }
                Ok(())
            },
        )?;
        for (origin, file, start_line, end_line) in spans {
            let file = files
                .get(&file)
                .cloned()
                .unwrap_or_else(|| key_parts(&file).map_or(file.clone(), |(_, o, _)| o.into()));
            g.span.insert(
                origin,
                SpanRef {
                    file,
                    start_line,
                    end_line,
                },
            );
        }
        Ok(g)
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn node(&self, key: &str) -> Option<u32> {
        self.index.get(key).copied()
    }

    pub fn key(&self, id: u32) -> &str {
        &self.keys[id as usize]
    }

    pub fn stage(&self, id: u32) -> Stage {
        self.stage[id as usize]
    }

    pub fn links(&self, id: u32) -> &[OriginLink] {
        &self.out[id as usize]
    }

    /// Nodes that were lowered from `id` (the reverse of [`Self::links`]).
    pub fn lowered_into(&self, id: u32) -> &[u32] {
        &self.into[id as usize]
    }

    pub fn mnemonic(&self, id: u32) -> Option<&str> {
        self.mnemonic.get(&id).map(String::as_str)
    }

    pub fn immediate(&self, id: u32) -> Option<&str> {
        self.immediate.get(&id).map(String::as_str)
    }

    pub fn extent(&self, id: u32) -> Option<(u32, u32)> {
        self.extent.get(&id).copied()
    }

    pub fn span(&self, id: u32) -> Option<&SpanRef> {
        self.span.get(&id)
    }

    /// The trace's instruction category (Fe derives it from the opcode for
    /// EVM code: arithmetic, load, store, jump, branch, move, unknown).
    pub fn category(&self, id: u32) -> Option<&str> {
        self.category.get(&id).map(String::as_str)
    }

    pub fn gap(&self, id: u32) -> Option<&str> {
        self.gap.get(&id).map(String::as_str)
    }

    /// Name of the function an instruction belongs to, when the trace says.
    pub fn function_name(&self, id: u32) -> Option<&str> {
        let f = self.function_of.get(&id)?;
        Some(
            self.function_names
                .get(f)
                .map_or(f.as_str(), String::as_str),
        )
    }

    /// A stable operation name for a node: the EVM mnemonic for bytecode,
    /// the first word of a Sonatina instruction, and for MIR statements the
    /// statement form with local ids and interned ids removed
    /// ([`normalize_mir`]). `None` when the trace gives no mnemonic.
    pub fn operation(&self, id: u32) -> Option<String> {
        let m = self.mnemonic(id)?;
        Some(match self.stage(id) {
            Stage::Mir => normalize_mir(m),
            Stage::PreOpt | Stage::PostOpt | Stage::Prepared | Stage::Vcode => {
                m.split_whitespace().next().unwrap_or("").to_string()
            }
            _ => m.to_string(),
        })
    }

    /// Every node reachable from `starts` along origin edges (later to
    /// earlier), `starts` included.
    pub fn trace_back(&self, starts: impl IntoIterator<Item = u32>) -> BTreeSet<u32> {
        self.closure(starts, |id| {
            self.out[id as usize].iter().map(|l| l.target).collect()
        })
    }

    /// Every node lowered (transitively) from `starts`, `starts` included.
    pub fn trace_forward(&self, starts: impl IntoIterator<Item = u32>) -> BTreeSet<u32> {
        self.closure(starts, |id| self.into[id as usize].clone())
    }

    fn closure(
        &self,
        starts: impl IntoIterator<Item = u32>,
        next: impl Fn(u32) -> Vec<u32>,
    ) -> BTreeSet<u32> {
        let mut seen = BTreeSet::new();
        let mut queue: VecDeque<u32> = VecDeque::new();
        for s in starts {
            if seen.insert(s) {
                queue.push_back(s);
            }
        }
        while let Some(id) = queue.pop_front() {
            for n in next(id) {
                if seen.insert(n) {
                    queue.push_back(n);
                }
            }
        }
        seen
    }

    /// The earliest pipeline stage reached from `id` along origin edges:
    /// where the chain of provenance ends.
    pub fn earliest_stage(&self, id: u32) -> Stage {
        self.trace_back([id])
            .into_iter()
            .map(|n| self.stage(n))
            .filter(|s| *s != Stage::Other)
            .min()
            .unwrap_or(Stage::Other)
    }

    /// The traced chain between `root` (an earlier-stage node) and `members`
    /// (nodes lowered from it), as a riff [`Graph`]: every node's kind is its
    /// stage, its Structure field `op` its [`Self::operation`], its Constants
    /// field `immediate` a PUSH value. Children run from the earlier node to
    /// the later ones lowered from it (edges between nodes of one stage, or
    /// to nodes outside the chain, are left out), all at ordinal 0, so a
    /// node's children are a multiset. The graph is acyclic by stage order.
    pub fn chain_graph(&self, root: u32, members: &BTreeSet<u32>) -> Result<Graph, CatalogError> {
        let owner = EntityKey::new("fe.stage.chain", "chain", "root")?;
        let mut graph = Graph::new(GraphKey::new(owner.clone(), "chain")?);
        let mut node_keys: HashMap<u32, NodeKey> = HashMap::new();
        let mut all: BTreeSet<u32> = members.clone();
        all.insert(root);
        for (n, id) in all.iter().enumerate() {
            let key = if *id == root {
                NodeKey::entity(owner.clone())
            } else {
                NodeKey::derived(owner.clone(), n.to_string())?
            };
            graph.add_node(key.clone(), self.stage(*id).as_str())?;
            if let Some(op) = self.operation(*id) {
                graph.add_field(&key, Dimension::Structure, "op", op)?;
            }
            if let Some(imm) = self.immediate(*id) {
                graph.add_field(&key, Dimension::Constants, "immediate", imm)?;
            }
            node_keys.insert(*id, key);
        }
        for id in &all {
            let mut targets: Vec<u32> = self.out[*id as usize]
                .iter()
                .map(|l| l.target)
                .filter(|t| all.contains(t) && self.stage(*t) < self.stage(*id))
                .collect();
            targets.sort_unstable();
            targets.dedup();
            for t in targets {
                graph.add_child(&node_keys[&t], "lowered", 0, &node_keys[id])?;
            }
        }
        Ok(graph)
    }
}

/// Normalize a MIR statement mnemonic to its form: `RLocalId(n)` becomes
/// `_`, interned ids and phantom markers are dropped, and only the leading
/// operation is kept (`_ = call`, `_ = load Ptr Memory`, `store`, ...).
pub fn normalize_mir(mnemonic: &str) -> String {
    let rhs_start = mnemonic.find(" = ").map(|i| i + 3);
    let (lhs, rhs) = match rhs_start {
        Some(i) => ("_ = ", &mnemonic[i..]),
        None => ("", mnemonic),
    };
    let head: String = rhs
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    let op = if head.starts_with("RLocalId") {
        "move".to_string()
    } else {
        head
    };
    let mut detail = String::new();
    if matches!(op.as_str(), "load" | "store" | "addr_of") {
        for root in ["Slot", "Ptr", "Ref"] {
            if rhs.contains(&format!("root: {root}")) {
                detail.push(' ');
                detail.push_str(root);
                break;
            }
        }
        for space in ["Memory", "Storage", "Calldata", "Transient"] {
            if rhs.contains(&format!("space: {space}")) {
                detail.push(' ');
                detail.push_str(space);
                break;
            }
        }
    }
    format!("{lhs}{op}{detail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(kind: &str, local: &str) -> serde_json::Value {
        serde_json::json!({"kind": kind, "owner_key": "o", "local_key": local})
    }

    fn bundle() -> String {
        let mut lines = vec![serde_json::json!({"record": "metadata", "schema_version": 2})];
        let edge = |from, to, phase: &str| {
            serde_json::json!({"record": "fact", "type": "origin_edge", "from": from,
                "to": to, "label": "lowered_from", "introduced_by": phase})
        };
        let hir = key("hir.expr", "1");
        let mir = key("runtime.stmt", "block:0:stmt:0");
        let pre = key("sonatina.preopt.inst", "i0");
        let post = key("sonatina.postopt.inst", "i0");
        let pc0 = key("bytecode.pc", "pc:0");
        let pc2 = key("bytecode.pc", "pc:2");
        lines.push(serde_json::json!({"record": "fact", "type": "origin_node", "key": hir}));
        lines.push(edge(mir.clone(), hir.clone(), "mir"));
        lines.push(edge(pre.clone(), mir.clone(), "sonatina_pre_opt"));
        lines.push(edge(post.clone(), pre.clone(), "sonatina_post_opt"));
        lines.push(edge(pc0.clone(), post.clone(), "backend"));
        lines.push(serde_json::json!({"record": "fact", "type": "instruction", "instruction": mir,
            "function": key("runtime.function", "function"), "index": 0,
            "mnemonic": "RLocalId(5) = load RuntimePlace { root: Ptr { addr: RLocalId(2), space: Memory } }"}));
        lines.push(
            serde_json::json!({"record": "fact", "type": "instruction", "instruction": post,
            "function": key("sonatina.postopt.function", "f"), "index": 0, "mnemonic": "mload v1"}),
        );
        lines.push(
            serde_json::json!({"record": "fact", "type": "opcode", "pc": pc0,
            "opcode": "PUSH1", "immediate": "0x40", "category": "push"}),
        );
        lines.push(
            serde_json::json!({"record": "fact", "type": "instruction_extent",
            "instruction": pc0, "code_object": key("code.object", "runtime"),
            "pc_range": {"start": 0, "end": 2}, "byte_len": 2}),
        );
        lines.push(
            serde_json::json!({"record": "fact", "type": "attribution_gap",
            "instruction": pc2, "reason": "missing_provenance"}),
        );
        lines.push(serde_json::json!({"record": "fact", "type": "function",
            "function": key("sonatina.postopt.function", "f"), "name": "alloc"}));
        lines
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn reads_stages_and_traces_both_ways() {
        let g = StageGraph::read(bundle().as_bytes()).unwrap();
        let pc = g.node(&key_text("bytecode.pc", "o", "pc:0")).unwrap();
        let hir = g.node(&key_text("hir.expr", "o", "1")).unwrap();
        let back = g.trace_back([pc]);
        let stages: Vec<Stage> = back.iter().map(|n| g.stage(*n)).collect();
        assert_eq!(back.len(), 5);
        assert!(stages.contains(&Stage::Hir) && stages.contains(&Stage::PreOpt));
        assert_eq!(g.earliest_stage(pc), Stage::Hir);
        assert_eq!(g.trace_forward([hir]).len(), 5);
        assert_eq!(g.extent(pc), Some((0, 2)));
        assert_eq!(g.immediate(pc), Some("0x40"));
        let pc2 = g.node(&key_text("bytecode.pc", "o", "pc:2")).unwrap();
        assert_eq!(g.gap(pc2), Some("missing_provenance"));
        assert_eq!(g.earliest_stage(pc2), Stage::Bytecode);
        let post = g
            .node(&key_text("sonatina.postopt.inst", "o", "i0"))
            .unwrap();
        assert_eq!(g.operation(post).as_deref(), Some("mload"));
        assert_eq!(g.function_name(post), Some("alloc"));
        let mir = g
            .node(&key_text("runtime.stmt", "o", "block:0:stmt:0"))
            .unwrap();
        assert_eq!(g.operation(mir).as_deref(), Some("_ = load Ptr Memory"));
        let chain = g.chain_graph(hir, &back).unwrap();
        assert_eq!(chain.nodes.len(), 5);
        assert_eq!(chain.children.len(), 4);
        chain.validate().unwrap();
    }

    #[test]
    fn trace_schema_one_and_two_are_read_and_others_refused() {
        let body = bundle();
        let body = body.split_once('\n').unwrap().1;
        for ok in [
            r#"{"record":"metadata","schema_version":1}"#,
            r#"{ "record": "metadata", "schema_version": 2 }"#,
        ] {
            StageGraph::read(format!("{ok}\n{body}").as_bytes()).unwrap();
        }
        for bad in [
            r#"{ "record": "metadata", "schema_version": 99 }"#,
            r#"{"schema_version":99,"record":"metadata"}"#,
            r#"{"record":"metadata"}"#,
            r#"{"record":"metadata","schema_version":"1"}"#,
        ] {
            assert!(
                StageGraph::read(format!("{bad}\n{body}").as_bytes()).is_err(),
                "{bad} accepted"
            );
        }
        assert!(
            StageGraph::read(body.as_bytes()).is_err(),
            "no metadata accepted"
        );
    }

    #[test]
    fn more_than_255_distinct_edge_labels_and_phases_are_read() {
        let mut text = bundle();
        for i in 0..300 {
            text.push_str(&format!(
                "\n{}",
                serde_json::json!({"record": "fact", "type": "origin_edge",
                    "from": key("bytecode.pc", "pc:0"), "to": key("hir.expr", &format!("x{i}")),
                    "label": format!("label{i}"), "introduced_by": format!("phase{i}")})
            ));
        }
        let read = std::panic::catch_unwind(|| StageGraph::read(text.as_bytes()).unwrap());
        let g = read.expect("reading 300 edge labels panicked");
        assert!(g.labels.len() > 300 && g.phases.len() > 300);
    }

    #[test]
    fn mir_mnemonics_normalize_to_their_form() {
        assert_eq!(normalize_mir("RLocalId(5) = RLocalId(2)"), "_ = move");
        assert_eq!(
            normalize_mir(
                "RLocalId(6) = call RuntimeInstance(Id(137c29), PhantomData<x>)([RLocalId(2)])"
            ),
            "_ = call"
        );
        assert_eq!(normalize_mir("return Some(RLocalId(16))"), "return");
    }
}
