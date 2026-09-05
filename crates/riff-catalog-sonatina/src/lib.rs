//! Canonical Sonatina IR graphs for riffcat.
//!
//! The lowering keeps compiler position IDs only as producer keys. Anonymous
//! shape hashes therefore see operation, type, constant, control, data, and
//! call structure without depending on arena numbering. This is intentionally
//! a read-only adapter: Sonatina and Fe do not depend on riffcat.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use riff_catalog_core::{
    CatalogError, CyclePolicy, Digest, DigestRequest, Dimension, EdgeRole, EntityKey, Facet, Graph,
    GraphKey, HashPolicy, NodeKey, ViewMode, digest_graph,
};
use sonatina_ir::{Immediate, Module, Value, ir_writer::IrWrite, module::FuncRef};
use thiserror::Error;

/// Versioned lowering contract. A change to graph topology or field assignment
/// requires a new level string.
pub const SONATINA_IR_LEVEL: &str = "sonatina-ir/1";

/// Versioned projection of Sonatina's explicit memory effects and the address
/// computations that feed them. This keeps the five catalog dimensions stable
/// while giving memory placement its own reusable analysis level.
pub const SONATINA_MEMORY_LEVEL: &str = "sonatina-memory/1";

#[derive(Debug, Error)]
pub enum LowerError {
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    #[error("Sonatina IR text formatting failed")]
    IrText,
    #[error("Sonatina IR parsing failed: {0}")]
    Parse(String),
    #[error("function {0} has no signature")]
    MissingSignature(String),
    #[error("function {0} has no body")]
    MissingBody(String),
    #[error("value {value} in function {function} has no graph node")]
    MissingValueNode { function: String, value: u32 },
    #[error("branch target {block} in function {function} has no graph node")]
    MissingBlockNode { function: String, block: u32 },
    #[error("callee {callee} has no graph node")]
    MissingCalleeNode { callee: String },
}

/// Parse and lower one textual Sonatina module.
pub fn parse_and_lower_module(owner: &str, source: &str) -> Result<LoweredModule, LowerError> {
    Ok(parse_and_lower_views(owner, source)?.0)
}

/// Parse once and derive both the complete IR graph and its memory-effect
/// projection. Large compiler snapshots should not pay for a second parse just
/// to ask a placement question.
pub fn parse_and_lower_views(
    owner: &str,
    source: &str,
) -> Result<(LoweredModule, LoweredModule), LowerError> {
    let parsed = sonatina_parser::parse_module(source)
        .map_err(|error| LowerError::Parse(format!("{error:?}")))?;
    let lowered = lower_module(owner, &parsed.module)?;
    let memory = project_memory_module(owner, &lowered)?;
    Ok((lowered, memory))
}

#[derive(Clone, Debug)]
pub struct LoweredModule {
    pub graph_key: GraphKey,
    pub graph: Graph,
    pub function_count: usize,
    pub block_count: usize,
    pub instruction_count: usize,
    pub call_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShapeClass {
    pub digest: Digest,
    pub occurrences: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructureCensus {
    pub nodes: usize,
    pub distinct_shapes: usize,
    pub repeated_occurrences: usize,
    pub largest_class: usize,
    pub classes: Vec<ShapeClass>,
}

/// A compact, content-addressed observation of one live compiler phase.
///
/// The digest intentionally excludes names and arena identities. It commits to
/// structure, types, and constants, so the same computation observed under a
/// different function name remains comparable while a semantic rewrite moves
/// the digest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleSnapshot {
    pub phase: String,
    pub anonymous_digest: Digest,
    pub function_count: usize,
    pub block_count: usize,
    pub instruction_count: usize,
    pub call_count: usize,
    pub census: StructureCensus,
}

/// Signed changes between adjacent compiler-phase observations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleDelta {
    pub before: String,
    pub after: String,
    pub digest_changed: bool,
    pub functions: i64,
    pub blocks: i64,
    pub instructions: i64,
    pub calls: i64,
    pub nodes: i64,
    pub distinct_shapes: i64,
    pub repeated_occurrences: i64,
    pub largest_class: i64,
}

/// Ordered in-memory observations of one module as it crosses compiler phases.
///
/// This owns only compact measurements, not cloned IR or graphs. Compiler
/// observers can therefore retain a complete timeline without extending the
/// lifetime of any intermediate module.
#[derive(Clone, Debug)]
pub struct ModuleTimeline {
    owner: String,
    snapshots: Vec<ModuleSnapshot>,
}

impl ModuleTimeline {
    pub fn new(owner: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            snapshots: Vec::new(),
        }
    }

    pub fn observe(
        &mut self,
        phase: impl Into<String>,
        module: &Module,
    ) -> Result<&ModuleSnapshot, LowerError> {
        self.snapshots
            .push(snapshot_module(&self.owner, phase, module)?);
        Ok(self
            .snapshots
            .last()
            .expect("a snapshot was appended immediately above"))
    }

    pub fn snapshots(&self) -> &[ModuleSnapshot] {
        &self.snapshots
    }

    pub fn deltas(&self) -> impl Iterator<Item = ModuleDelta> + '_ {
        self.snapshots
            .windows(2)
            .map(|pair| ModuleDelta::between(&pair[0], &pair[1]))
    }
}

impl ModuleDelta {
    fn between(before: &ModuleSnapshot, after: &ModuleSnapshot) -> Self {
        fn delta(before: usize, after: usize) -> i64 {
            if after >= before {
                i64::try_from(after - before).unwrap_or(i64::MAX)
            } else {
                -i64::try_from(before - after).unwrap_or(i64::MAX)
            }
        }

        Self {
            before: before.phase.clone(),
            after: after.phase.clone(),
            digest_changed: before.anonymous_digest != after.anonymous_digest,
            functions: delta(before.function_count, after.function_count),
            blocks: delta(before.block_count, after.block_count),
            instructions: delta(before.instruction_count, after.instruction_count),
            calls: delta(before.call_count, after.call_count),
            nodes: delta(before.census.nodes, after.census.nodes),
            distinct_shapes: delta(before.census.distinct_shapes, after.census.distinct_shapes),
            repeated_occurrences: delta(
                before.census.repeated_occurrences,
                after.census.repeated_occurrences,
            ),
            largest_class: delta(before.census.largest_class, after.census.largest_class),
        }
    }
}

/// Observe a live Sonatina module without serializing it through textual IR.
pub fn snapshot_module(
    owner: &str,
    phase: impl Into<String>,
    module: &Module,
) -> Result<ModuleSnapshot, LowerError> {
    let lowered = lower_module(owner, module)?;
    let census = structure_census(&lowered)?;
    let dimensions = [Dimension::Structure, Dimension::Types, Dimension::Constants];
    let policy = HashPolicy::new(
        SONATINA_IR_LEVEL,
        ViewMode::AnonymousShape,
        CyclePolicy::CondenseScc,
    )?;
    let request = DigestRequest::new(lowered.graph_key.clone(), policy, dimensions)?;
    let facet = Facet::new(request.policy_id(), dimensions)?;
    let result = digest_graph(&request, &lowered.graph)?;
    let anonymous_digest = result.hashes.facet_address(&facet)?.address_digest();

    Ok(ModuleSnapshot {
        phase: phase.into(),
        anonymous_digest,
        function_count: lowered.function_count,
        block_count: lowered.block_count,
        instruction_count: lowered.instruction_count,
        call_count: lowered.call_count,
        census,
    })
}

/// Count anonymous structural subtree classes inside one module graph. Unlike
/// emitted byte size, this distinguishes novel computation from copied shape.
pub fn structure_census(lowered: &LoweredModule) -> Result<StructureCensus, LowerError> {
    structure_census_at_level(lowered, SONATINA_IR_LEVEL)
}

/// Count anonymous structural classes inside a memory-effect projection.
pub fn memory_structure_census(lowered: &LoweredModule) -> Result<StructureCensus, LowerError> {
    structure_census_at_level(lowered, SONATINA_MEMORY_LEVEL)
}

fn structure_census_at_level(
    lowered: &LoweredModule,
    level: &str,
) -> Result<StructureCensus, LowerError> {
    let policy = HashPolicy::new(level, ViewMode::AnonymousShape, CyclePolicy::CondenseScc)?;
    let result = digest_graph(
        &DigestRequest::new(lowered.graph_key.clone(), policy, [Dimension::Structure])?,
        &lowered.graph,
    )?;
    let mut counts = BTreeMap::<Digest, usize>::new();
    for hashes in result.hashes.nodes.values() {
        let digest = *hashes
            .tree
            .get(Dimension::Structure)
            .expect("requested structure digest must be present");
        *counts.entry(digest).or_default() += 1;
    }
    let mut classes = counts
        .into_iter()
        .map(|(digest, occurrences)| ShapeClass {
            digest,
            occurrences,
        })
        .collect::<Vec<_>>();
    classes.sort_by(|left, right| {
        right
            .occurrences
            .cmp(&left.occurrences)
            .then_with(|| left.digest.cmp(&right.digest))
    });
    let nodes = result.hashes.nodes.len();
    let distinct_shapes = classes.len();
    Ok(StructureCensus {
        nodes,
        distinct_shapes,
        repeated_occurrences: nodes.saturating_sub(distinct_shapes),
        largest_class: classes.first().map_or(0, |class| class.occurrences),
        classes,
    })
}

fn operation(node: &riff_catalog_core::Node) -> Option<&str> {
    node.fields.iter().find_map(|field| {
        if field.dimension == Dimension::Structure && field.name.as_str() == "operation" {
            match &field.value {
                riff_catalog_core::Value::Text(value) => Some(value.as_str()),
                _ => None,
            }
        } else {
            None
        }
    })
}

fn is_memory_operation(operation: &str) -> bool {
    operation.starts_with("mem.")
        || operation.starts_with("obj.")
        || matches!(
            operation,
            "mload" | "mstore" | "memcopy" | "memzero" | "alloca"
        )
}

fn address_operand(operation: &str, operand: usize) -> bool {
    match operation {
        "mem.checkpoint" => false,
        "mem.alloc_dynamic" | "mem.rewind" | "mload" | "mstore" | "alloca" => operand == 0,
        "memcopy" => operand <= 2,
        "memzero" => operand <= 1,
        operation if operation.starts_with("obj.") => operand == 0,
        _ => true,
    }
}

fn operand_index(label: &str) -> Option<usize> {
    label.strip_prefix("operand:")?.parse().ok()
}

/// Project a full Sonatina graph down to explicit memory effects, their
/// address-producing backward slices, and the containing function/block
/// topology. Stored values are deliberately not pulled into an `mstore`
/// address slice, otherwise ordinary arithmetic would swamp the placement
/// question this view is meant to answer.
pub fn project_memory_module(
    owner: &str,
    lowered: &LoweredModule,
) -> Result<LoweredModule, LowerError> {
    let source = &lowered.graph;
    let memory_owner = EntityKey::new("sonatina.memory", owner, "module")?;
    let graph_key = GraphKey::new(memory_owner.clone(), "memory")?;
    let root = NodeKey::entity(memory_owner);
    let source_root = NodeKey::entity(source.graph_key.owner.clone());

    let mut selected = BTreeSet::new();
    let mut memory_nodes = Vec::new();
    for (key, node) in &source.nodes {
        if operation(node).is_some_and(is_memory_operation) {
            selected.insert(key.clone());
            memory_nodes.push(key.clone());
        }
    }

    let incoming_data = source
        .edges
        .iter()
        .filter(|edge| edge.role == EdgeRole::Data)
        .fold(HashMap::<NodeKey, Vec<_>>::new(), |mut by_target, edge| {
            by_target.entry(edge.target.clone()).or_default().push(edge);
            by_target
        });
    let mut address_work = VecDeque::new();
    for key in &memory_nodes {
        let operation = operation(
            source
                .nodes
                .get(key)
                .expect("a selected memory node must remain present"),
        )
        .expect("a selected memory node must carry an operation");
        for edge in incoming_data.get(key).into_iter().flatten() {
            if operand_index(edge.label.as_str())
                .is_some_and(|operand| address_operand(operation, operand))
            {
                address_work.push_back(edge.source.clone());
            }
        }
    }
    while let Some(key) = address_work.pop_front() {
        if !selected.insert(key.clone()) {
            continue;
        }
        if operation(
            source
                .nodes
                .get(&key)
                .expect("a data-edge source must remain present"),
        )
        .is_some_and(is_memory_operation)
        {
            continue;
        }
        for edge in incoming_data.get(&key).into_iter().flatten() {
            address_work.push_back(edge.source.clone());
        }
    }

    // Retain the block/function ancestry for every selected instruction or
    // value. This provides scope and ordering without pulling the complete IR
    // back into the memory projection.
    loop {
        let mut changed = false;
        for child in &source.children {
            if selected.contains(&child.child) && child.parent != source_root {
                changed |= selected.insert(child.parent.clone());
            }
        }
        if !changed {
            break;
        }
    }

    // Keep branch terminators between retained blocks so the projection still
    // distinguishes straight-line, branched, and looping memory lifetimes.
    let parent_of = source
        .children
        .iter()
        .fold(HashMap::new(), |mut parents, child| {
            parents.insert(child.child.clone(), child.parent.clone());
            parents
        });
    for edge in source
        .edges
        .iter()
        .filter(|edge| edge.role == EdgeRole::Control)
    {
        if selected.contains(&edge.target)
            && parent_of
                .get(&edge.source)
                .is_some_and(|block| selected.contains(block))
        {
            selected.insert(edge.source.clone());
        }
    }

    let mut graph = Graph::new(graph_key.clone());
    graph.add_node(root.clone(), "sonatina.memory")?;
    for key in &selected {
        let node = source
            .nodes
            .get(key)
            .expect("a projected key must remain present");
        graph.nodes.insert(key.clone(), node.clone());
    }
    for child in &source.children {
        if child.parent == source_root && selected.contains(&child.child) {
            graph.add_child(&root, child.label.as_str(), child.ordinal, &child.child)?;
        } else if selected.contains(&child.parent) && selected.contains(&child.child) {
            graph.children.push(child.clone());
        }
    }
    graph.edges.extend(
        source
            .edges
            .iter()
            .filter(|edge| selected.contains(&edge.source) && selected.contains(&edge.target))
            .cloned(),
    );
    graph.validate()?;

    let function_count = graph
        .nodes
        .values()
        .filter(|node| node.kind.as_str() == "sonatina.function")
        .count();
    let block_count = graph
        .nodes
        .values()
        .filter(|node| node.kind.as_str() == "sonatina.block")
        .count();
    let instruction_count = graph
        .nodes
        .values()
        .filter(|node| node.kind.as_str() == "sonatina.instruction")
        .count();
    let call_count = graph
        .edges
        .iter()
        .filter(|edge| edge.role == EdgeRole::Call)
        .count();

    Ok(LoweredModule {
        graph_key,
        graph,
        function_count,
        block_count,
        instruction_count,
        call_count,
    })
}

/// Lower one Sonatina module into a canonical riff graph.
pub fn lower_module(owner: &str, module: &Module) -> Result<LoweredModule, LowerError> {
    let module_entity = EntityKey::new("sonatina.module", owner, "module")?;
    let graph_key = GraphKey::new(module_entity.clone(), "module")?;
    let root = NodeKey::entity(module_entity);
    let mut graph = Graph::new(graph_key.clone());
    graph.add_node(root.clone(), "sonatina.module")?;
    graph.add_field(
        &root,
        Dimension::Types,
        "target",
        module.ctx.triple.to_string(),
    )?;

    let functions = module.funcs();
    let mut function_nodes = HashMap::<FuncRef, NodeKey>::new();
    for (ordinal, function) in functions.iter().copied().enumerate() {
        let local = format!("function:{function:?}");
        let node = NodeKey::entity(EntityKey::new("sonatina.function", owner, local)?);
        graph.add_node(node.clone(), "sonatina.function")?;
        let signature = module
            .ctx
            .get_sig(function)
            .ok_or_else(|| LowerError::MissingSignature(format!("{function:?}")))?;
        graph.add_field(
            &node,
            Dimension::Names,
            "name",
            signature.name().to_string(),
        )?;
        graph.add_field(
            &node,
            Dimension::Structure,
            "linkage",
            format!("{:?}", signature.linkage()),
        )?;
        for (index, ty) in signature.args().iter().copied().enumerate() {
            graph.add_field(
                &node,
                Dimension::Types,
                format!("argument:{index}"),
                ir_text(ty, module)?,
            )?;
        }
        for (index, ty) in signature.ret_tys().iter().copied().enumerate() {
            graph.add_field(
                &node,
                Dimension::Types,
                format!("result:{index}"),
                ir_text(ty, module)?,
            )?;
        }
        graph.add_child(&root, "function", ordinal as u32, &node)?;
        function_nodes.insert(function, node);
    }

    let mut block_count = 0;
    let mut instruction_count = 0;
    let mut call_count = 0;
    for function in functions.iter().copied() {
        let function_name = format!("{function:?}");
        let function_node = function_nodes
            .get(&function)
            .expect("function node was predeclared")
            .clone();
        let result = module.func_store.try_view(function, |body| {
            let mut block_nodes = HashMap::new();
            let mut instruction_nodes = HashMap::new();
            let mut value_nodes = HashMap::new();

            for (ordinal, block) in body.layout.iter_block().enumerate() {
                let node =
                    NodeKey::derived(function_node.owner().clone(), format!("block:{}", block.0))?;
                graph.add_node(node.clone(), "sonatina.block")?;
                graph.add_child(&function_node, "block", ordinal as u32, &node)?;
                block_nodes.insert(block, node);
                block_count += 1;
            }

            for (ordinal, value) in body.dfg.value_ids().enumerate() {
                let data = body.dfg.value(value);
                if matches!(data, Value::Inst { .. }) {
                    continue;
                }
                let node =
                    NodeKey::derived(function_node.owner().clone(), format!("value:{}", value.0))?;
                let kind = match data {
                    Value::Arg { .. } => "sonatina.argument",
                    Value::Immediate { .. } => "sonatina.immediate",
                    Value::Global { .. } => "sonatina.global",
                    Value::Undef { .. } => "sonatina.undef",
                    Value::Inst { .. } => unreachable!(),
                };
                graph.add_node(node.clone(), kind)?;
                graph.add_field(
                    &node,
                    Dimension::Types,
                    "type",
                    ir_text(body.dfg.value_ty(value), module)?,
                )?;
                match data {
                    Value::Arg { idx, .. } => graph.add_field(
                        &node,
                        Dimension::Structure,
                        "argument_index",
                        *idx as u64,
                    )?,
                    Value::Immediate { imm, .. } => graph.add_field(
                        &node,
                        Dimension::Constants,
                        "value",
                        immediate_text(*imm, module)?,
                    )?,
                    Value::Global { gv, .. } => {
                        graph.add_field(&node, Dimension::Names, "global", format!("{gv:?}"))?
                    }
                    Value::Undef { .. } => {}
                    Value::Inst { .. } => unreachable!(),
                }
                graph.add_child(&function_node, "value", ordinal as u32, &node)?;
                value_nodes.insert(value, node);
            }

            for block in body.layout.iter_block() {
                let block_node = block_nodes
                    .get(&block)
                    .expect("block node was predeclared")
                    .clone();
                for (ordinal, instruction) in body.layout.iter_inst(block).enumerate() {
                    let node = NodeKey::derived(
                        function_node.owner().clone(),
                        format!("instruction:{}", instruction.0),
                    )?;
                    let data = body.dfg.inst(instruction);
                    graph.add_node(node.clone(), "sonatina.instruction")?;
                    graph.add_field(&node, Dimension::Structure, "operation", data.as_text())?;
                    for (index, result) in body.dfg.inst_results(instruction).iter().enumerate() {
                        graph.add_field(
                            &node,
                            Dimension::Types,
                            format!("result:{index}"),
                            ir_text(body.dfg.value_ty(*result), module)?,
                        )?;
                    }
                    graph.add_child(&block_node, "instruction", ordinal as u32, &node)?;
                    instruction_nodes.insert(instruction, node);
                    instruction_count += 1;
                }
            }

            for block in body.layout.iter_block() {
                for instruction in body.layout.iter_inst(block) {
                    let instruction_node = instruction_nodes
                        .get(&instruction)
                        .expect("instruction node was predeclared")
                        .clone();
                    let data = body.dfg.inst(instruction);
                    for (operand_index, value) in data.collect_values().into_iter().enumerate() {
                        let source = match body.dfg.value(value) {
                            Value::Inst { inst, .. } => instruction_nodes.get(inst),
                            _ => value_nodes.get(&value),
                        }
                        .ok_or(LowerError::MissingValueNode {
                            function: function_name.clone(),
                            value: value.0,
                        })?;
                        graph.add_edge(
                            source,
                            format!("operand:{operand_index}"),
                            &instruction_node,
                            EdgeRole::Data,
                        )?;
                    }
                    if let Some(branch) = body.dfg.branch_info(instruction) {
                        for (edge_index, destination) in branch.dests().into_iter().enumerate() {
                            let target = block_nodes.get(&destination).ok_or(
                                LowerError::MissingBlockNode {
                                    function: function_name.clone(),
                                    block: destination.0,
                                },
                            )?;
                            graph.add_edge(
                                &instruction_node,
                                format!("successor:{edge_index}"),
                                target,
                                EdgeRole::Control,
                            )?;
                        }
                    }
                    if let Some(call) = body.dfg.call_info(instruction) {
                        let callee = call.callee();
                        let target = function_nodes.get(&callee).ok_or_else(|| {
                            LowerError::MissingCalleeNode {
                                callee: format!("{callee:?}"),
                            }
                        })?;
                        graph.add_edge(&instruction_node, "callee", target, EdgeRole::Call)?;
                        call_count += 1;
                    }
                }
            }
            Result::<(), LowerError>::Ok(())
        });
        result.ok_or_else(|| LowerError::MissingBody(function_name))??;
    }

    Ok(LoweredModule {
        graph_key,
        graph,
        function_count: functions.len(),
        block_count,
        instruction_count,
        call_count,
    })
}

fn ir_text<T>(value: T, module: &Module) -> Result<String, LowerError>
where
    T: IrWrite<sonatina_ir::module::ModuleCtx>,
{
    let mut bytes = Vec::new();
    value
        .write(&mut bytes, &module.ctx)
        .map_err(|_| LowerError::IrText)?;
    String::from_utf8(bytes).map_err(|_| LowerError::IrText)
}

fn immediate_text(value: Immediate, module: &Module) -> Result<String, LowerError> {
    ir_text(value, module)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODULE: &str = r#"
target = "wasm32-unknown-native"

func private %mix(v0.i32) -> i32 {
    block0:
        v1.i32 = mul v0 7.i32;
        v2.i32 = add v1 3.i32;
        return v2;
}

func public %entry(v0.i32) -> i32 {
    block0:
        v1.i32 = call %mix v0;
        v2.i32 = call %mix v1;
        return v2;
}
"#;

    const MEMORY_MODULE: &str = r#"
target = "wasm32-unknown-native"

func public %memory(v0.i32) -> i32 {
    block0:
        v1.*i8 = mem.checkpoint;
        v2.i32 = mem.alloc_dynamic 16.i32;
        v3.i32 = add v2 7.i32;
        v4.i32 = and v3 -8.i32;
        v5.i32 = mul v0 9.i32;
        mstore v4 v5 i32;
        v6.i32 = mload v4 i32;
        mem.rewind v1;
        return v6;
}
"#;

    #[test]
    fn lowers_typed_data_control_and_call_structure() {
        let lowered = parse_and_lower_module("fixture", MODULE).expect("module should lower");
        assert_eq!(lowered.function_count, 2);
        assert_eq!(lowered.block_count, 2);
        assert_eq!(lowered.instruction_count, 6);
        assert_eq!(lowered.call_count, 2);
        assert!(
            lowered
                .graph
                .edges
                .iter()
                .any(|edge| edge.role == EdgeRole::Call)
        );
        assert!(
            lowered
                .graph
                .edges
                .iter()
                .any(|edge| edge.role == EdgeRole::Data)
        );
        let census = structure_census(&lowered).expect("census should hash");
        assert_eq!(census.nodes, lowered.graph.nodes.len());
        assert!(census.distinct_shapes < census.nodes);
        assert!(census.largest_class >= 2);
    }

    #[test]
    fn anonymous_structure_ignores_function_names_and_arena_keys() {
        let renamed = MODULE.replace("%mix", "%blend").replace("%entry", "%start");
        let left = parse_and_lower_module("left", MODULE).expect("left should lower");
        let right = parse_and_lower_module("right", &renamed).expect("right should lower");
        let policy = HashPolicy::new(
            SONATINA_IR_LEVEL,
            ViewMode::AnonymousShape,
            CyclePolicy::CondenseScc,
        )
        .unwrap();
        let digest = |lowered: &LoweredModule| {
            digest_graph(
                &DigestRequest::new(
                    lowered.graph_key.clone(),
                    policy.clone(),
                    [Dimension::Structure, Dimension::Types, Dimension::Constants],
                )
                .unwrap(),
                &lowered.graph,
            )
            .unwrap()
            .hashes
            .graph
            .values
        };
        assert_eq!(digest(&left), digest(&right));
    }

    #[test]
    fn memory_projection_keeps_lifetimes_and_addresses_without_stored_arithmetic() {
        let (_, memory) =
            parse_and_lower_views("memory-fixture", MEMORY_MODULE).expect("module should lower");
        let operations = memory
            .graph
            .nodes
            .values()
            .filter_map(operation)
            .collect::<BTreeSet<_>>();
        for expected in [
            "mem.checkpoint",
            "mem.alloc_dynamic",
            "add",
            "and",
            "mstore",
            "mload",
            "mem.rewind",
        ] {
            assert!(
                operations.contains(expected),
                "missing `{expected}`: {operations:?}"
            );
        }
        assert!(
            !operations.contains("mul"),
            "the stored value's arithmetic must not swamp the address projection",
        );
        assert_eq!(memory.function_count, 1);
        assert_eq!(memory.block_count, 1);
        assert!(memory_structure_census(&memory).is_ok());
    }

    #[test]
    fn memory_facets_separate_topology_from_allocation_size() {
        let (_, left) =
            parse_and_lower_views("left-memory", MEMORY_MODULE).expect("left should lower");
        let (_, right) = parse_and_lower_views(
            "right-memory",
            &MEMORY_MODULE.replace("mem.alloc_dynamic 16.i32", "mem.alloc_dynamic 32.i32"),
        )
        .expect("right should lower");
        let digest = |lowered: &LoweredModule, dimensions: &[Dimension]| {
            let policy = HashPolicy::new(
                SONATINA_MEMORY_LEVEL,
                ViewMode::AnonymousShape,
                CyclePolicy::CondenseScc,
            )
            .unwrap();
            digest_graph(
                &DigestRequest::new(
                    lowered.graph_key.clone(),
                    policy,
                    dimensions.iter().copied(),
                )
                .unwrap(),
                &lowered.graph,
            )
            .unwrap()
            .hashes
            .graph
            .values
        };
        assert_eq!(
            digest(&left, &[Dimension::Structure]),
            digest(&right, &[Dimension::Structure]),
        );
        assert_ne!(
            digest(&left, &[Dimension::Structure, Dimension::Constants]),
            digest(&right, &[Dimension::Structure, Dimension::Constants]),
        );
    }

    #[test]
    fn records_live_phase_deltas_without_serializing_ir() {
        let parsed = sonatina_parser::parse_module(MODULE).expect("module should parse");
        let mut timeline = ModuleTimeline::new("pipeline-fixture");
        let initial = timeline
            .observe("lowered", &parsed.module)
            .expect("initial phase should lower")
            .clone();

        let changed_source = MODULE
            .replacen(
                "v2.i32 = add v1 3.i32;",
                "v2.i32 = add v1 3.i32;\n        v3.i32 = add v2 5.i32;",
                1,
            )
            .replacen("return v2;", "return v3;", 1);
        let changed =
            sonatina_parser::parse_module(&changed_source).expect("changed module should parse");
        let final_snapshot = timeline
            .observe("optimized", &changed.module)
            .expect("changed phase should lower")
            .clone();

        assert_ne!(initial.anonymous_digest, final_snapshot.anonymous_digest);
        let deltas = timeline.deltas().collect::<Vec<_>>();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].before, "lowered");
        assert_eq!(deltas[0].after, "optimized");
        assert!(deltas[0].digest_changed);
        assert_eq!(deltas[0].instructions, 1);
        assert!(deltas[0].nodes > 0);
    }
}
