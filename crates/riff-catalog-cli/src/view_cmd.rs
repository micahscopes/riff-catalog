//! Materialize user-authored graph views and persist their addresses.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use riff_catalog_core::{CyclePolicy, DigestRequest, Facet, HashPolicy, ViewMode, digest_graph};
use riff_catalog_view::ViewPlan;
use serde_json::json;
use sha2::{Digest as _, Sha256};

use crate::corpus::{Corpus, Record};

pub struct ViewArgs {
    pub spec: PathBuf,
    pub selector: String,
    pub unit: Option<String>,
    pub name: Option<String>,
}

pub fn run(corpus: &Corpus, args: &ViewArgs, json_output: bool) -> Result<()> {
    let source = std::fs::read_to_string(&args.spec)
        .with_context(|| format!("reading view {}", args.spec.display()))?;
    let plan = ViewPlan::parse(&source)
        .with_context(|| format!("parsing view {}", args.spec.display()))?;
    let plan_id = plan.plan_id();
    let output_level = plan.output_level();
    let output_unit = format!("view:{}", plan.name);

    let rows = corpus.graph_rows(
        &plan.input_level,
        &args.selector,
        args.unit.as_deref(),
        args.name.as_deref(),
    )?;
    if rows.is_empty() {
        bail!(
            "no graph at level `{}` matched `{}`{}{}",
            plan.input_level,
            args.selector,
            args.unit
                .as_deref()
                .map(|unit| format!(" with unit `{unit}`"))
                .unwrap_or_default(),
            args.name
                .as_deref()
                .map(|name| format!(" and name `{name}`"))
                .unwrap_or_default(),
        );
    }

    let mut reports = Vec::new();
    for row in rows {
        let graph = plan.materialize(&row.graph).with_context(|| {
            format!(
                "materializing view `{}` for {}:{}",
                plan.name, row.unit, row.name
            )
        })?;
        let mut records = vec![Record::Graph {
            artifact_id: row.artifact_id.clone(),
            unit: output_unit.clone(),
            level: output_level.clone(),
            name: row.name.clone(),
            graph_key: row.graph_key.clone(),
            graph: graph.clone(),
        }];
        let mut addresses = BTreeMap::new();

        for (mode, view_mode) in [
            ("identity", ViewMode::IdentityBound),
            ("shape", ViewMode::AnonymousShape),
        ] {
            let policy =
                HashPolicy::new(output_level.clone(), view_mode, CyclePolicy::CondenseScc)?;
            let request = DigestRequest::new(
                row.graph_key.clone(),
                policy.clone(),
                plan.dimensions.clone(),
            )?;
            let result = digest_graph(&request, &graph)?;
            let facet = Facet::new(policy.policy_id(), plan.dimensions.clone())?;
            let address = result.hashes.facet_address(&facet)?.address_digest();
            addresses.insert(mode, address);
            records.push(Record::Digest {
                artifact_id: row.artifact_id.clone(),
                owner: row.graph_key.owner.owner().to_string(),
                unit: output_unit.clone(),
                level: output_level.clone(),
                name: row.name.clone(),
                mode: mode.to_string(),
                policy_id: policy.policy_id(),
                digests: result.hashes.graph.values,
                node_count: graph.nodes.len(),
            });
        }

        let file_stem = view_file_stem(
            &row.artifact_id,
            &row.unit,
            &row.name,
            &row.graph_key.canonical_key(),
            &plan_id.to_hex(),
        );
        corpus.replace(&file_stem, &records)?;

        reports.push(json!({
            "view": plan.name,
            "plan_id": plan_id.to_hex(),
            "input_level": plan.input_level,
            "output_level": output_level,
            "output_unit": output_unit,
            "artifact_id": row.artifact_id,
            "source_unit": row.unit,
            "name": row.name,
            "input_nodes": row.graph.nodes.len(),
            "output_nodes": graph.nodes.len(),
            "dimensions": plan.dimensions.iter().map(|dimension| dimension.as_str()).collect::<Vec<_>>(),
            "identity_address": addresses["identity"].to_hex(),
            "shape_address": addresses["shape"].to_hex(),
        }));
    }

    if json_output {
        println!("{}", serde_json::Value::Array(reports));
    } else {
        println!("view {} ({})", plan.name, plan_id.display_short(),);
        for report in reports {
            println!(
                "{} {}: {} -> {} nodes, shape {}",
                report["artifact_id"].as_str().unwrap_or_default(),
                report["name"].as_str().unwrap_or_default(),
                report["input_nodes"].as_u64().unwrap_or_default(),
                report["output_nodes"].as_u64().unwrap_or_default(),
                report["shape_address"]
                    .as_str()
                    .map(|address| &address[..16])
                    .unwrap_or_default(),
            );
        }
        println!(
            "query with: riffcat bucket --unit {} --facet {}",
            output_unit,
            plan.dimensions
                .iter()
                .map(|dimension| dimension.as_str())
                .collect::<Vec<_>>()
                .join("+"),
        );
    }
    Ok(())
}

fn view_file_stem(
    artifact_id: &str,
    unit: &str,
    name: &str,
    graph_key: &str,
    plan_id: &str,
) -> String {
    let mut hasher = Sha256::new();
    for value in [artifact_id, unit, name, graph_key, plan_id] {
        hasher.update((value.len() as u64).to_le_bytes());
        hasher.update(value.as_bytes());
    }
    format!("view-{}", &hex::encode(hasher.finalize())[..24])
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use riff_catalog_core::{Dimension, EdgeRole, EntityKey, Graph, GraphKey, NodeKey};

    use super::*;

    static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

    struct ScratchDir(PathBuf);

    impl ScratchDir {
        fn new() -> Self {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/riffcat-test-scratch")
                .join(format!(
                    "view-{}-{}",
                    std::process::id(),
                    NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed)
                ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn node(owner: &str, kind: &str, local: &str) -> NodeKey {
        NodeKey::entity(EntityKey::new(kind, owner, local).unwrap())
    }

    fn source_graph() -> Graph {
        let owner = "yulssa:view-test";
        let graph_key =
            GraphKey::new(EntityKey::new("yulssa.fn", owner, "fn:f").unwrap(), "fn").unwrap();
        let mut graph = Graph::new(graph_key);
        let function = node(owner, "yulssa.fn", "fn:f");
        let entry = node(owner, "yulssa.block", "fn:f/b:entry");
        let next = node(owner, "yulssa.block", "fn:f/b:next");
        let dead = node(owner, "yulssa.block", "fn:f/b:dead");
        for (key, kind) in [
            (&function, "yulssa.fn"),
            (&entry, "yulssa.block"),
            (&next, "yulssa.block"),
            (&dead, "yulssa.block"),
        ] {
            graph.add_node(key.clone(), kind).unwrap();
        }
        graph
            .add_field(&function, Dimension::Names, "name", "f")
            .unwrap();
        graph.add_child(&function, "entry", 0, &entry).unwrap();
        graph.add_child(&function, "block", 0, &next).unwrap();
        graph.add_child(&function, "block", 0, &dead).unwrap();
        graph
            .add_edge(&entry, "jump:0", &next, EdgeRole::Dependency)
            .unwrap();
        graph
    }

    #[test]
    fn materializes_persists_and_reloads_a_view() {
        let scratch = ScratchDir::new();
        let corpus = Corpus::open(&scratch.0.join("corpus")).unwrap();
        let graph = source_graph();
        corpus
            .replace(
                "source",
                &[Record::Graph {
                    artifact_id: "artifact-1".into(),
                    unit: "yulssa-fn".into(),
                    level: "yul-ssa-cfg/1".into(),
                    name: "f".into(),
                    graph_key: graph.graph_key.clone(),
                    graph,
                }],
            )
            .unwrap();

        let spec = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/views/yul-reachable.riffview");
        let args = ViewArgs {
            spec: spec.clone(),
            selector: "view-test".into(),
            unit: Some("yulssa-fn".into()),
            name: None,
        };
        run(&corpus, &args, true).unwrap();
        run(&corpus, &args, true).unwrap();

        let plan = ViewPlan::parse(&std::fs::read_to_string(spec).unwrap()).unwrap();
        let output_unit = "view:yul.reachable/1";
        let graphs = corpus
            .graph_rows(&plan.output_level(), "view-test", Some(output_unit), None)
            .unwrap();
        assert_eq!(graphs.len(), 1, "re-running the view must be idempotent");
        assert_eq!(graphs[0].graph.nodes.len(), 3);
        assert!(
            graphs[0]
                .graph
                .nodes
                .keys()
                .all(|key| !key.canonical_key().contains("b:dead"))
        );

        let digests = corpus.digest_rows(output_unit, "shape").unwrap();
        assert_eq!(digests.len(), 1);
        assert_eq!(
            digests[0].digests.keys().copied().collect::<Vec<_>>(),
            vec![Dimension::Structure, Dimension::Constants, Dimension::Types,]
        );
    }
}
