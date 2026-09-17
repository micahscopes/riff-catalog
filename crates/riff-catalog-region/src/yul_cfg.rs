//! Explicit block-region materialization from the experimental solc SSA JSON.
//! This is a new incidence contract, not a change to historical yul-ssa-cfg/1.
use crate::exact::{Edge, Graph};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const CONTRACT: &str = "yul-ported-cfg/1";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Region {
    pub contract: String,
    pub graph: Graph,
    pub occurrences: Vec<Value>,
    pub selected_blocks: Vec<String>,
    pub has_control_cycle: bool,
}

#[derive(Debug, Serialize)]
pub struct Comparison {
    pub schema: String,
    pub left: Region,
    pub right: Region,
    pub left_outcome: crate::exact::Outcome,
    pub right_outcome: crate::exact::Outcome,
    pub left_content_id: Option<String>,
    pub right_content_id: Option<String>,
    pub equivalent: Option<bool>,
    pub witness: Option<Vec<usize>>,
}

pub fn compare(left: Region, right: Region, budget: crate::exact::Budget) -> Result<Comparison> {
    use crate::exact::{Outcome, canonicalize, verify};
    let left_outcome = canonicalize(&left.graph, budget);
    let right_outcome = canonicalize(&right.graph, budget);
    let address = |outcome: &Outcome| -> Result<Option<String>> {
        match outcome {
            Outcome::Exact { canonical, .. } => {
                let mut hash =
                    blake3::Hasher::new_derive_key("riffcat yul ported CFG canonical record v1");
                hash.update(&serde_json::to_vec(&(CONTRACT, canonical))?);
                Ok(Some(format!("yulcfg1:{}", hash.finalize().to_hex())))
            }
            _ => Ok(None),
        }
    };
    let left_content_id = address(&left_outcome)?;
    let right_content_id = address(&right_outcome)?;
    let (equivalent, witness) = match (&left_outcome, &right_outcome) {
        (
            Outcome::Exact {
                canonical: a,
                mapping: am,
                ..
            },
            Outcome::Exact {
                canonical: b,
                mapping: bm,
                ..
            },
        ) => {
            if a == b {
                let mut inverse = vec![0; bm.len()];
                for (old, &new) in bm.iter().enumerate() {
                    inverse[new] = old;
                }
                let mapping: Vec<_> = am.iter().map(|&id| inverse[id]).collect();
                ensure!(
                    verify(&left.graph, &right.graph, &mapping),
                    "internal error: comparison witness rejected"
                );
                (Some(true), Some(mapping))
            } else {
                (Some(false), None)
            }
        }
        _ => (None, None),
    };
    Ok(Comparison {
        schema: "riffcat-yul-region-comparison/1".into(),
        left,
        right,
        left_outcome,
        right_outcome,
        left_content_id,
        right_content_id,
        equivalent,
        witness,
    })
}

fn fields(value: &Value, allowed: &[&str]) -> Result<()> {
    for key in value.as_object().context("expected object")?.keys() {
        ensure!(
            allowed.contains(&key.as_str()),
            "unsupported producer field {key}"
        );
    }
    Ok(())
}
fn strings(value: &Value) -> Result<Vec<&str>> {
    value
        .as_array()
        .context("expected array")?
        .iter()
        .map(|v| v.as_str().context("expected string"))
        .collect()
}
fn optional_strings<'a>(value: &'a Value, key: &str) -> Result<Vec<&'a str>> {
    value.get(key).map_or(Ok(Vec::new()), strings)
}

struct Builder {
    graph: Graph,
    occurrences: Vec<Value>,
}
impl Builder {
    fn node(&mut self, label: Value, occurrence: Value) -> usize {
        let id = self.graph.labels.len();
        self.graph.labels.push(label.to_string());
        self.occurrences.push(occurrence);
        id
    }
    fn edge(&mut self, from: usize, to: usize, role: impl Into<String>) {
        self.graph.edges.push(Edge {
            from,
            to,
            role: role.into(),
        });
    }
    fn value(
        &mut self,
        value: &str,
        values: &mut BTreeMap<String, usize>,
        definitions: &BTreeSet<String>,
    ) -> Result<usize> {
        if let Some(&id) = values.get(value) {
            return Ok(id);
        }
        let label = if definitions.contains(value) {
            json!({"kind":"input-port"})
        } else {
            ensure!(
                value
                    .strip_prefix("0x")
                    .is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit())),
                "unknown SSA value or unsupported literal {value}"
            );
            json!({"kind":"literal-token","value":value})
        };
        let id = self.node(label, json!({"value":value}));
        values.insert(value.into(), id);
        Ok(id)
    }
}

/// Empty selection means all function blocks. A nonempty selection can be
/// disconnected or multi-entry; no natural-loop-tree assumption is made.
pub fn materialize(function: &Value, selected: &[String]) -> Result<Region> {
    fields(
        function,
        &["arguments", "blocks", "entry", "numReturns", "type"],
    )?;
    ensure!(function["type"] == "Function", "expected function CFG");
    ensure!(
        function["numReturns"].as_u64().is_some(),
        "missing return arity"
    );
    let blocks = function["blocks"].as_array().context("missing blocks")?;
    ensure!(blocks.len() <= 10_000, "producer block budget");
    let mut by_id = BTreeMap::new();
    let mut definitions = BTreeSet::new();
    for arg in strings(&function["arguments"])? {
        ensure!(
            definitions.insert(arg.to_owned()),
            "duplicate SSA definition"
        );
    }
    for block in blocks {
        fields(
            block,
            &["id", "entries", "exit", "instructions", "liveness", "type"],
        )?;
        let id = block["id"].as_str().context("missing block ID")?;
        ensure!(by_id.insert(id, block).is_none(), "duplicate block ID");
        for ins in block["instructions"]
            .as_array()
            .context("missing instructions")?
        {
            fields(ins, &["op", "in", "out", "literalArgs"])?;
            ensure!(ins["op"].as_str().is_some(), "missing opcode");
            strings(&ins["in"])?;
            for out in strings(&ins["out"])? {
                ensure!(
                    definitions.insert(out.to_owned()),
                    "duplicate SSA definition"
                );
            }
        }
    }
    let mut instruction_count = 0usize;
    ensure!(
        definitions
            .iter()
            .all(|name| !name.is_empty() && !name.starts_with("0x")),
        "invalid SSA definition name"
    );
    for block in blocks {
        for ins in block["instructions"].as_array().unwrap() {
            instruction_count += 1;
            ensure!(instruction_count <= 10_000, "producer instruction budget");
            for input in strings(&ins["in"])? {
                ensure!(
                    definitions.contains(input)
                        || input.strip_prefix("0x").is_some_and(
                            |s| !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit())
                        ),
                    "unknown SSA value {input}"
                );
            }
        }
    }
    let entry = function["entry"].as_str().context("missing entry")?;
    ensure!(by_id.contains_key(entry), "entry block missing");
    let selection: BTreeSet<&str> = if selected.is_empty() {
        by_id.keys().copied().collect()
    } else {
        selected.iter().map(String::as_str).collect()
    };
    ensure!(
        selection.len() == selected.len() || selected.is_empty(),
        "duplicate selected block"
    );
    ensure!(
        selection.iter().all(|id| by_id.contains_key(id)),
        "unknown selected block"
    );
    let whole = selection.len() == blocks.len();
    let mut predecessors: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (&id, block) in &by_id {
        let exit = &block["exit"];
        fields(exit, &["type", "targets", "cond", "returnValues"])?;
        let targets = optional_strings(exit, "targets")?;
        match exit["type"].as_str() {
            Some("Jump") => ensure!(
                targets.len() == 1
                    && exit.get("cond").is_none()
                    && exit.get("returnValues").is_none(),
                "invalid jump"
            ),
            Some("ConditionalJump") => ensure!(
                targets.len() == 2
                    && exit["cond"].is_string()
                    && exit.get("returnValues").is_none(),
                "invalid conditional jump"
            ),
            Some("FunctionReturn") => ensure!(
                targets.is_empty()
                    && exit.get("cond").is_none()
                    && strings(&exit["returnValues"])?.len()
                        == function["numReturns"]
                            .as_u64()
                            .context("missing return arity")? as usize,
                "invalid return"
            ),
            Some("Terminated") => ensure!(
                targets.is_empty()
                    && exit.get("cond").is_none()
                    && exit.get("returnValues").is_none(),
                "invalid termination"
            ),
            _ => anyhow::bail!("unsupported exit type"),
        }
        for target in targets {
            ensure!(by_id.contains_key(target), "unknown jump target");
            predecessors.entry(target).or_default().insert(id);
        }
    }
    let mut b = Builder {
        graph: Graph {
            labels: Vec::new(),
            edges: Vec::new(),
        },
        occurrences: Vec::new(),
    };
    let root = b.node(
        json!({"kind":"region","contract":CONTRACT,"whole_function":whole,"return_arity":if whole {function.get("numReturns")}else{None}}),
        json!({"function":true}),
    );
    let mut block_nodes = BTreeMap::new();
    let mut values = BTreeMap::new();
    for &id in &selection {
        let node = b.node(json!({"kind":"block"}), json!({"block":id}));
        b.edge(
            root,
            node,
            if id == entry {
                "function-entry"
            } else {
                "block"
            },
        );
        block_nodes.insert(id, node);
    }
    // Crossing control entities are shared, so phi/control attachments agree.
    for (&id, block) in &by_id {
        for target in optional_strings(&block["exit"], "targets")? {
            if selection.contains(id) != selection.contains(target) {
                let outside = if selection.contains(id) { target } else { id };
                block_nodes.entry(outside).or_insert_with(|| {
                    b.node(
                        json!({"kind":"control-port"}),
                        json!({"block":outside,"outside":true}),
                    )
                });
            }
        }
    }
    if whole {
        for (i, arg) in strings(&function["arguments"])?.into_iter().enumerate() {
            let node = b.value(arg, &mut values, &definitions)?;
            b.edge(root, node, format!("argument:{i}"));
        }
    }
    let mut instructions = BTreeMap::new();
    for &id in &selection {
        for (i, ins) in by_id[id]["instructions"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let node = b.node(
                json!({"kind":"instruction","op":ins["op"],"literalArgs":ins.get("literalArgs")}),
                json!({"block":id,"instruction":i}),
            );
            b.edge(block_nodes[id], node, format!("instruction:{i}"));
            instructions.insert((id, i), node);
            for (slot, out) in strings(&ins["out"])?.into_iter().enumerate() {
                let v = b.node(
                    json!({"kind":"result"}),
                    json!({"value":out,"result_slot":slot}),
                );
                ensure!(
                    values.insert(out.into(), v).is_none(),
                    "result collides with argument"
                );
                b.edge(node, v, format!("result:{slot}"));
            }
        }
    }
    let mut exports = BTreeSet::new();
    for (&id, block) in &by_id {
        let inside = selection.contains(id);
        let entries = optional_strings(block, "entries")?;
        for (i, ins) in block["instructions"].as_array().unwrap().iter().enumerate() {
            let inputs = strings(&ins["in"])?;
            if ins["op"] == "PhiFunction" {
                ensure!(
                    inputs.len() == entries.len(),
                    "phi inputs and predecessors differ"
                );
                let declared: BTreeSet<_> = entries.iter().copied().collect();
                ensure!(
                    declared.len() == entries.len()
                        && declared == predecessors.get(id).cloned().unwrap_or_default(),
                    "phi predecessor list does not match CFG"
                );
            }
            for (slot, input) in inputs.into_iter().enumerate() {
                if !inside {
                    if values.contains_key(input) {
                        exports.insert(input);
                    }
                    continue;
                }
                let value = b.value(input, &mut values, &definitions)?;
                let node = instructions[&(id, i)];
                if ins["op"] == "PhiFunction" {
                    let pair = b.node(
                        json!({"kind":"phi-input"}),
                        json!({"block":id,"instruction":i,"predecessor":entries[slot]}),
                    );
                    b.edge(node, pair, "phi-input");
                    b.edge(pair, value, "value");
                    b.edge(pair, block_nodes[entries[slot]], "predecessor");
                } else {
                    b.edge(node, value, format!("operand:{slot}"));
                }
            }
        }
        let exit = &block["exit"];
        for (slot, target) in optional_strings(exit, "targets")?.into_iter().enumerate() {
            if inside || selection.contains(target) {
                b.edge(
                    block_nodes[id],
                    block_nodes[target],
                    format!("control:{slot}"),
                );
            }
        }
        if inside {
            let node = b.node(
                json!({"kind":"exit","type":exit["type"]}),
                json!({"block":id,"exit":true}),
            );
            b.edge(block_nodes[id], node, "exit");
            if let Some(cond) = exit.get("cond") {
                let v = b.value(
                    cond.as_str().context("invalid condition")?,
                    &mut values,
                    &definitions,
                )?;
                b.edge(node, v, "condition");
            }
            for (slot, value) in optional_strings(exit, "returnValues")?
                .into_iter()
                .enumerate()
            {
                let v = b.value(value, &mut values, &definitions)?;
                b.edge(node, v, format!("return:{slot}"));
            }
        } else {
            for value in optional_strings(exit, "returnValues")?
                .into_iter()
                .chain(exit.get("cond").and_then(Value::as_str))
            {
                if values.contains_key(value) {
                    exports.insert(value);
                }
            }
        }
    }
    // Exports are a set of produced values, not external consumer multiplicity.
    for value in exports {
        let v = values[value];
        if b.graph.labels[v] == json!({"kind":"result"}).to_string() {
            b.edge(root, v, "export");
        }
    }
    // Kahn elimination avoids recursion on potentially long producer CFGs.
    let mut degree: BTreeMap<_, usize> = selection.iter().map(|&id| (id, 0)).collect();
    for &id in &selection {
        for target in optional_strings(&by_id[id]["exit"], "targets")? {
            if let Some(d) = degree.get_mut(target) {
                *d += 1;
            }
        }
    }
    let mut ready: Vec<_> = degree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(&id, _)| id)
        .collect();
    let mut removed = 0;
    while let Some(id) = ready.pop() {
        removed += 1;
        for target in optional_strings(&by_id[id]["exit"], "targets")? {
            if let Some(d) = degree.get_mut(target) {
                *d -= 1;
                if *d == 0 {
                    ready.push(target);
                }
            }
        }
    }
    let has_control_cycle = removed < selection.len();
    Ok(Region {
        contract: CONTRACT.into(),
        graph: b.graph,
        occurrences: b.occurrences,
        selected_blocks: selection.into_iter().map(str::to_owned).collect(),
        has_control_cycle,
    })
}

/// Generic explicit instruction-window selection for the ordered pure fast path.
/// Exports are relative instruction ordinals supplied by the caller.
pub fn pure_window(
    function: &Value,
    block: &str,
    start: usize,
    end: usize,
    exports: Vec<usize>,
) -> Result<crate::Region> {
    let blocks = function["blocks"].as_array().context("missing blocks")?;
    let block = blocks
        .iter()
        .find(|b| b["id"] == block)
        .context("unknown block")?;
    let instructions = block["instructions"]
        .as_array()
        .context("missing instructions")?;
    ensure!(start < end, "empty instruction window");
    let window = instructions
        .get(start..end)
        .context("invalid instruction window")?;
    let mut definitions = BTreeMap::new();
    for (i, ins) in window.iter().enumerate() {
        let outputs = strings(&ins["out"])?;
        ensure!(
            outputs.len() == 1,
            "pure window needs single-result operations"
        );
        ensure!(
            definitions.insert(outputs[0], i).is_none(),
            "duplicate result"
        );
    }
    let mut all_definitions: BTreeSet<_> = strings(&function["arguments"])?.into_iter().collect();
    for block in blocks {
        for ins in block["instructions"]
            .as_array()
            .context("missing instructions")?
        {
            for output in strings(&ins["out"])? {
                ensure!(all_definitions.insert(output), "duplicate SSA definition");
            }
        }
    }
    let mut operations = Vec::new();
    for ins in window {
        fields(ins, &["op", "in", "out", "literalArgs"])?;
        let outputs = strings(&ins["out"])?;
        ensure!(
            outputs.len() == 1 && ins.get("literalArgs").is_none(),
            "pure window needs single-result operations without immediates"
        );
        let operands = strings(&ins["in"])?
            .into_iter()
            .map(|value| -> Result<crate::Operand> {
                if let Some(&index) = definitions.get(value) {
                    Ok(crate::Operand::Result(index))
                } else if value.starts_with("0x") {
                    ensure!(
                        value.len() > 2 && value[2..].chars().all(|c| c.is_ascii_hexdigit()),
                        "invalid literal"
                    );
                    Ok(crate::Operand::Literal(value.into()))
                } else {
                    ensure!(all_definitions.contains(value), "unknown external value");
                    Ok(crate::Operand::External(value.into()))
                }
            })
            .collect::<Result<_>>()?;
        operations.push(crate::Operation {
            op: ins["op"].as_str().context("missing op")?.into(),
            operands,
        });
    }
    let region = crate::Region {
        operations,
        outputs: exports,
    };
    region.normalize()?;
    Ok(region)
}
