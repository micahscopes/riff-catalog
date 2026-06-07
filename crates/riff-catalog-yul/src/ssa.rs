//! Level "yul-ssa-cfg/1": lower solc's experimental `yulCFGJson` (the SSA
//! CFG from the new code-generation pipeline) into riff-catalog graphs.
//!
//! Why this level is special:
//! - SSA value numbers (`v0, v1…`) erase variable names BY CONSTRUCTION —
//!   the names-blind facet is the native representation.
//! - Jump targets are explicit block edges → loops are real back-edges →
//!   SCCs → the WL machinery actually bites (invariant I4).
//! - It works for BOTH Solidity-viaIR and direct `.yul` input (verified),
//!   which `--ir-ast-json` does not.
//!
//! Lowering rules (invariant I15: `vN` strings NEVER enter payloads —
//! def-use is positional structure):
//! - block → `yulssa.block` node; jump targets → Dependency edges labeled
//!   `jump:<k>` (target order is semantics for ConditionalJump).
//! - instruction → ordered `yulssa.insn` child; builtin/PhiFunction/
//!   LiteralAssignment ops → Structure `op`; user calls → Names `callee` +
//!   Dependency `calls` edge to the function node when in-graph.
//! - value operand → `yulssa.use` child (label `in`, ordinal k) + Data edge
//!   to the DEFINING node (instruction or argument).
//! - literal operand / literalArg → `yulssa.lit` child, Constants `value`.
//! - phi inputs pair with predecessor blocks as labeled pairs, NOT raw
//!   order: `phi-in` children at ordinal 0 (multiset), each with a Data edge
//!   to its def and a Control edge `from` to its predecessor block.
//! - block sets are multisets (label `block`, ordinal 0) — JSON array order
//!   is emission order, not semantics; the CFG lives in the jump edges. The
//!   entry block is singled out via the `entry` child label.
//!
//! Schema caveat, on purpose: yulCFGJson is experimental and actively
//! changing (solidity PRs #16646–#16767). The `/1` level string fences this
//! lowering; when the schema shifts, bump to `yul-ssa-cfg/2` and old digests
//! remain valid strangers (invariant I8).

use std::collections::BTreeMap;

use riff_catalog_core::{Dimension, EdgeRole, EntityKey, Graph, GraphKey, NodeKey};
use serde_json::Value;

use crate::builtins::is_evm_builtin;
use crate::canon::canon_number;
use crate::error::YulLowerError;
use crate::lower::LoweredUnit;

/// The versioned level string for this lowering (invariant I10).
pub const YUL_SSA_LEVEL: &str = "yul-ssa-cfg/1";

#[derive(Clone, Debug)]
pub struct LoweredSsa {
    pub objects: Vec<LoweredUnit>,
    pub functions: Vec<LoweredUnit>,
}

/// Lower a `yulCFGJson` value (the per-contract output of riff-catalog-solc's
/// `yul_cfg_json`).
pub fn lower_yul_cfg(cfg: &Value, owner: &str) -> Result<LoweredSsa, YulLowerError> {
    let map = cfg
        .as_object()
        .ok_or_else(|| shape("top level is not an object"))?;

    let mut out = LoweredSsa {
        objects: Vec::new(),
        functions: Vec::new(),
    };
    // Top level: { "<ObjectName>": {blocks, functions, subObjects}, "type": "Object" }
    for (name, value) in map {
        if value.is_object() && value.get("blocks").is_some() {
            lower_object_cfg(value, name, owner, "s", &mut out)?;
        }
    }
    if out.objects.is_empty() {
        return Err(shape("no object with blocks found"));
    }
    Ok(out)
}

fn shape(message: impl Into<String>) -> YulLowerError {
    YulLowerError::SsaShape(message.into())
}

fn lower_object_cfg(
    object: &Value,
    name: &str,
    owner: &str,
    path: &str,
    out: &mut LoweredSsa,
) -> Result<(), YulLowerError> {
    let graph_key = GraphKey::new(
        EntityKey::new("yulssa.object", owner, path).map_err(YulLowerError::Core)?,
        "yulssa-object",
    )
    .map_err(YulLowerError::Core)?;
    let mut graph = Graph::new(graph_key.clone());

    let object_node =
        NodeKey::entity(EntityKey::new("yulssa.object", owner, path).map_err(YulLowerError::Core)?);
    graph
        .add_node(object_node.clone(), "yulssa.object")
        .map_err(YulLowerError::Core)?;
    graph
        .add_field(&object_node, Dimension::Names, "name", name)
        .map_err(YulLowerError::Core)?;

    // Function nodes pre-registered so call instructions can edge to them.
    let functions = object
        .get("functions")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut fn_keys: BTreeMap<String, NodeKey> = BTreeMap::new();
    for fn_name in functions.keys() {
        let fn_path = format!("{path}/fn:{fn_name}");
        let key = NodeKey::entity(
            EntityKey::new("yulssa.fn", owner, fn_path.as_str()).map_err(YulLowerError::Core)?,
        );
        graph
            .add_node(key.clone(), "yulssa.fn")
            .map_err(YulLowerError::Core)?;
        graph
            .add_field(&key, Dimension::Names, "name", fn_name.as_str())
            .map_err(YulLowerError::Core)?;
        graph
            .add_child(&object_node, "fn", 0, &key)
            .map_err(YulLowerError::Core)?;
        fn_keys.insert(fn_name.clone(), key);
    }

    // Object-level (dispatch) blocks: first block is the entry.
    if let Some(blocks) = object.get("blocks").and_then(Value::as_array) {
        let entry = blocks
            .first()
            .and_then(|block| block.get("id"))
            .and_then(Value::as_str);
        lower_blocks(
            &mut graph,
            owner,
            &object_node,
            blocks,
            entry,
            &format!("{path}/code"),
            &fn_keys,
            &BTreeMap::new(),
        )?;
    }

    // Functions: full bodies inside the object graph...
    for (fn_name, function) in &functions {
        let fn_path = format!("{path}/fn:{fn_name}");
        lower_function_body(&mut graph, owner, function, &fn_keys, &fn_path)?;
    }

    // sub-objects (deployed object lives here), recursed as separate graphs;
    // the parent graph records the containment as a multiset child.
    if let Some(subs) = object.get("subObjects").and_then(Value::as_object) {
        for (sub_index, (sub_name, sub)) in subs
            .iter()
            .filter(|(_, value)| value.is_object() && value.get("blocks").is_some())
            .enumerate()
        {
            let sub_path = format!("{path}.{sub_index}");
            lower_object_cfg(sub, sub_name, owner, &sub_path, out)?;
        }
    }

    // ...and one standalone graph per function (calls to siblings stay
    // Names-only there, same v1 limitation as the yul-ast level).
    for (fn_name, function) in &functions {
        let fn_path = format!("{path}/fn:{fn_name}");
        let fn_graph_key = GraphKey::new(
            EntityKey::new("yulssa.fn", owner, fn_path.as_str()).map_err(YulLowerError::Core)?,
            "yulssa-fn",
        )
        .map_err(YulLowerError::Core)?;
        let mut fn_graph = Graph::new(fn_graph_key.clone());
        let self_key = NodeKey::entity(
            EntityKey::new("yulssa.fn", owner, fn_path.as_str()).map_err(YulLowerError::Core)?,
        );
        fn_graph
            .add_node(self_key.clone(), "yulssa.fn")
            .map_err(YulLowerError::Core)?;
        fn_graph
            .add_field(&self_key, Dimension::Names, "name", fn_name.as_str())
            .map_err(YulLowerError::Core)?;
        let self_map: BTreeMap<String, NodeKey> = [(fn_name.clone(), self_key)].into();
        lower_function_body(&mut fn_graph, owner, function, &self_map, &fn_path)?;
        out.functions.push(LoweredUnit {
            graph_key: fn_graph_key,
            graph: fn_graph,
            unit: "yulssa-fn",
            name: fn_name.clone(),
        });
    }

    out.objects.push(LoweredUnit {
        graph_key,
        graph,
        unit: "yulssa-object",
        name: name.to_string(),
    });
    Ok(())
}

/// Lower one function's arguments + blocks under its (pre-registered) node.
fn lower_function_body(
    graph: &mut Graph,
    owner: &str,
    function: &Value,
    fn_keys: &BTreeMap<String, NodeKey>,
    fn_path: &str,
) -> Result<(), YulLowerError> {
    let fn_node =
        NodeKey::entity(EntityKey::new("yulssa.fn", owner, fn_path).map_err(YulLowerError::Core)?);

    if let Some(returns) = function.get("numReturns").and_then(Value::as_u64) {
        graph
            .add_field(&fn_node, Dimension::Structure, "num_returns", returns)
            .map_err(YulLowerError::Core)?;
    }

    // Arguments are def sites at known positions.
    let mut defs: BTreeMap<String, NodeKey> = BTreeMap::new();
    if let Some(arguments) = function.get("arguments").and_then(Value::as_array) {
        for (index, argument) in arguments.iter().enumerate() {
            let Some(value_name) = argument.as_str() else {
                continue;
            };
            let arg_node = NodeKey::entity(
                EntityKey::new("yulssa.arg", owner, format!("{fn_path}/arg:{index}"))
                    .map_err(YulLowerError::Core)?,
            );
            graph
                .add_node(arg_node.clone(), "yulssa.arg")
                .map_err(YulLowerError::Core)?;
            graph
                .add_child(&fn_node, "arg", index as u32, &arg_node)
                .map_err(YulLowerError::Core)?;
            defs.insert(value_name.to_string(), arg_node);
        }
    }

    let entry = function.get("entry").and_then(Value::as_str);
    if let Some(blocks) = function.get("blocks").and_then(Value::as_array) {
        lower_blocks(
            graph, owner, &fn_node, blocks, entry, fn_path, fn_keys, &defs,
        )?;
    }
    Ok(())
}

/// Shared block-set lowering for object code and function bodies.
#[allow(clippy::too_many_arguments)]
fn lower_blocks(
    graph: &mut Graph,
    owner: &str,
    parent: &NodeKey,
    blocks: &[Value],
    entry: Option<&str>,
    base: &str,
    fn_keys: &BTreeMap<String, NodeKey>,
    outer_defs: &BTreeMap<String, NodeKey>,
) -> Result<(), YulLowerError> {
    // Pass 1: block nodes + instruction skeleton + def map.
    let mut block_keys: BTreeMap<String, NodeKey> = BTreeMap::new();
    let mut defs = outer_defs.clone();

    for block in blocks {
        let id = block
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| shape("block without id"))?;
        let block_path = format!("{base}/b:{id}");
        let block_node = NodeKey::entity(
            EntityKey::new("yulssa.block", owner, block_path.as_str())
                .map_err(YulLowerError::Core)?,
        );
        graph
            .add_node(block_node.clone(), "yulssa.block")
            .map_err(YulLowerError::Core)?;
        let label = if entry == Some(id) { "entry" } else { "block" };
        graph
            .add_child(parent, label, 0, &block_node)
            .map_err(YulLowerError::Core)?;
        block_keys.insert(id.to_string(), block_node.clone());

        if let Some(instructions) = block.get("instructions").and_then(Value::as_array) {
            for (index, instruction) in instructions.iter().enumerate() {
                let insn_path = format!("{block_path}/i:{index}");
                let insn_node = NodeKey::entity(
                    EntityKey::new("yulssa.insn", owner, insn_path.as_str())
                        .map_err(YulLowerError::Core)?,
                );
                graph
                    .add_node(insn_node.clone(), "yulssa.insn")
                    .map_err(YulLowerError::Core)?;
                graph
                    .add_child(&block_node, "insn", index as u32, &insn_node)
                    .map_err(YulLowerError::Core)?;

                let op = instruction.get("op").and_then(Value::as_str).unwrap_or("");
                if op == "PhiFunction" || op == "LiteralAssignment" || is_evm_builtin(op) {
                    graph
                        .add_field(&insn_node, Dimension::Structure, "op", op)
                        .map_err(YulLowerError::Core)?;
                } else if !op.is_empty() {
                    graph
                        .add_field(&insn_node, Dimension::Names, "callee", op)
                        .map_err(YulLowerError::Core)?;
                    if let Some(fn_key) = fn_keys.get(op) {
                        graph
                            .add_edge(&insn_node, "calls", fn_key, EdgeRole::Dependency)
                            .map_err(YulLowerError::Core)?;
                    }
                }

                if let Some(outs) = instruction.get("out").and_then(Value::as_array) {
                    for value in outs.iter().filter_map(Value::as_str) {
                        defs.insert(value.to_string(), insn_node.clone());
                    }
                }
            }
        }
    }

    // Pass 2: operands, immediates, exits, jumps — defs are complete now.
    for block in blocks {
        let id = block.get("id").and_then(Value::as_str).expect("checked");
        let block_path = format!("{base}/b:{id}");
        let block_node = block_keys[id].clone();

        let entries: Vec<&str> = block
            .get("entries")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();

        if let Some(instructions) = block.get("instructions").and_then(Value::as_array) {
            for (index, instruction) in instructions.iter().enumerate() {
                let insn_path = format!("{block_path}/i:{index}");
                let insn_node = NodeKey::entity(
                    EntityKey::new("yulssa.insn", owner, insn_path.as_str())
                        .map_err(YulLowerError::Core)?,
                );
                let op = instruction.get("op").and_then(Value::as_str).unwrap_or("");
                let is_phi = op == "PhiFunction";

                if let Some(ins) = instruction.get("in").and_then(Value::as_array) {
                    for (operand_index, operand) in ins.iter().enumerate() {
                        let Some(text) = operand.as_str() else {
                            continue;
                        };
                        if is_phi {
                            // labeled (predecessor, value) pair — multiset
                            let pair_node = add_operand_node(
                                graph,
                                owner,
                                &insn_node,
                                "phi-in",
                                0,
                                &format!("{insn_path}/phi:{operand_index}"),
                                "yulssa.use",
                            )?;
                            wire_use(graph, &pair_node, text, &defs)?;
                            if let Some(pred) = entries
                                .get(operand_index)
                                .and_then(|pred| block_keys.get(*pred))
                            {
                                graph
                                    .add_edge(&pair_node, "from", pred, EdgeRole::Control)
                                    .map_err(YulLowerError::Core)?;
                            }
                        } else if is_value_ref(text) {
                            let use_node = add_operand_node(
                                graph,
                                owner,
                                &insn_node,
                                "in",
                                operand_index as u32,
                                &format!("{insn_path}/in:{operand_index}"),
                                "yulssa.use",
                            )?;
                            wire_use(graph, &use_node, text, &defs)?;
                        } else {
                            let lit_node = add_operand_node(
                                graph,
                                owner,
                                &insn_node,
                                "in",
                                operand_index as u32,
                                &format!("{insn_path}/in:{operand_index}"),
                                "yulssa.lit",
                            )?;
                            graph
                                .add_field(
                                    &lit_node,
                                    Dimension::Constants,
                                    "value",
                                    canon_number(text).unwrap_or_else(|_| text.to_string()),
                                )
                                .map_err(YulLowerError::Core)?;
                        }
                    }
                }

                if let Some(immediates) = instruction.get("literalArgs").and_then(Value::as_array) {
                    for (imm_index, immediate) in immediates.iter().enumerate() {
                        let Some(text) = immediate.as_str() else {
                            continue;
                        };
                        let lit_node = add_operand_node(
                            graph,
                            owner,
                            &insn_node,
                            "imm",
                            imm_index as u32,
                            &format!("{insn_path}/imm:{imm_index}"),
                            "yulssa.lit",
                        )?;
                        graph
                            .add_field(
                                &lit_node,
                                Dimension::Constants,
                                "value",
                                canon_number(text).unwrap_or_else(|_| text.to_string()),
                            )
                            .map_err(YulLowerError::Core)?;
                    }
                }
            }
        }

        // Exit: kind in Structure, condition/returns as uses, targets as
        // Dependency edges with order-bearing labels.
        if let Some(exit) = block.get("exit").and_then(Value::as_object) {
            if let Some(exit_type) = exit.get("type").and_then(Value::as_str) {
                graph
                    .add_field(&block_node, Dimension::Structure, "exit", exit_type)
                    .map_err(YulLowerError::Core)?;
            }
            if let Some(cond) = exit.get("cond").and_then(Value::as_str) {
                let use_node = add_operand_node(
                    graph,
                    owner,
                    &block_node,
                    "cond",
                    u32::MAX,
                    &format!("{block_path}/cond"),
                    "yulssa.use",
                )?;
                wire_use(graph, &use_node, cond, &defs)?;
            }
            if let Some(returns) = exit.get("returnValues").and_then(Value::as_array) {
                for (ret_index, value) in returns.iter().enumerate() {
                    let Some(text) = value.as_str() else { continue };
                    let use_node = add_operand_node(
                        graph,
                        owner,
                        &block_node,
                        "ret",
                        ret_index as u32,
                        &format!("{block_path}/ret:{ret_index}"),
                        "yulssa.use",
                    )?;
                    if is_value_ref(text) {
                        wire_use(graph, &use_node, text, &defs)?;
                    } else {
                        graph
                            .add_field(
                                &use_node,
                                Dimension::Constants,
                                "value",
                                canon_number(text).unwrap_or_else(|_| text.to_string()),
                            )
                            .map_err(YulLowerError::Core)?;
                    }
                }
            }
            if let Some(targets) = exit.get("targets").and_then(Value::as_array) {
                for (target_index, target) in targets.iter().enumerate() {
                    let Some(target_id) = target.as_str() else {
                        continue;
                    };
                    if let Some(target_node) = block_keys.get(target_id) {
                        graph
                            .add_edge(
                                &block_node,
                                format!("jump:{target_index}"),
                                target_node,
                                EdgeRole::Dependency,
                            )
                            .map_err(YulLowerError::Core)?;
                    }
                }
            }
        }
    }

    Ok(())
}

fn add_operand_node(
    graph: &mut Graph,
    owner: &str,
    parent: &NodeKey,
    label: &str,
    ordinal: u32,
    path: &str,
    kind: &str,
) -> Result<NodeKey, YulLowerError> {
    let node = NodeKey::entity(EntityKey::new(kind, owner, path).map_err(YulLowerError::Core)?);
    graph
        .add_node(node.clone(), kind)
        .map_err(YulLowerError::Core)?;
    graph
        .add_child(parent, label, ordinal, &node)
        .map_err(YulLowerError::Core)?;
    Ok(node)
}

/// Wire a value use to its definition. `vN` text itself never becomes a
/// field — only the Data edge to the def site carries the information (I15).
/// Unresolved values (cross-block liveness oddities) become a Structure
/// `free` marker so they at least perturb shape deterministically.
fn wire_use(
    graph: &mut Graph,
    use_node: &NodeKey,
    value: &str,
    defs: &BTreeMap<String, NodeKey>,
) -> Result<(), YulLowerError> {
    match defs.get(value) {
        Some(def) => graph
            .add_edge(use_node, "def", def, EdgeRole::Data)
            .map_err(YulLowerError::Core),
        None => graph
            .add_field(use_node, Dimension::Structure, "free", true)
            .map_err(YulLowerError::Core),
    }
}

fn is_value_ref(text: &str) -> bool {
    text.strip_prefix('v')
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit()))
}
