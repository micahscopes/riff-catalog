//! Lower the stack-layout event stream emitted by solc's `yulssatrace` tool.
//!
//! Each event becomes a standalone graph. Stack slot spellings are used only
//! while building the graph: occurrences point to shared, anonymous slot nodes,
//! so anonymous-shape addresses retain the equality pattern without depending
//! on SSA value numbers. Problem and result subtrees remain separate so a
//! `.riffview` can address either the search key or the observed solution.

use std::collections::BTreeMap;

use riff_catalog_core::{Dimension, EdgeRole, EntityKey, Graph, GraphKey, NodeKey};
use serde_json::{Map, Value, json};
use thiserror::Error;

use crate::lower::LoweredUnit;
use crate::ssa::YUL_SSA_LEVEL;
use crate::trace::{SOLC_SSA_OBSERVATION_SCHEMA, SsaTraceError, parse_solc_ssa_trace};

pub const SOLC_STACK_EVENT_STREAM_SCHEMA: &str = "solc-stack-layout-event-stream/1";
pub const SOLC_COMPILER_EVENT_STREAM_SCHEMA: &str = "solc-compiler-event-stream/1";
pub const SOLC_STACK_EVENT_LEVEL: &str = "solc-stack-event/1";

const SHUFFLE_SCHEMA: &str = "solc-shuffle-observation/2";
const TARGET_SCHEMA: &str = "solc-stack-target-observation/1";
const STACK_IN_SCHEMA: &str = "solc-stack-in-choice/2";
const LAYOUT_ITERATION_SCHEMA: &str = "solc-stack-layout-iteration/1";
const SPILL_CLOSURE_SCHEMA: &str = "solc-spill-closure-observation/1";
const PLAYBACK_SCHEMA: &str = "solc-shuffle-playback-observation/1";
const COMPILER_RESULT_SCHEMA: &str = "solc-compiler-result/1";
const STACK_IN_TRADEOFF_SCHEMA: &str = "riffcat-solc-stack-in-tradeoff/1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackTraceMetadata {
    pub schema: String,
    pub source: String,
    pub object: String,
    pub stack_in_policy_digest: Option<String>,
}

#[derive(Clone, Debug)]
pub struct StackTraceObservation {
    pub record_kind: String,
    pub schema: String,
    pub stage_kind: String,
    pub stage: String,
    pub level: &'static str,
    pub ordinal: u64,
    pub function_graph_id: u64,
    pub function: Option<String>,
    pub layout_iteration: Option<u64>,
    pub duration_us: u64,
    pub metrics: BTreeMap<String, u64>,
    pub unit: LoweredUnit,
}

#[derive(Clone, Debug)]
pub struct SolcStackTrace {
    pub metadata: StackTraceMetadata,
    pub observations: Vec<StackTraceObservation>,
}

#[derive(Debug, Error)]
pub enum StackTraceError {
    #[error("stack trace line {line}: invalid JSON: {source}")]
    Json {
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error("stack trace line {line}: {message}")]
    Shape { line: usize, message: String },
    #[error("stack trace line {line}: graph lowering failed: {source}")]
    Lower {
        line: usize,
        #[source]
        source: riff_catalog_core::CatalogError,
    },
    #[error("stack trace line {line}: SSA observation failed: {source}")]
    Ssa {
        line: usize,
        #[source]
        source: SsaTraceError,
    },
}

pub fn parse_solc_stack_trace(input: &str, owner: &str) -> Result<SolcStackTrace, StackTraceError> {
    let mut metadata = None;
    let mut pending = Vec::new();

    for (index, raw) in input.lines().enumerate() {
        let line = index + 1;
        if raw.trim().is_empty() {
            continue;
        }
        let value: Value =
            serde_json::from_str(raw).map_err(|source| StackTraceError::Json { line, source })?;
        let object = value
            .as_object()
            .ok_or_else(|| shape(line, "record is not an object"))?;
        let record = required_str(object, "record", line)?;
        if record == "metadata" {
            if metadata.is_some() {
                return Err(shape(line, "duplicate metadata record"));
            }
            let schema = required_str(object, "schema", line)?;
            if !matches!(
                schema,
                SOLC_STACK_EVENT_STREAM_SCHEMA | SOLC_COMPILER_EVENT_STREAM_SCHEMA
            ) {
                return Err(shape(
                    line,
                    format!(
                        "unsupported stream schema `{schema}`; expected `{SOLC_STACK_EVENT_STREAM_SCHEMA}` or `{SOLC_COMPILER_EVENT_STREAM_SCHEMA}`"
                    ),
                ));
            }
            metadata = Some(StackTraceMetadata {
                schema: schema.to_string(),
                source: required_str(object, "source", line)?.to_string(),
                object: required_str(object, "object", line)?.to_string(),
                stack_in_policy_digest: optional_metadata_string(
                    object,
                    "stack_in_policy_digest",
                    line,
                )?,
            });
        } else {
            validate_event_schema(record, required_str(object, "schema", line)?, line)?;
            pending.push((line, value));
        }
    }

    let metadata = metadata.ok_or_else(|| shape(0, "missing metadata record"))?;
    if pending.is_empty() {
        return Err(shape(0, "trace contains no stack events"));
    }
    let mut tradeoffs = lower_stack_in_tradeoffs(&pending, &metadata, owner)?;
    let mut observations = pending
        .into_iter()
        .map(|(line, value)| lower_observation(value, &metadata, owner, line))
        .collect::<Result<Vec<_>, _>>()?;
    observations.append(&mut tradeoffs);
    Ok(SolcStackTrace {
        metadata,
        observations,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct StackInSite {
    object: String,
    function_graph_id: u64,
    block: u64,
    layout_iteration: u64,
}

#[derive(Clone, Copy, Debug)]
struct StackInCost {
    gas: u64,
    spills: u64,
    stack_size: u64,
}

fn lower_stack_in_tradeoffs(
    pending: &[(usize, Value)],
    metadata: &StackTraceMetadata,
    owner: &str,
) -> Result<Vec<StackTraceObservation>, StackTraceError> {
    let mut groups = BTreeMap::<StackInSite, Vec<(usize, &Map<String, Value>)>>::new();
    for (line, value) in pending {
        let object = value
            .as_object()
            .ok_or_else(|| shape(*line, "record is not an object"))?;
        if object.get("record").and_then(Value::as_str) != Some("stack_in_candidate") {
            continue;
        }
        let site = StackInSite {
            object: object
                .get("object")
                .and_then(Value::as_str)
                .unwrap_or(&metadata.object)
                .to_string(),
            function_graph_id: required_u64(object, "function_graph_id", *line)?,
            block: required_u64(object, "block", *line)?,
            layout_iteration: required_u64(object, "layout_iteration", *line)?,
        };
        groups.entry(site).or_default().push((*line, object));
    }

    let mut observations = Vec::new();
    for (site, candidates) in groups {
        if candidates.len() < 2 {
            continue;
        }
        let defaults = candidates
            .iter()
            .filter(|(_, candidate)| {
                candidate.get("default").and_then(Value::as_bool) == Some(true)
            })
            .collect::<Vec<_>>();
        if defaults.len() != 1 {
            return Err(shape(
                candidates[0].0,
                format!(
                    "stack-in site must have exactly one default candidate; found {}",
                    defaults.len()
                ),
            ));
        }
        let default_cost = stack_in_cost(defaults[0].1, defaults[0].0)?;
        let candidate_count = candidates.len() as u64;
        for (line, candidate) in candidates {
            let cost = stack_in_cost(candidate, line)?;
            let (gas_saving, gas_penalty) = saving_and_penalty(default_cost.gas, cost.gas);
            let (spills_saved, spills_added) = saving_and_penalty(default_cost.spills, cost.spills);
            let (stack_slots_saved, stack_slots_added) =
                saving_and_penalty(default_cost.stack_size, cost.stack_size);
            let derived = json!({
                "record": "stack_in_tradeoff",
                "schema": STACK_IN_TRADEOFF_SCHEMA,
                "object": site.object,
                "function_graph_id": site.function_graph_id,
                "function": candidate.get("function").cloned().unwrap_or(Value::Null),
                "block": site.block,
                "layout_iteration": site.layout_iteration,
                "candidate": required_u64(candidate, "candidate", line)?,
                "default": candidate.get("default").and_then(Value::as_bool).unwrap_or(false),
                "selected": candidate.get("selected").and_then(Value::as_bool).unwrap_or(false),
                "stable_iteration": candidate
                    .get("stable_iteration")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                "profile": {
                    "candidate_count": candidate_count,
                    "gas_saving": gas_saving,
                    "gas_penalty": gas_penalty,
                    "spills_saved": spills_saved,
                    "spills_added": spills_added,
                    "stack_slots_saved": stack_slots_saved,
                    "stack_slots_added": stack_slots_added,
                }
            });
            observations.push(lower_observation(derived, metadata, owner, line)?);
        }
    }
    Ok(observations)
}

fn stack_in_cost(
    candidate: &Map<String, Value>,
    line: usize,
) -> Result<StackInCost, StackTraceError> {
    let cost = candidate
        .get("local_cost")
        .and_then(Value::as_object)
        .ok_or_else(|| shape(line, "stack-in candidate is missing local_cost"))?;
    Ok(StackInCost {
        gas: required_u64(cost, "gas", line)?,
        spills: required_u64(cost, "spills", line)?,
        stack_size: required_u64(cost, "stack_size", line)?,
    })
}

fn saving_and_penalty(default: u64, candidate: u64) -> (u64, u64) {
    if candidate <= default {
        (default - candidate, 0)
    } else {
        (0, candidate - default)
    }
}

fn validate_event_schema(record: &str, schema: &str, line: usize) -> Result<(), StackTraceError> {
    let expected = match record {
        "ssa_observation" => SOLC_SSA_OBSERVATION_SCHEMA,
        "shuffle_observation" => SHUFFLE_SCHEMA,
        "stack_target_observation" => TARGET_SCHEMA,
        "stack_in_candidate" => STACK_IN_SCHEMA,
        "stack_layout_iteration" => LAYOUT_ITERATION_SCHEMA,
        "spill_closure_observation" => SPILL_CLOSURE_SCHEMA,
        "shuffle_playback_observation" => PLAYBACK_SCHEMA,
        "compiler_result" => COMPILER_RESULT_SCHEMA,
        other => return Err(shape(line, format!("unknown record kind `{other}`"))),
    };
    if schema != expected {
        return Err(shape(
            line,
            format!("record `{record}` uses schema `{schema}`; expected `{expected}`"),
        ));
    }
    Ok(())
}

fn lower_observation(
    value: Value,
    metadata: &StackTraceMetadata,
    owner: &str,
    line: usize,
) -> Result<StackTraceObservation, StackTraceError> {
    let object = value.as_object().expect("validated as object");
    let record_kind = required_str(object, "record", line)?.to_string();
    if record_kind == "ssa_observation" {
        return lower_ssa_observation(value, metadata, owner, line);
    }
    let schema = required_str(object, "schema", line)?.to_string();
    let function_graph_id = required_u64(object, "function_graph_id", line)?;
    let function = optional_string(object, "function", line)?;
    let layout_iteration = object.get("layout_iteration").and_then(Value::as_u64);
    let ordinal = object
        .get("ordinal")
        .or_else(|| object.get("iteration"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let duration_us = find_u64(object, &["duration_us"])
        .or_else(|| find_u64(object, &["result", "duration_us"]))
        .unwrap_or(0);
    let mut metrics = extract_metrics(&record_kind, object);
    if let Some(iteration) = layout_iteration {
        metrics.insert("layout_iteration".to_string(), iteration);
    }
    if let Some(stable) = object.get("stable_iteration").and_then(Value::as_bool) {
        metrics.insert("stable_iteration".to_string(), u64::from(stable));
    }
    let local = event_local(object, &record_kind, function_graph_id, ordinal);
    let graph_key = GraphKey::new(
        EntityKey::new("solc.stack.event", owner, &local)
            .map_err(|source| StackTraceError::Lower { line, source })?,
        "solc-stack-event",
    )
    .map_err(|source| StackTraceError::Lower { line, source })?;
    let mut builder = EventGraphBuilder::new(graph_key.clone(), owner, &local);
    builder
        .lower_event(object, &record_kind, &schema)
        .map_err(|source| StackTraceError::Lower { line, source })?;
    let graph = builder.graph;
    graph
        .validate()
        .map_err(|source| StackTraceError::Lower { line, source })?;

    Ok(StackTraceObservation {
        record_kind: record_kind.clone(),
        schema,
        stage_kind: "stack-event".to_string(),
        stage: record_kind.clone(),
        level: SOLC_STACK_EVENT_LEVEL,
        ordinal,
        function_graph_id,
        function: function.clone(),
        layout_iteration,
        duration_us,
        metrics,
        unit: LoweredUnit {
            graph_key,
            graph,
            unit: unit_for_record(&record_kind),
            name: function.unwrap_or_else(|| "<main>".to_string()),
        },
    })
}

fn lower_ssa_observation(
    value: Value,
    metadata: &StackTraceMetadata,
    owner: &str,
    line: usize,
) -> Result<StackTraceObservation, StackTraceError> {
    let object = value.as_object().expect("validated as object");
    let object_path = object
        .get("object")
        .and_then(Value::as_str)
        .unwrap_or(&metadata.object);
    let qualified_owner = format!("{owner}:{object_path}");
    let metadata_record = json!({
        "record": "metadata",
        "schema": SOLC_SSA_OBSERVATION_SCHEMA,
        "source": metadata.source,
        "object": object_path,
    });
    let input = format!(
        "{}\n{}",
        serde_json::to_string(&metadata_record)
            .map_err(|source| StackTraceError::Json { line, source })?,
        serde_json::to_string(&value).map_err(|source| StackTraceError::Json { line, source })?,
    );
    let mut trace = parse_solc_ssa_trace(&input, &qualified_owner)
        .map_err(|source| StackTraceError::Ssa { line, source })?;
    let observation = trace
        .observations
        .pop()
        .expect("one SSA record produces one observation");
    Ok(StackTraceObservation {
        record_kind: "ssa_observation".to_string(),
        schema: SOLC_SSA_OBSERVATION_SCHEMA.to_string(),
        stage_kind: observation.stage_kind,
        stage: observation.stage,
        level: YUL_SSA_LEVEL,
        ordinal: observation.ordinal,
        function_graph_id: observation.function_graph_id,
        function: observation.function,
        layout_iteration: None,
        duration_us: observation.duration_us,
        metrics: observation.metrics,
        unit: observation.unit,
    })
}

fn unit_for_record(record: &str) -> &'static str {
    match record {
        "shuffle_observation" => "solc-shuffle",
        "stack_target_observation" => "solc-stack-target",
        "stack_in_candidate" => "solc-stack-in-candidate",
        "stack_layout_iteration" => "solc-layout-iteration",
        "spill_closure_observation" => "solc-spill-closure",
        "shuffle_playback_observation" => "solc-shuffle-playback",
        "compiler_result" => "solc-compiler-result",
        "stack_in_tradeoff" => "solc-stack-in-tradeoff",
        _ => unreachable!("record schema was validated"),
    }
}

fn event_local(object: &Map<String, Value>, record: &str, graph: u64, ordinal: u64) -> String {
    let object_path = object
        .get("object")
        .and_then(Value::as_str)
        .unwrap_or("object");
    let iteration = object
        .get("layout_iteration")
        .or_else(|| object.get("iteration"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let block = object
        .get("block")
        .and_then(Value::as_u64)
        .or_else(|| find_u64(object, &["site", "block"]))
        .unwrap_or(0);
    let candidate = object.get("candidate").and_then(Value::as_u64).unwrap_or(0);
    format!("{object_path}/g{graph}/{record}/i{iteration}/b{block}/o{ordinal}/c{candidate}")
}

fn extract_metrics(record: &str, object: &Map<String, Value>) -> BTreeMap<String, u64> {
    let mut metrics = BTreeMap::new();
    let mut insert = |name: &str, value: Option<u64>| {
        if let Some(value) = value {
            metrics.insert(name.to_string(), value);
        }
    };
    match record {
        "shuffle_observation" => {
            insert(
                "shuffle_operations",
                find_u64(object, &["result", "operations"]),
            );
            insert("shuffle_gas", find_u64(object, &["result", "gas"]));
            insert("spills", array_len(object, &["result", "spills"]));
            insert(
                "spill_decisions",
                array_len(object, &["result", "spill_decisions"]),
            );
        }
        "stack_target_observation" => {
            insert("max_stack_in", array_len(object, &["target"]));
        }
        "stack_in_candidate" => {
            insert("spills", find_u64(object, &["local_cost", "spills"]));
            insert("shuffle_gas", find_u64(object, &["local_cost", "gas"]));
            insert(
                "max_stack_in",
                find_u64(object, &["local_cost", "stack_size"]),
            );
            if let Some(Value::Object(whole)) = object.get("whole_function_metrics") {
                for (name, value) in whole {
                    if let Some(value) = value.as_u64() {
                        metrics.insert(name.clone(), value);
                    }
                }
            }
        }
        "stack_layout_iteration" => {
            insert("spills", array_len(object, &["spills", "after_closure"]));
            for name in ["shuffles", "targets", "stack_in_choices", "spill_closures"] {
                insert(name, find_u64(object, &["events", name]));
            }
        }
        "spill_closure_observation" => {
            insert(
                "shuffle_operations",
                find_u64(object, &["result", "operations"]),
            );
            insert("shuffle_gas", find_u64(object, &["result", "gas"]));
            insert("spills", array_len(object, &["result", "spills_after"]));
            insert(
                "spill_decisions",
                array_len(object, &["result", "spill_decisions"]),
            );
        }
        "shuffle_playback_observation" => {
            insert("shuffle_operations", find_u64(object, &["operations"]));
            insert("shuffle_gas", find_u64(object, &["gas"]));
        }
        "compiler_result" => {
            insert(
                "bytecode_size",
                find_u64(object, &["output", "bytecode_size"]),
            );
            insert(
                "policy_decisions",
                find_u64(object, &["policy", "decisions"]),
            );
            insert(
                "applied_policy_decisions",
                find_u64(object, &["policy", "applied_decisions"]),
            );
            insert(
                "selected_bytecode_size",
                find_u64(object, &["selected_output", "bytecode_size"]),
            );
            insert(
                "selected_applied_policy_decisions",
                find_u64(object, &["policy", "selected_applied_decisions"]),
            );
        }
        "stack_in_tradeoff" => {
            for name in [
                "candidate_count",
                "gas_saving",
                "gas_penalty",
                "spills_saved",
                "spills_added",
                "stack_slots_saved",
                "stack_slots_added",
            ] {
                insert(name, find_u64(object, &["profile", name]));
            }
        }
        _ => {}
    }
    metrics
}

struct EventGraphBuilder<'a> {
    graph: Graph,
    owner: &'a str,
    local: &'a str,
    next_node: u64,
    slots: BTreeMap<String, NodeKey>,
}

impl<'a> EventGraphBuilder<'a> {
    fn new(graph_key: GraphKey, owner: &'a str, local: &'a str) -> Self {
        Self {
            graph: Graph::new(graph_key),
            owner,
            local,
            next_node: 0,
            slots: BTreeMap::new(),
        }
    }

    fn lower_event(
        &mut self,
        object: &Map<String, Value>,
        record: &str,
        schema: &str,
    ) -> Result<(), riff_catalog_core::CatalogError> {
        let root = self.add_node(&format!("solc.stack.{record}"))?;
        self.graph
            .add_field(&root, Dimension::Structure, "schema", schema)?;
        for (key, value) in object {
            if matches!(key.as_str(), "record" | "schema" | "duration_us") {
                continue;
            }
            self.lower_member(&root, key, value, key)?;
        }
        Ok(())
    }

    fn lower_member(
        &mut self,
        parent: &NodeKey,
        key: &str,
        value: &Value,
        path: &str,
    ) -> Result<(), riff_catalog_core::CatalogError> {
        match value {
            Value::Null => Ok(()),
            Value::Object(object) => {
                let node = self.add_node(&format!("solc.stack.{key}"))?;
                self.graph.add_child(parent, key, 0, &node)?;
                for (child_key, child_value) in object {
                    if child_key == "duration_us" {
                        continue;
                    }
                    self.lower_member(
                        &node,
                        child_key,
                        child_value,
                        &format!("{path}.{child_key}"),
                    )?;
                }
                Ok(())
            }
            Value::Array(values) => self.lower_array(parent, key, values, path),
            Value::String(slot) if is_slot_scalar(key) => {
                self.add_slot_occurrence(parent, key, 0, slot, path)
            }
            Value::String(text) => {
                self.graph
                    .add_field(parent, string_dimension(key), key, text.as_str())
            }
            Value::Bool(value) => self
                .graph
                .add_field(parent, Dimension::Structure, key, *value),
            Value::Number(number) => {
                let value = number.as_u64().unwrap_or(0);
                self.graph
                    .add_field(parent, number_dimension(key), key, value)
            }
        }
    }

    fn lower_array(
        &mut self,
        parent: &NodeKey,
        key: &str,
        values: &[Value],
        path: &str,
    ) -> Result<(), riff_catalog_core::CatalogError> {
        let node = self.add_node(&format!("solc.stack.{key}"))?;
        self.graph.add_child(parent, key, 0, &node)?;
        if is_slot_array(key) && values.iter().all(Value::is_string) {
            for (index, value) in values.iter().enumerate() {
                self.add_slot_occurrence(
                    &node,
                    "slot",
                    index as u32,
                    value.as_str().expect("checked string"),
                    &format!("{path}[{index}]"),
                )?;
            }
            return Ok(());
        }
        for (index, value) in values.iter().enumerate() {
            match value {
                Value::Object(object) => {
                    let kind = if key == "trace" {
                        "solc.stack.trace_event"
                    } else {
                        "solc.stack.item"
                    };
                    let item = self.add_node(kind)?;
                    self.graph.add_child(&node, "item", index as u32, &item)?;
                    for (child_key, child_value) in object {
                        self.lower_member(
                            &item,
                            child_key,
                            child_value,
                            &format!("{path}[{index}].{child_key}"),
                        )?;
                    }
                }
                Value::String(slot) if is_slot_array(key) => {
                    self.add_slot_occurrence(
                        &node,
                        "slot",
                        index as u32,
                        slot,
                        &format!("{path}[{index}]"),
                    )?;
                }
                scalar => {
                    let item = self.add_node("solc.stack.item")?;
                    self.graph.add_child(&node, "item", index as u32, &item)?;
                    self.lower_member(&item, "value", scalar, &format!("{path}[{index}]"))?;
                }
            }
        }
        Ok(())
    }

    fn add_slot_occurrence(
        &mut self,
        parent: &NodeKey,
        label: &str,
        ordinal: u32,
        slot: &str,
        path: &str,
    ) -> Result<(), riff_catalog_core::CatalogError> {
        let occurrence = self.add_node("solc.stack.slot")?;
        self.graph.add_child(parent, label, ordinal, &occurrence)?;
        self.graph
            .add_field(&occurrence, Dimension::Constants, "position", path)?;
        let symbol = if let Some(symbol) = self.slots.get(slot) {
            symbol.clone()
        } else {
            let kind = if slot == "junk" {
                "solc.stack.symbol.junk"
            } else if slot.starts_with("literal:") {
                "solc.stack.symbol.literal"
            } else if slot.starts_with("call-return") {
                "solc.stack.symbol.call_return"
            } else if slot.starts_with("function-return") {
                "solc.stack.symbol.function_return"
            } else {
                "solc.stack.symbol.value"
            };
            let symbol = self.add_node(kind)?;
            self.graph.add_field(
                &symbol,
                Dimension::Constants,
                "alpha_id",
                self.slots.len() as u64,
            )?;
            if let Some(literal) = slot.strip_prefix("literal:") {
                self.graph
                    .add_field(&symbol, Dimension::Constants, "value", literal)?;
            }
            self.slots.insert(slot.to_string(), symbol.clone());
            symbol
        };
        self.graph
            .add_edge(&occurrence, "value", &symbol, EdgeRole::Data)
    }

    fn add_node(&mut self, kind: &str) -> Result<NodeKey, riff_catalog_core::CatalogError> {
        let local = format!("{}/n{}", self.local, self.next_node);
        self.next_node += 1;
        let key = NodeKey::entity(EntityKey::new("solc.stack.node", self.owner, local)?);
        self.graph.add_node(key.clone(), kind)?;
        Ok(key)
    }
}

fn is_slot_array(key: &str) -> bool {
    matches!(
        key,
        "source"
            | "target"
            | "spills"
            | "stack"
            | "result"
            | "arguments"
            | "live_out"
            | "live_in"
            | "layout"
            | "spills_before"
            | "spills_after"
            | "at_start"
            | "after_layout"
            | "after_closure"
    )
}

fn is_slot_scalar(key: &str) -> bool {
    matches!(key, "slot" | "owner" | "selected")
}

fn string_dimension(key: &str) -> Dimension {
    match key {
        "function" | "object" => Dimension::Names,
        "bytecode_hash" => Dimension::Constants,
        "op" => Dimension::TraceEvents,
        _ => Dimension::Structure,
    }
}

fn number_dimension(key: &str) -> Dimension {
    match key {
        "function_graph_id" | "ordinal" | "layout_iteration" | "iteration" | "block"
        | "instruction" | "target_block" | "candidate" => Dimension::Names,
        _ => Dimension::Constants,
    }
}

fn find_u64(object: &Map<String, Value>, path: &[&str]) -> Option<u64> {
    let (last, parents) = path.split_last()?;
    let mut current = object;
    for key in parents {
        current = current.get(*key)?.as_object()?;
    }
    current.get(*last)?.as_u64()
}

fn array_len(object: &Map<String, Value>, path: &[&str]) -> Option<u64> {
    let (last, parents) = path.split_last()?;
    let mut current = object;
    for key in parents {
        current = current.get(*key)?.as_object()?;
    }
    current
        .get(*last)?
        .as_array()
        .map(|values| values.len() as u64)
}

fn optional_string(
    object: &Map<String, Value>,
    field: &str,
    line: usize,
) -> Result<Option<String>, StackTraceError> {
    match object.get(field) {
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(Value::Null) => Ok(None),
        Some(_) => Err(shape(
            line,
            format!("field `{field}` must be a string or null"),
        )),
        None => Err(shape(line, format!("missing field `{field}`"))),
    }
}

fn optional_metadata_string(
    object: &Map<String, Value>,
    field: &str,
    line: usize,
) -> Result<Option<String>, StackTraceError> {
    match object.get(field) {
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(Value::Null) | None => Ok(None),
        Some(_) => Err(shape(
            line,
            format!("field `{field}` must be a string or null"),
        )),
    }
}

fn required_str<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    line: usize,
) -> Result<&'a str, StackTraceError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| shape(line, format!("missing or non-string field `{field}`")))
}

fn required_u64(
    object: &Map<String, Value>,
    field: &str,
    line: usize,
) -> Result<u64, StackTraceError> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| shape(line, format!("missing or non-integer field `{field}`")))
}

fn shape(line: usize, message: impl Into<String>) -> StackTraceError {
    StackTraceError::Shape {
        line,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use riff_catalog_core::{
        CyclePolicy, DigestRequest, Dimension, HashPolicy, ViewMode, digest_graph,
    };
    use serde_json::json;

    use super::*;

    fn stream(event: Value) -> String {
        [
            json!({
                "record": "metadata",
                "schema": SOLC_STACK_EVENT_STREAM_SCHEMA,
                "source": "fixture.yul",
                "object": "Fixture"
            }),
            event,
        ]
        .into_iter()
        .map(|record| serde_json::to_string(&record).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
    }

    fn shuffle(source: &[&str], target: &[&str], trace: Value) -> Value {
        json!({
            "record": "shuffle_observation",
            "schema": SHUFFLE_SCHEMA,
            "function_graph_id": 1,
            "function": "f",
            "ordinal": 0,
            "layout_iteration": 0,
            "stable_iteration": true,
            "site": {"kind": "operation", "block": 7, "instruction": 9, "tentative": false},
            "problem": {
                "orientation": "bottom-to-top",
                "source": source,
                "target": target,
                "spills": [],
                "reachable_depth": 16,
                "spilling_allowed": true
            },
            "result": {
                "status": "admissible",
                "stack": target,
                "spills": [],
                "spill_decisions": [],
                "trace": trace,
                "operations": 1,
                "gas": 3,
                "duration_us": 10
            }
        })
    }

    fn structure_digest(observation: &StackTraceObservation) -> riff_catalog_core::Digest {
        let policy = HashPolicy::new(
            SOLC_STACK_EVENT_LEVEL,
            ViewMode::AnonymousShape,
            CyclePolicy::CondenseScc,
        )
        .unwrap();
        *digest_graph(
            &DigestRequest::all_dimensions(observation.unit.graph_key.clone(), policy),
            &observation.unit.graph,
        )
        .unwrap()
        .hashes
        .graph
        .get(Dimension::Structure)
        .unwrap()
    }

    fn structure_constants_digest(
        observation: &StackTraceObservation,
    ) -> (riff_catalog_core::Digest, riff_catalog_core::Digest) {
        let policy = HashPolicy::new(
            SOLC_STACK_EVENT_LEVEL,
            ViewMode::AnonymousShape,
            CyclePolicy::CondenseScc,
        )
        .unwrap();
        let result = digest_graph(
            &DigestRequest::all_dimensions(observation.unit.graph_key.clone(), policy),
            &observation.unit.graph,
        )
        .unwrap();
        (
            *result.hashes.graph.get(Dimension::Structure).unwrap(),
            *result.hashes.graph.get(Dimension::Constants).unwrap(),
        )
    }

    #[test]
    fn slot_numbers_do_not_leak_into_shape() {
        let first = parse_solc_stack_trace(
            &stream(shuffle(
                &["v0", "v1", "v0"],
                &["v1", "v0", "v0"],
                json!([{"op": "swap", "depth": 1}]),
            )),
            "trace:fixture",
        )
        .unwrap();
        let renamed = parse_solc_stack_trace(
            &stream(shuffle(
                &["v8", "v3", "v8"],
                &["v3", "v8", "v8"],
                json!([{"op": "swap", "depth": 1}]),
            )),
            "trace:fixture",
        )
        .unwrap();
        assert_eq!(
            structure_digest(&first.observations[0]),
            structure_digest(&renamed.observations[0])
        );
    }

    #[test]
    fn alpha_normalized_slot_relations_are_permutation_sensitive() {
        let trace = json!([{"op": "swap", "depth": 1}]);
        let first = parse_solc_stack_trace(
            &stream(shuffle(
                &["v0", "v1", "v2"],
                &["v1", "v0", "v2"],
                trace.clone(),
            )),
            "trace:fixture",
        )
        .unwrap();
        let renamed = parse_solc_stack_trace(
            &stream(shuffle(
                &["v8", "v3", "v9"],
                &["v3", "v8", "v9"],
                trace.clone(),
            )),
            "trace:fixture",
        )
        .unwrap();
        let different = parse_solc_stack_trace(
            &stream(shuffle(&["v0", "v1", "v2"], &["v0", "v2", "v1"], trace)),
            "trace:fixture",
        )
        .unwrap();

        assert_eq!(
            structure_constants_digest(&first.observations[0]),
            structure_constants_digest(&renamed.observations[0])
        );
        assert_ne!(
            structure_constants_digest(&first.observations[0]),
            structure_constants_digest(&different.observations[0])
        );
    }

    #[test]
    fn extracts_costs_without_hashing_duration() {
        let trace = parse_solc_stack_trace(
            &stream(shuffle(
                &["v0", "v1"],
                &["v1", "v0"],
                json!([{"op": "swap", "depth": 1}]),
            )),
            "trace:fixture",
        )
        .unwrap();
        let observation = &trace.observations[0];
        assert_eq!(observation.metrics["shuffle_operations"], 1);
        assert_eq!(observation.metrics["shuffle_gas"], 3);
        assert!(
            observation
                .unit
                .graph
                .nodes
                .values()
                .flat_map(|node| &node.fields)
                .all(|field| field.name.as_str() != "duration_us")
        );
    }

    #[test]
    fn compiler_stream_keeps_ssa_snapshots_and_stack_events() {
        let records = [
            json!({
                "record": "metadata",
                "schema": SOLC_COMPILER_EVENT_STREAM_SCHEMA,
                "source": "fixture.yul",
                "object": "Fixture"
            }),
            json!({
                "record": "ssa_observation",
                "schema": SOLC_SSA_OBSERVATION_SCHEMA,
                "object": "Fixture.Runtime",
                "stage_kind": "transform",
                "stage": "input",
                "ordinal": 0,
                "function_graph_id": 1,
                "function": "f",
                "duration_us": 3,
                "metrics": {"max_live_in": 2},
                "graph": {
                    "type": "Function",
                    "name": "f",
                    "arguments": ["v0"],
                    "numReturns": 0,
                    "entry": "Block0",
                    "blocks": [{
                        "id": "Block0",
                        "instructions": [],
                        "exit": {"type": "Terminated"}
                    }]
                }
            }),
            json!({
                "record": "shuffle_playback_observation",
                "schema": PLAYBACK_SCHEMA,
                "object": "Fixture.Runtime",
                "function_graph_id": 1,
                "function": "f",
                "ordinal": 0,
                "site": {"kind": "operation", "block": 0, "instruction": 0, "target_block": null},
                "source": ["v0", "v1"],
                "result": ["v1", "v0"],
                "trace": [{"op": "swap", "depth": 1}],
                "operations": 1,
                "gas": 3
            }),
        ]
        .into_iter()
        .map(|record| serde_json::to_string(&record).unwrap())
        .collect::<Vec<_>>()
        .join("\n");

        let trace = parse_solc_stack_trace(&records, "trace:fixture").unwrap();
        assert_eq!(trace.observations.len(), 2);
        assert_eq!(trace.observations[0].level, YUL_SSA_LEVEL);
        assert_eq!(trace.observations[0].stage, "input");
        assert_eq!(trace.observations[1].level, SOLC_STACK_EVENT_LEVEL);
        assert_eq!(trace.observations[1].metrics["shuffle_gas"], 3);
        assert!(
            trace.observations[1]
                .unit
                .graph_key
                .canonical_key()
                .contains("Fixture.Runtime")
        );
    }

    #[test]
    fn compiler_result_keeps_selected_output_metrics() {
        let records = [
            json!({
                "record": "metadata",
                "schema": SOLC_COMPILER_EVENT_STREAM_SCHEMA,
                "source": "fixture.yul",
                "object": "Fixture"
            }),
            json!({
                "record": "compiler_result",
                "schema": COMPILER_RESULT_SCHEMA,
                "object": "Fixture",
                "function_graph_id": 0,
                "function": null,
                "ordinal": 0,
                "output": {"bytecode_size": 11, "bytecode_hash": "aa"},
                "policy": {
                    "decisions": 1,
                    "applied_decisions": 1,
                    "selected_applied_decisions": 1
                },
                "selected_output": {
                    "object": "Fixture.Runtime",
                    "bytecode_size": 7,
                    "bytecode_hash": "bb"
                }
            }),
        ]
        .into_iter()
        .map(|record| serde_json::to_string(&record).unwrap())
        .collect::<Vec<_>>()
        .join("\n");

        let trace = parse_solc_stack_trace(&records, "trace:fixture").unwrap();
        assert_eq!(trace.observations.len(), 1);
        assert_eq!(trace.observations[0].metrics["bytecode_size"], 11);
        assert_eq!(trace.observations[0].metrics["selected_bytecode_size"], 7);
        assert_eq!(
            trace.observations[0].metrics["selected_applied_policy_decisions"],
            1
        );
    }

    #[test]
    fn selected_output_address_ignores_policy_provenance() {
        fn compiler_stream(decisions: u64, applied: u64) -> String {
            [
                json!({
                    "record": "metadata",
                    "schema": SOLC_COMPILER_EVENT_STREAM_SCHEMA,
                    "source": "fixture.yul",
                    "object": "Fixture"
                }),
                json!({
                    "record": "compiler_result",
                    "schema": COMPILER_RESULT_SCHEMA,
                    "object": "Fixture",
                    "function_graph_id": 0,
                    "function": null,
                    "ordinal": 0,
                    "output": {"bytecode_size": 11, "bytecode_hash": "aa"},
                    "policy": {
                        "decisions": decisions,
                        "applied_decisions": applied,
                        "selected_applied_decisions": applied
                    },
                    "selected_output": {
                        "object": "Fixture.Runtime",
                        "bytecode_size": 7,
                        "bytecode_hash": "bb"
                    }
                }),
            ]
            .into_iter()
            .map(|record| serde_json::to_string(&record).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
        }

        fn selected_output_tree(input: &str) -> riff_catalog_core::DimensionDigests {
            let trace = parse_solc_stack_trace(input, "trace:fixture").unwrap();
            let observation = &trace.observations[0];
            let policy = HashPolicy::new(
                SOLC_STACK_EVENT_LEVEL,
                ViewMode::AnonymousShape,
                CyclePolicy::CondenseScc,
            )
            .unwrap();
            let result = digest_graph(
                &DigestRequest::all_dimensions(observation.unit.graph_key.clone(), policy),
                &observation.unit.graph,
            )
            .unwrap();
            let selected = observation
                .unit
                .graph
                .nodes
                .iter()
                .find(|(_, node)| node.kind.as_str() == "solc.stack.selected_output")
                .map(|(key, _)| key)
                .unwrap();
            result.hashes.nodes[selected].tree.clone()
        }

        assert_eq!(
            selected_output_tree(&compiler_stream(0, 0)),
            selected_output_tree(&compiler_stream(9, 4))
        );
    }

    #[test]
    fn normalizes_stack_in_tradeoffs_across_absolute_costs_and_selection() {
        fn candidate_stream(default_gas: u64, selected_alternative: bool) -> String {
            [
                json!({
                    "record": "metadata",
                    "schema": SOLC_COMPILER_EVENT_STREAM_SCHEMA,
                    "source": "fixture.yul",
                    "object": "Fixture"
                }),
                json!({
                    "record": "stack_in_candidate",
                    "schema": STACK_IN_SCHEMA,
                    "object": "Fixture.Runtime",
                    "function_graph_id": 1,
                    "function": "f",
                    "block": 7,
                    "layout_iteration": 0,
                    "candidate": 0,
                    "default": true,
                    "selected": !selected_alternative,
                    "stable_iteration": true,
                    "local_cost": {"gas": default_gas, "spills": 0, "stack_size": 6}
                }),
                json!({
                    "record": "stack_in_candidate",
                    "schema": STACK_IN_SCHEMA,
                    "object": "Fixture.Runtime",
                    "function_graph_id": 1,
                    "function": "f",
                    "block": 7,
                    "layout_iteration": 0,
                    "candidate": 1,
                    "default": false,
                    "selected": selected_alternative,
                    "stable_iteration": true,
                    "local_cost": {"gas": default_gas + 2, "spills": 0, "stack_size": 5}
                }),
            ]
            .into_iter()
            .map(|record| serde_json::to_string(&record).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
        }

        let low = parse_solc_stack_trace(&candidate_stream(3, false), "trace:low").unwrap();
        let high = parse_solc_stack_trace(&candidate_stream(8, true), "trace:high").unwrap();
        let alternatives = [&low, &high]
            .into_iter()
            .map(|trace| {
                trace
                    .observations
                    .iter()
                    .find(|observation| {
                        observation.record_kind == "stack_in_tradeoff"
                            && observation.metrics["gas_penalty"] == 2
                    })
                    .unwrap()
            })
            .collect::<Vec<_>>();

        for alternative in &alternatives {
            assert_eq!(alternative.metrics["candidate_count"], 2);
            assert_eq!(alternative.metrics["stack_slots_saved"], 1);
            assert_eq!(alternative.metrics["spills_added"], 0);
        }
        let profile_fields = |observation: &StackTraceObservation| {
            observation
                .unit
                .graph
                .nodes
                .values()
                .find(|node| node.kind.as_str() == "solc.stack.profile")
                .unwrap()
                .fields
                .clone()
        };
        assert_eq!(
            profile_fields(alternatives[0]),
            profile_fields(alternatives[1])
        );
    }
}
