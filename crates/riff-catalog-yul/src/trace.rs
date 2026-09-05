//! Reader for the versioned JSONL observations emitted by solc's SSA tracer.
//!
//! The trace schema is a transport contract. Each graph snapshot is lowered
//! independently and receives normal riff-catalog addresses under
//! [`crate::ssa::YUL_SSA_LEVEL`].

use std::collections::BTreeMap;

use serde_json::{Value, json};
use thiserror::Error;

use crate::error::YulLowerError;
use crate::lower::LoweredUnit;
use crate::ssa::lower_yul_cfg;

pub const SOLC_SSA_OBSERVATION_SCHEMA: &str = "solc-ssa-observation/1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceMetadata {
    pub source: String,
    pub object: String,
}

#[derive(Clone, Debug)]
pub struct TraceObservation {
    pub stage_kind: String,
    pub stage: String,
    pub ordinal: u64,
    pub function_graph_id: u64,
    pub function: Option<String>,
    pub duration_us: u64,
    pub metrics: BTreeMap<String, u64>,
    pub unit: LoweredUnit,
}

#[derive(Clone, Debug)]
pub struct SolcSsaTrace {
    pub metadata: TraceMetadata,
    pub observations: Vec<TraceObservation>,
}

#[derive(Debug, Error)]
pub enum SsaTraceError {
    #[error("SSA trace line {line}: invalid JSON: {source}")]
    Json {
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error("SSA trace line {line}: {message}")]
    Shape { line: usize, message: String },
    #[error("SSA trace line {line}: graph lowering failed: {source}")]
    Lower {
        line: usize,
        #[source]
        source: YulLowerError,
    },
}

/// Parse and lower one complete solc SSA observation stream.
pub fn parse_solc_ssa_trace(input: &str, owner: &str) -> Result<SolcSsaTrace, SsaTraceError> {
    let mut metadata = None;
    let mut pending = Vec::new();

    for (index, line) in input.lines().enumerate() {
        let line_number = index + 1;
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line).map_err(|source| SsaTraceError::Json {
            line: line_number,
            source,
        })?;
        let object = value
            .as_object()
            .ok_or_else(|| shape(line_number, "record is not an object"))?;
        let schema = required_str(object, "schema", line_number)?;
        if schema != SOLC_SSA_OBSERVATION_SCHEMA {
            return Err(shape(
                line_number,
                format!("unsupported schema `{schema}`; expected `{SOLC_SSA_OBSERVATION_SCHEMA}`"),
            ));
        }

        match required_str(object, "record", line_number)? {
            "metadata" => {
                if metadata.is_some() {
                    return Err(shape(line_number, "duplicate metadata record"));
                }
                metadata = Some(TraceMetadata {
                    source: required_str(object, "source", line_number)?.to_string(),
                    object: required_str(object, "object", line_number)?.to_string(),
                });
            }
            "ssa_observation" => pending.push((line_number, value)),
            other => return Err(shape(line_number, format!("unknown record kind `{other}`"))),
        }
    }

    let metadata = metadata.ok_or_else(|| shape(0, "missing metadata record"))?;
    if pending.is_empty() {
        return Err(shape(0, "trace contains no observations"));
    }

    let observations = pending
        .into_iter()
        .map(|(line, value)| lower_observation(value, &metadata, owner, line))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(SolcSsaTrace {
        metadata,
        observations,
    })
}

fn lower_observation(
    value: Value,
    metadata: &TraceMetadata,
    owner: &str,
    line: usize,
) -> Result<TraceObservation, SsaTraceError> {
    let object = value.as_object().expect("validated as object");
    let stage_kind = required_str(object, "stage_kind", line)?.to_string();
    let stage = required_str(object, "stage", line)?.to_string();
    let ordinal = required_u64(object, "ordinal", line)?;
    let function_graph_id = required_u64(object, "function_graph_id", line)?;
    let duration_us = required_u64(object, "duration_us", line)?;
    let function = match object.get("function") {
        Some(Value::String(name)) => Some(name.clone()),
        Some(Value::Null) => None,
        Some(_) => return Err(shape(line, "field `function` must be a string or null")),
        None => return Err(shape(line, "missing field `function`")),
    };
    let metrics = object
        .get("metrics")
        .cloned()
        .ok_or_else(|| shape(line, "missing field `metrics`"))
        .and_then(|metrics| {
            serde_json::from_value(metrics).map_err(|source| SsaTraceError::Json { line, source })
        })?;
    let graph = object
        .get("graph")
        .ok_or_else(|| shape(line, "missing field `graph`"))?;
    let graph_type = graph
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| shape(line, "graph has no string `type`"))?;

    let wrapper = match graph_type {
        "Main" => json!({
            metadata.object.clone(): {
                "blocks": graph.get("blocks").cloned().unwrap_or_else(|| json!([])),
                "entry": graph.get("entry").cloned().unwrap_or(Value::Null),
                "functions": {},
                "subObjects": {}
            },
            "type": "Object"
        }),
        "Function" => {
            let graph_name = graph
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| shape(line, "function graph has no string `name`"))?;
            if function.as_deref() != Some(graph_name) {
                return Err(shape(line, "record `function` does not match graph `name`"));
            }
            json!({
                metadata.object.clone(): {
                    "blocks": [],
                    "functions": { graph_name: graph.clone() },
                    "subObjects": {}
                },
                "type": "Object"
            })
        }
        other => return Err(shape(line, format!("unknown graph type `{other}`"))),
    };

    let lowered =
        lower_yul_cfg(&wrapper, owner).map_err(|source| SsaTraceError::Lower { line, source })?;
    let unit = match graph_type {
        "Main" => lowered
            .objects
            .into_iter()
            .next()
            .expect("lower_yul_cfg always emits the wrapper object"),
        "Function" => lowered
            .functions
            .into_iter()
            .next()
            .expect("wrapper contains one function"),
        _ => unreachable!(),
    };

    Ok(TraceObservation {
        stage_kind,
        stage,
        ordinal,
        function_graph_id,
        function,
        duration_us,
        metrics,
        unit,
    })
}

fn required_str<'a>(
    object: &'a serde_json::Map<String, Value>,
    field: &str,
    line: usize,
) -> Result<&'a str, SsaTraceError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| shape(line, format!("missing or non-string field `{field}`")))
}

fn required_u64(
    object: &serde_json::Map<String, Value>,
    field: &str,
    line: usize,
) -> Result<u64, SsaTraceError> {
    object
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| shape(line, format!("missing or non-integer field `{field}`")))
}

fn shape(line: usize, message: impl Into<String>) -> SsaTraceError {
    SsaTraceError::Shape {
        line,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trace(graph: Value, function: Value) -> String {
        [
            json!({
                "record": "metadata",
                "schema": SOLC_SSA_OBSERVATION_SCHEMA,
                "source": "fixture.yul",
                "object": "Fixture"
            }),
            json!({
                "record": "ssa_observation",
                "schema": SOLC_SSA_OBSERVATION_SCHEMA,
                "stage_kind": "transform",
                "stage": "input",
                "ordinal": 0,
                "function_graph_id": 1,
                "function": function,
                "duration_us": 3,
                "metrics": {"max_live_in": 2},
                "graph": graph
            }),
        ]
        .into_iter()
        .map(|record| serde_json::to_string(&record).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
    }

    #[test]
    fn lowers_function_observation_with_stable_graph_key() {
        let input = trace(
            json!({
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
            }),
            json!("f"),
        );
        let first = parse_solc_ssa_trace(&input, "trace:fixture").unwrap();
        let second = parse_solc_ssa_trace(&input, "trace:fixture").unwrap();
        assert_eq!(first.metadata.object, "Fixture");
        assert_eq!(first.observations[0].unit.unit, "yulssa-fn");
        assert_eq!(first.observations[0].unit.name, "f");
        assert_eq!(
            first.observations[0].unit.graph_key,
            second.observations[0].unit.graph_key
        );
    }

    #[test]
    fn uses_explicit_main_entry() {
        let input = trace(
            json!({
                "type": "Main",
                "name": "",
                "entry": "Block1",
                "blocks": [
                    {"id": "Block0", "instructions": [], "exit": {"type": "Terminated"}},
                    {"id": "Block1", "instructions": [], "exit": {"type": "Terminated"}}
                ]
            }),
            Value::Null,
        );
        let parsed = parse_solc_ssa_trace(&input, "trace:fixture").unwrap();
        let graph = &parsed.observations[0].unit.graph;
        let object = graph
            .nodes
            .values()
            .find(|node| node.kind.as_str() == "yulssa.object")
            .unwrap();
        let entry = graph
            .children
            .iter()
            .find(|edge| edge.parent == object.key && edge.label.as_str() == "entry")
            .unwrap();
        assert!(entry.child.canonical_key().contains("b:Block1"));
    }

    #[test]
    fn rejects_unknown_schema() {
        let input = trace(json!({"type": "Main", "blocks": []}), Value::Null)
            .replace(SOLC_SSA_OBSERVATION_SCHEMA, "solc-ssa-observation/2");
        let error = parse_solc_ssa_trace(&input, "trace:fixture").unwrap_err();
        assert!(error.to_string().contains("unsupported schema"));
    }
}
