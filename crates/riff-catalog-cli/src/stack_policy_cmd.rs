//! Turn a content-addressed stack tradeoff class into a solc replay policy.

use std::collections::BTreeSet;

use anyhow::{Result, bail};
use riff_catalog_core::{Dimension, Graph, Node, Value};
use serde::Serialize;
use serde_json::json;

use crate::corpus::Corpus;
use crate::facet::address_of;

const TRADEOFF_VIEW_UNIT: &str = "view:solc.stack-in-tradeoff/1";
const TRADEOFF_EVENT_UNIT: &str = "solc-stack-in-tradeoff";
const STACK_EVENT_LEVEL: &str = "solc-stack-event/1";

pub struct StackPolicyArgs {
    pub profile: String,
    pub selector: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
struct PolicyDecision {
    object: String,
    function_graph_id: u64,
    block: u64,
    layout_iteration: u64,
    candidate: u64,
}

pub fn run(corpus: &Corpus, args: &StackPolicyArgs) -> Result<()> {
    let dimensions = [Dimension::Structure, Dimension::Constants]
        .into_iter()
        .collect::<BTreeSet<_>>();
    let selector = args.selector.as_deref().unwrap_or_default();
    let mut matches = Vec::new();
    for row in corpus.digest_rows(TRADEOFF_VIEW_UNIT, "shape")? {
        if !selector.is_empty()
            && !row.owner.contains(selector)
            && !row.name.contains(selector)
            && !row.artifact_id.contains(selector)
        {
            continue;
        }
        let address = address_of(&row, &dimensions)?.to_hex();
        if address.starts_with(&args.profile) {
            matches.push((address, row.artifact_id));
        }
    }
    if matches.is_empty() {
        bail!(
            "no stack tradeoff matched profile prefix `{}`{}",
            args.profile,
            if selector.is_empty() {
                String::new()
            } else {
                format!(" and selector `{selector}`")
            }
        );
    }
    let addresses = matches
        .iter()
        .map(|(address, _)| address)
        .collect::<BTreeSet<_>>();
    if addresses.len() != 1 {
        bail!(
            "profile prefix `{}` is ambiguous across {} addresses",
            args.profile,
            addresses.len()
        );
    }
    let profile_address = (*addresses.iter().next().expect("one address")).clone();
    let artifact_ids = matches
        .into_iter()
        .map(|(_, artifact_id)| artifact_id)
        .collect::<BTreeSet<_>>();

    let mut decisions = BTreeSet::new();
    for row in corpus.graph_rows(STACK_EVENT_LEVEL, selector, Some(TRADEOFF_EVENT_UNIT), None)? {
        if artifact_ids.contains(&row.artifact_id) {
            decisions.insert(decision_from_graph(&row.graph)?);
        }
    }
    if decisions.is_empty() {
        bail!("matched view records have no source stack tradeoff graphs");
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "solc-stack-in-policy/1",
            "riffcat": {
                "view": "solc.stack-in-tradeoff/1",
                "facet": "structure+constants",
                "profile_address": profile_address,
            },
            "decisions": decisions,
        }))?
    );
    Ok(())
}

fn decision_from_graph(graph: &Graph) -> Result<PolicyDecision> {
    let root = graph
        .nodes
        .values()
        .find(|node| node.kind.as_str() == "solc.stack.stack_in_tradeoff")
        .ok_or_else(|| anyhow::anyhow!("tradeoff graph has no root node"))?;
    Ok(PolicyDecision {
        object: text_field(root, "object")?.to_string(),
        function_graph_id: u64_field(root, "function_graph_id")?,
        block: u64_field(root, "block")?,
        layout_iteration: u64_field(root, "layout_iteration")?,
        candidate: u64_field(root, "candidate")?,
    })
}

fn text_field<'a>(node: &'a Node, name: &str) -> Result<&'a str> {
    node.fields
        .iter()
        .find(|field| field.name.as_str() == name)
        .and_then(|field| match &field.value {
            Value::Text(value) => Some(value.as_str()),
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("tradeoff root is missing text field `{name}`"))
}

fn u64_field(node: &Node, name: &str) -> Result<u64> {
    node.fields
        .iter()
        .find(|field| field.name.as_str() == name)
        .and_then(|field| match field.value {
            Value::U64(value) => Some(value),
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("tradeoff root is missing integer field `{name}`"))
}

#[cfg(test)]
mod tests {
    use riff_catalog_yul::stack_trace::parse_solc_stack_trace;
    use serde_json::json;

    use super::*;

    #[test]
    fn extracts_a_replayable_decision_from_a_tradeoff_graph() {
        let input = [
            json!({
                "record": "metadata",
                "schema": "solc-compiler-event-stream/1",
                "source": "fixture.yul",
                "object": "Fixture"
            }),
            json!({
                "record": "stack_in_candidate",
                "schema": "solc-stack-in-choice/2",
                "object": "Fixture.Runtime",
                "function_graph_id": 3,
                "function": "f",
                "block": 7,
                "layout_iteration": 2,
                "candidate": 0,
                "default": true,
                "selected": true,
                "stable_iteration": true,
                "local_cost": {"gas": 3, "spills": 0, "stack_size": 6}
            }),
            json!({
                "record": "stack_in_candidate",
                "schema": "solc-stack-in-choice/2",
                "object": "Fixture.Runtime",
                "function_graph_id": 3,
                "function": "f",
                "block": 7,
                "layout_iteration": 2,
                "candidate": 1,
                "default": false,
                "selected": false,
                "stable_iteration": true,
                "local_cost": {"gas": 5, "spills": 0, "stack_size": 5}
            }),
        ]
        .into_iter()
        .map(|record| serde_json::to_string(&record).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
        let trace = parse_solc_stack_trace(&input, "owner").unwrap();
        let alternative = trace
            .observations
            .iter()
            .find(|observation| {
                observation.record_kind == "stack_in_tradeoff"
                    && observation.metrics["gas_penalty"] == 2
            })
            .unwrap();

        assert_eq!(
            decision_from_graph(&alternative.unit.graph).unwrap(),
            PolicyDecision {
                object: "Fixture.Runtime".into(),
                function_graph_id: 3,
                block: 7,
                layout_iteration: 2,
                candidate: 1,
            }
        );
    }
}
