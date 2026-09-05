//! Export the exact pure-SWAP subset of solc shuffle observations.

use std::collections::BTreeSet;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use riff_catalog_core::{
    CyclePolicy, DigestRequest, Dimension, EntityKey, Facet, Graph, GraphKey, HashPolicy, NodeKey,
    ViewMode, digest_graph,
};
use riff_catalog_view::ViewPlan;
use riff_catalog_yul::stack_trace::parse_solc_stack_trace;
use serde_json::{Map, Value, json};

const ORACLE_INPUT_SCHEMA: &str = "riffcat-shuffle-oracle-input/1";
const SHUFFLE_PROBLEM_VIEW: &str = r#"
language "riffcat-view/1"
view "solc.shuffle-problem/2"
input "solc-stack-event/1"

root node-kind "solc.shuffle.problem"
traverse children

retain structure, constants, types
"#;

pub struct StackOracleExportArgs {
    pub path: PathBuf,
    pub max_size: usize,
    pub owner: Option<String>,
}

pub fn run(args: &StackOracleExportArgs) -> Result<()> {
    if args.max_size > 16 {
        bail!("pure-SWAP oracle stacks cannot exceed EVM reachability depth 16");
    }
    let input = std::fs::read_to_string(&args.path)
        .with_context(|| format!("reading stack trace {}", args.path.display()))?;
    let owner = args
        .owner
        .clone()
        .unwrap_or_else(|| default_owner(&args.path));
    let cases = extract_cases(&input, &owner, args.max_size)?;
    let stdout = std::io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    for case in &cases {
        serde_json::to_writer(&mut output, case)?;
        output.write_all(b"\n")?;
    }
    output.flush()?;
    eprintln!("exported {} pure-SWAP oracle cases", cases.len());
    Ok(())
}

fn extract_cases(input: &str, owner: &str, max_size: usize) -> Result<Vec<Value>> {
    let raw_events = input
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|value| value.get("record").and_then(Value::as_str) != Some("metadata"))
        .collect::<Vec<_>>();
    let trace = parse_solc_stack_trace(input, owner)?;
    if trace.observations.len() < raw_events.len() {
        bail!("lowered trace lost raw observations");
    }
    let plan = ViewPlan::parse(SHUFFLE_PROBLEM_VIEW)?;
    let mut cases = Vec::new();

    for (raw, observation) in raw_events.iter().zip(&trace.observations) {
        let Some(event) = raw.as_object() else {
            continue;
        };
        if event.get("record").and_then(Value::as_str) != Some("shuffle_observation") {
            continue;
        }
        let Some((source, target, depths)) = pure_swap_problem(event, max_size) else {
            continue;
        };
        let target_indices = target
            .iter()
            .map(|slot| {
                source
                    .iter()
                    .position(|candidate| candidate == slot)
                    .expect("sets were checked equal")
            })
            .collect::<Vec<_>>();
        let problem_graph = normalized_problem_graph(
            observation.unit.graph_key.clone(),
            owner,
            cases.len(),
            source.len(),
            &target_indices,
        )?;
        let materialized = plan.materialize(&problem_graph)?;
        let policy = HashPolicy::new(
            plan.output_level(),
            ViewMode::AnonymousShape,
            CyclePolicy::CondenseScc,
        )?;
        let request = DigestRequest::new(
            materialized.graph_key.clone(),
            policy,
            plan.dimensions.clone(),
        )?;
        let result = digest_graph(&request, &materialized)?;
        let facet = Facet::new(result.hashes.policy_id, plan.dimensions.clone())?;
        let problem_address = result.hashes.facet_address(&facet)?.address_digest();
        cases.push(json!({
            "schema": ORACLE_INPUT_SCHEMA,
            "problem_address": problem_address.to_hex(),
            "problem_view": plan.name.as_str(),
            "problem_facet": plan.dimensions.iter().map(|dimension| dimension.as_str()).collect::<Vec<_>>(),
            "size": source.len(),
            "source": (0..source.len()).collect::<Vec<_>>(),
            "target": target_indices,
            "solc": {
                "swaps": depths.len(),
                "witness_depths": depths,
            },
            "origin": {
                "object": event
                    .get("object")
                    .cloned()
                    .unwrap_or_else(|| Value::String(trace.metadata.object.clone())),
                "function_graph_id": event.get("function_graph_id").cloned().unwrap_or(Value::Null),
                "function": event.get("function").cloned().unwrap_or(Value::Null),
                "ordinal": event.get("ordinal").cloned().unwrap_or(Value::Null),
                "layout_iteration": event.get("layout_iteration").cloned().unwrap_or(Value::Null),
                "site": event.get("site").cloned().unwrap_or(Value::Null),
                "graph_key": observation.unit.graph_key.canonical_key(),
            }
        }));
    }

    Ok(cases)
}

fn normalized_problem_graph(
    graph_key: GraphKey,
    owner: &str,
    ordinal: usize,
    size: usize,
    target: &[usize],
) -> Result<Graph> {
    let node = |local: &str| -> Result<NodeKey> {
        Ok(NodeKey::entity(EntityKey::new(
            "solc.shuffle.problem.node",
            owner,
            format!("case-{ordinal}/{local}"),
        )?))
    };
    let mut graph = Graph::new(graph_key);
    let root = node("root")?;
    graph.add_node(root.clone(), "solc.shuffle.problem")?;
    graph.add_field(&root, Dimension::Structure, "operation_set", "evm-swap-top")?;
    graph.add_field(&root, Dimension::Structure, "orientation", "bottom-to-top")?;
    graph.add_field(&root, Dimension::Constants, "size", size as u64)?;
    graph.add_field(&root, Dimension::Constants, "reachable_depth", 16_u64)?;

    let source = node("source")?;
    graph.add_node(source.clone(), "solc.shuffle.source")?;
    graph.add_child(&root, "source", 0, &source)?;
    let target_node = node("target")?;
    graph.add_node(target_node.clone(), "solc.shuffle.target")?;
    graph.add_child(&root, "target", 0, &target_node)?;

    for index in 0..size {
        let slot = node(&format!("source/{index}"))?;
        graph.add_node(slot.clone(), "solc.shuffle.slot")?;
        graph.add_field(&slot, Dimension::Constants, "value", index as u64)?;
        graph.add_child(&source, "slot", index as u32, &slot)?;
    }
    for (index, value) in target.iter().copied().enumerate() {
        let slot = node(&format!("target/{index}"))?;
        graph.add_node(slot.clone(), "solc.shuffle.slot")?;
        graph.add_field(&slot, Dimension::Constants, "value", value as u64)?;
        graph.add_child(&target_node, "slot", index as u32, &slot)?;
    }
    graph.validate()?;
    Ok(graph)
}

fn pure_swap_problem(
    event: &Map<String, Value>,
    max_size: usize,
) -> Option<(Vec<String>, Vec<String>, Vec<u64>)> {
    let problem = event.get("problem")?.as_object()?;
    let result = event.get("result")?.as_object()?;
    if result.get("status")?.as_str()? != "admissible"
        || !result.get("spills")?.as_array()?.is_empty()
        || !problem.get("spills")?.as_array()?.is_empty()
    {
        return None;
    }
    let source = strings(problem.get("source")?)?;
    let target = strings(problem.get("target")?)?;
    if source.len() != target.len()
        || source.len() > max_size
        || source.iter().any(|slot| slot == "junk")
        || target.iter().any(|slot| slot == "junk")
    {
        return None;
    }
    let source_set = source.iter().collect::<BTreeSet<_>>();
    let target_set = target.iter().collect::<BTreeSet<_>>();
    if source_set.len() != source.len() || source_set != target_set {
        return None;
    }
    let trace = result.get("trace")?.as_array()?;
    let mut depths = Vec::with_capacity(trace.len());
    for operation in trace {
        let operation = operation.as_object()?;
        if operation.get("op")?.as_str()? != "swap" {
            return None;
        }
        let depth = operation.get("depth")?.as_u64()?;
        if depth == 0 || depth >= source.len() as u64 || depth > 16 {
            return None;
        }
        depths.push(depth);
    }
    if strings(result.get("stack")?)? != target {
        return None;
    }
    Some((source, target, depths))
}

fn strings(value: &Value) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|value| value.as_str().map(str::to_owned))
        .collect()
}

fn default_owner(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("trace");
    format!("shuffle-oracle:{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(source: &[&str], target: &[&str], depths: &[u64]) -> String {
        let stack = depths.iter().fold(
            source
                .iter()
                .map(|slot| slot.to_string())
                .collect::<Vec<_>>(),
            |mut stack, depth| {
                let top = stack.len() - 1;
                stack.swap(top, top - *depth as usize);
                stack
            },
        );
        [
            json!({
                "record": "metadata",
                "schema": "solc-stack-layout-event-stream/1",
                "source": "fixture.yul",
                "object": "Fixture"
            }),
            json!({
                "record": "shuffle_observation",
                "schema": "solc-shuffle-observation/2",
                "function_graph_id": 1,
                "function": "f",
                "ordinal": 4,
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
                    "stack": stack,
                    "spills": [],
                    "spill_decisions": [],
                    "trace": depths.iter().map(|depth| json!({"op": "swap", "depth": depth})).collect::<Vec<_>>(),
                    "operations": depths.len(),
                    "gas": depths.len() * 3,
                    "duration_us": 1
                }
            }),
        ]
        .into_iter()
        .map(|record| serde_json::to_string(&record).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
    }

    #[test]
    fn exports_addressed_distinct_permutation() {
        let cases = extract_cases(
            &stream(&["a", "b", "c"], &["b", "a", "c"], &[2, 1, 2]),
            "trace:test",
            16,
        )
        .unwrap();
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0]["schema"], ORACLE_INPUT_SCHEMA);
        assert_eq!(cases[0]["target"], json!([1, 0, 2]));
        assert_eq!(cases[0]["solc"]["witness_depths"], json!([2, 1, 2]));
        assert_eq!(cases[0]["problem_address"].as_str().unwrap().len(), 64);
    }

    #[test]
    fn rejects_junk_and_duplicate_slots() {
        assert!(
            extract_cases(
                &stream(&["a", "junk"], &["junk", "a"], &[1]),
                "trace:test",
                16,
            )
            .unwrap()
            .is_empty()
        );
        assert!(
            extract_cases(&stream(&["a", "a"], &["a", "a"], &[]), "trace:test", 16,)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn address_is_alpha_invariant_and_permutation_sensitive() {
        let left = extract_cases(
            &stream(&["a", "b", "c"], &["b", "a", "c"], &[2, 1, 2]),
            "trace:left",
            16,
        )
        .unwrap();
        let renamed = extract_cases(
            &stream(&["x", "y", "z"], &["y", "x", "z"], &[2, 1, 2]),
            "trace:renamed",
            16,
        )
        .unwrap();
        let different = extract_cases(
            &stream(&["a", "b", "c"], &["a", "b", "c"], &[]),
            "trace:different",
            16,
        )
        .unwrap();

        assert_eq!(left[0]["problem_address"], renamed[0]["problem_address"]);
        assert_ne!(left[0]["problem_address"], different[0]["problem_address"]);
    }
}
