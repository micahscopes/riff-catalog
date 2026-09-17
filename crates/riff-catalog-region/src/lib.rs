//! Experimental comparison contract for ordered, pure, single-result operations.
//!
//! A region is explicitly selected. External definitions become abstract inputs,
//! numbered by first use in semantic instruction/operand order. Exported results
//! are ordered by their defining instruction. Consumer identities and multiplicity
//! are occurrence context. This does not normalize arbitrary control flow, effects,
//! function signatures, or unordered graphs.

pub mod protocol;
pub mod exact;
pub mod wl_observation;

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail, ensure};
use riff_catalog_core::{
    CyclePolicy, Digest, DigestRequest, Dimension, EdgeRole, EntityKey, Facet, Graph, GraphKey,
    HashPolicy, NodeKey, Value, ViewMode, digest_graph,
};
use serde::{Deserialize, Serialize};

pub const CONTRACT: &str = "pilot:ordered-pure-u256-region/1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operand {
    External(String),
    Result(usize),
    Literal(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub op: String,
    pub operands: Vec<Operand>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub operations: Vec<Operation>,
    /// Exported values in defining-instruction order, independent of consumers.
    pub outputs: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NormalOperand {
    Input(usize),
    Result(usize),
    Literal(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalOperation {
    pub op: String,
    pub operands: Vec<NormalOperand>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalRegion {
    pub inputs: usize,
    pub operations: Vec<NormalOperation>,
    pub outputs: Vec<usize>,
}

impl Region {
    /// Return content separately from the occurrence's external-definition keys.
    pub fn normalize(&self) -> Result<(NormalRegion, Vec<String>)> {
        ensure!(!self.operations.is_empty(), "empty selection");
        ensure!(
            self.outputs.windows(2).all(|pair| pair[0] < pair[1])
                && self
                    .outputs
                    .iter()
                    .all(|&slot| slot < self.operations.len()),
            "outputs must be distinct valid slots in definition order"
        );
        let mut inputs = BTreeMap::new();
        let mut bindings = Vec::new();
        let mut operations = Vec::new();
        for (slot, operation) in self.operations.iter().enumerate() {
            ensure!(
                matches!(
                    operation.op.as_str(),
                    "add" | "mul" | "xor" | "sub" | "and" | "or"
                ),
                "unsupported pure operation: {}",
                operation.op
            );
            ensure!(operation.operands.len() == 2, "expected two operands");
            let mut operands = Vec::new();
            for operand in &operation.operands {
                operands.push(match operand {
                    Operand::External(key) => {
                        let next = inputs.len();
                        let index = *inputs.entry(key.clone()).or_insert_with(|| {
                            bindings.push(key.clone());
                            next
                        });
                        NormalOperand::Input(index)
                    }
                    Operand::Result(index) => {
                        ensure!(*index < slot, "region must have backward result references");
                        NormalOperand::Result(*index)
                    }
                    Operand::Literal(value) => NormalOperand::Literal(value.clone()),
                });
            }
            operations.push(NormalOperation {
                op: operation.op.clone(),
                operands,
            });
        }
        Ok((
            NormalRegion {
                inputs: inputs.len(),
                operations,
                outputs: self.outputs.clone(),
            },
            bindings,
        ))
    }
}

impl NormalRegion {
    pub fn graph(&self) -> Result<Graph> {
        // NormalRegion is public and deserializable. Never index its references
        // before checking that it is the canonical result of a valid raw region.
        let raw = Region {
            operations: self
                .operations
                .iter()
                .map(|operation| Operation {
                    op: operation.op.clone(),
                    operands: operation
                        .operands
                        .iter()
                        .map(|operand| match operand {
                            NormalOperand::Input(index) => {
                                Operand::External(format!("input:{index}"))
                            }
                            NormalOperand::Result(index) => Operand::Result(*index),
                            NormalOperand::Literal(value) => Operand::Literal(value.clone()),
                        })
                        .collect(),
                })
                .collect(),
            outputs: self.outputs.clone(),
        };
        ensure!(
            raw.normalize()?.0 == *self,
            "invalid or noncanonical normalized region"
        );
        let owner = EntityKey::new("pilot.region", "comparison", "root")?;
        let key = |local: String| NodeKey::derived(owner.clone(), local).unwrap();
        let mut graph = Graph::new(GraphKey::new(owner.clone(), "body")?);
        let root = key("root".into());
        graph.add_node(root.clone(), "ported.region")?;
        let mut inputs = Vec::new();
        for slot in 0..self.inputs {
            let node = key(format!("input:{slot}"));
            graph.add_node(node.clone(), "u256.input")?;
            graph.add_child(&root, "input", slot as u32, &node)?;
            inputs.push(node);
        }
        let mut results = Vec::new();
        for (slot, operation) in self.operations.iter().enumerate() {
            let node = key(format!("op:{slot}"));
            graph.add_node(node.clone(), "u256.operation")?;
            graph.add_field(&node, Dimension::Structure, "op", operation.op.as_str())?;
            graph.add_child(&root, "operation", slot as u32, &node)?;
            for (position, operand) in operation.operands.iter().enumerate() {
                let use_node = key(format!("op:{slot}/operand:{position}"));
                graph.add_node(use_node.clone(), "u256.operand")?;
                graph.add_child(&node, "operand", position as u32, &use_node)?;
                match operand {
                    NormalOperand::Input(index) => {
                        // Both ends need semantic positions: operand leaf
                        // hashes alone forget their enclosing instruction.
                        // These positions come from this ordered grammar,
                        // never producer IDs or container-relative offsets.
                        graph.add_edge(
                            &use_node,
                            format!("op:{slot}/operand:{position}/input:{index}"),
                            &inputs[*index],
                            EdgeRole::Data,
                        )?;
                    }
                    NormalOperand::Result(index) => {
                        graph.add_edge(
                            &use_node,
                            format!("op:{slot}/operand:{position}/result:{index}"),
                            &results[*index],
                            EdgeRole::Data,
                        )?;
                    }
                    NormalOperand::Literal(value) => {
                        graph.add_field(
                            &use_node,
                            Dimension::Constants,
                            "value",
                            value.as_str(),
                        )?;
                    }
                }
            }
            results.push(node);
        }
        for (position, &index) in self.outputs.iter().enumerate() {
            let output = key(format!("output:{position}"));
            graph.add_node(output.clone(), "u256.output")?;
            graph.add_child(&root, "output", position as u32, &output)?;
            graph.add_edge(
                &output,
                format!("result:{index}"),
                &results[index],
                EdgeRole::Data,
            )?;
        }
        graph.validate()?;
        Ok(graph)
    }

    pub fn address(&self, retain_literals: bool) -> Result<Digest> {
        address_graph(&self.graph()?, CONTRACT, retain_literals)
    }
}

pub fn address_graph(graph: &Graph, contract: &str, retain_literals: bool) -> Result<Digest> {
    let policy = HashPolicy::new(contract, ViewMode::AnonymousShape, CyclePolicy::CondenseScc)?;
    let facet = Facet::new(
        policy.policy_id(),
        if retain_literals {
            vec![Dimension::Structure, Dimension::Constants]
        } else {
            vec![Dimension::Structure]
        },
    )?;
    let result = digest_graph(
        &DigestRequest::all_dimensions(graph.graph_key.clone(), policy),
        graph,
    )?;
    Ok(result.hashes.facet_address(&facet)?.address_digest())
}

fn text_field<'a>(graph: &'a Graph, key: &NodeKey, name: &str) -> Result<&'a str> {
    graph.nodes[key]
        .fields
        .iter()
        .find_map(|field| {
            if field.name.as_str() == name {
                if let Value::Text(text) = &field.value {
                    return Some(text.as_str());
                }
            }
            None
        })
        .ok_or_else(|| anyhow::anyhow!("missing text field {name}"))
}

/// Select the unique add/mul/xor window, then recover its data boundary from
/// real SSA edges. This is an explicit pilot selector, not region discovery.
pub fn from_yul_graph(graph: &Graph) -> Result<Region> {
    graph.validate()?;
    let mut candidates = Vec::new();
    for (block, node) in &graph.nodes {
        if node.kind.as_str() != "yulssa.block" {
            continue;
        }
        let mut instructions: Vec<_> = graph
            .children
            .iter()
            .filter(|edge| edge.parent == *block && edge.label.as_str() == "insn")
            .collect();
        instructions.sort_by_key(|edge| edge.ordinal);
        for window in instructions.windows(3) {
            if window
                .iter()
                .map(|edge| text_field(graph, &edge.child, "op").unwrap_or(""))
                .eq(["add", "mul", "xor"])
            {
                candidates.push(
                    window
                        .iter()
                        .map(|edge| edge.child.clone())
                        .collect::<Vec<_>>(),
                );
            }
        }
    }
    ensure!(
        candidates.len() == 1,
        "expected one explicit payload, found {}",
        candidates.len()
    );
    let selected = &candidates[0];
    let slots: BTreeMap<_, _> = selected
        .iter()
        .enumerate()
        .map(|(i, key)| (key.clone(), i))
        .collect();
    let mut members: BTreeSet<_> = selected.iter().cloned().collect();
    let mut operations = Vec::new();
    for instruction in selected {
        let mut children: Vec<_> = graph
            .children
            .iter()
            .filter(|edge| edge.parent == *instruction)
            .collect();
        children.sort_by_key(|edge| edge.ordinal);
        ensure!(
            children
                .iter()
                .enumerate()
                .all(|(i, edge)| edge.label.as_str() == "in" && edge.ordinal == i as u32),
            "expected contiguous ordered operands"
        );
        let mut operands = Vec::new();
        for child in children {
            members.insert(child.child.clone());
            let node = &graph.nodes[&child.child];
            match node.kind.as_str() {
                "yulssa.lit" => operands.push(Operand::Literal(
                    text_field(graph, &child.child, "value")?.into(),
                )),
                "yulssa.use" => {
                    let edges: Vec<_> = graph
                        .edges
                        .iter()
                        .filter(|edge| edge.source == child.child)
                        .collect();
                    ensure!(
                        edges.len() == 1
                            && edges[0].role == EdgeRole::Data
                            && edges[0].label.as_str() == "def",
                        "expected one resolved definition per operand"
                    );
                    operands.push(match slots.get(&edges[0].target) {
                        Some(index) => Operand::Result(*index),
                        None => Operand::External(edges[0].target.canonical_key()),
                    });
                }
                other => bail!("unsupported operand kind {other}"),
            }
        }
        operations.push(Operation {
            op: text_field(graph, instruction, "op")?.into(),
            operands,
        });
    }
    // Reject unmodelled retained relations instead of silently cutting them.
    for edge in &graph.edges {
        if members.contains(&edge.source) || members.contains(&edge.target) {
            ensure!(
                edge.role == EdgeRole::Data && edge.label.as_str() == "def",
                "unsupported region relation"
            );
        }
    }
    let outputs = selected
        .iter()
        .enumerate()
        .filter_map(|(slot, key)| {
            graph
                .edges
                .iter()
                .any(|edge| edge.target == *key && !members.contains(&edge.source))
                .then_some(slot)
        })
        .collect();
    let region = Region {
        operations,
        outputs,
    };
    region.normalize()?;
    Ok(region)
}
