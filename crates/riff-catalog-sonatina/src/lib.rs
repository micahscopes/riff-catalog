//! Canonical Sonatina IR graphs for riffcat.
//!
//! The lowering keeps compiler position IDs only as producer keys. Anonymous
//! shape hashes therefore see operation, type, constant, control, data, and
//! call structure without depending on arena numbering. This is intentionally
//! a read-only adapter: Sonatina and Fe do not depend on riffcat.

use std::collections::{BTreeMap, HashMap};

use riff_catalog_core::{
    CatalogError, CyclePolicy, Digest, DigestRequest, Dimension, EdgeRole, EntityKey, Graph,
    GraphKey, HashPolicy, NodeKey, ViewMode, digest_graph,
};
use sonatina_ir::{Immediate, Module, Value, ir_writer::IrWrite, module::FuncRef};
use thiserror::Error;

/// Versioned lowering contract. A change to graph topology or field assignment
/// requires a new level string.
pub const SONATINA_IR_LEVEL: &str = "sonatina-ir/1";

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
    let parsed = sonatina_parser::parse_module(source)
        .map_err(|error| LowerError::Parse(format!("{error:?}")))?;
    lower_module(owner, &parsed.module)
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

/// Count anonymous structural subtree classes inside one module graph. Unlike
/// emitted byte size, this distinguishes novel computation from copied shape.
pub fn structure_census(lowered: &LoweredModule) -> Result<StructureCensus, LowerError> {
    let policy = HashPolicy::new(
        SONATINA_IR_LEVEL,
        ViewMode::AnonymousShape,
        CyclePolicy::CondenseScc,
    )?;
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
}
