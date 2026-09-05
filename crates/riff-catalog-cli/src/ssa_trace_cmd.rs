//! Ingest and query solc SSA observation streams.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use riff_catalog_yul::ssa::YUL_SSA_LEVEL;
use riff_catalog_yul::trace::{
    SOLC_SSA_OBSERVATION_SCHEMA, TraceObservation, parse_solc_ssa_trace,
};
use serde_json::json;
use sha2::{Digest as _, Sha256};

use crate::corpus::{Corpus, Record};
use crate::ingest::emit_unit;
use crate::table::Table;

#[derive(Clone)]
struct CachedUnit {
    digest_records: Vec<Record>,
    identity_address: String,
    shape_address: String,
}

pub struct TraceIngestArgs {
    pub path: PathBuf,
    pub owner: Option<String>,
}

pub fn ingest(corpus: &Corpus, args: &TraceIngestArgs, json_output: bool) -> Result<()> {
    let source = std::fs::read_to_string(&args.path)
        .with_context(|| format!("reading SSA trace {}", args.path.display()))?;
    let provisional = parse_solc_ssa_trace(&source, "solc-ssa:provisional")
        .with_context(|| format!("parsing SSA trace {}", args.path.display()))?;
    let owner = args.owner.clone().unwrap_or_else(|| {
        let source_name = Path::new(&provisional.metadata.source)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("input");
        format!("solc-ssa:{source_name}:{}", provisional.metadata.object)
    });
    let trace = parse_solc_ssa_trace(&source, &owner)
        .with_context(|| format!("lowering SSA trace {}", args.path.display()))?;

    let mut records = Vec::new();
    let mut unit_cache: BTreeMap<Vec<u8>, CachedUnit> = BTreeMap::new();
    for observation in &trace.observations {
        let artifact_id = snapshot_id(&owner, &trace.metadata.source, observation);
        records.push(Record::Artifact {
            artifact_id: artifact_id.clone(),
            owner: owner.clone(),
            origin: trace.metadata.source.clone(),
            pipeline: SOLC_SSA_OBSERVATION_SCHEMA.to_string(),
            optimize: true,
            compiler: None,
            label: None,
        });
        let graph_wire = serde_json::to_vec(&observation.unit.graph)?;
        let addresses: BTreeMap<String, String> = if let Some(cached) = unit_cache.get(&graph_wire)
        {
            records.push(Record::Graph {
                artifact_id: artifact_id.clone(),
                unit: observation.unit.unit.to_string(),
                level: YUL_SSA_LEVEL.to_string(),
                name: observation.unit.name.clone(),
                graph_key: observation.unit.graph_key.clone(),
                graph: observation.unit.graph.clone(),
            });
            for template in &cached.digest_records {
                let mut record = template.clone();
                let Record::Digest {
                    artifact_id: cached_artifact,
                    ..
                } = &mut record
                else {
                    unreachable!("unit cache contains only digest records")
                };
                *cached_artifact = artifact_id.clone();
                records.push(record);
            }
            [
                ("identity".to_string(), cached.identity_address.clone()),
                ("shape".to_string(), cached.shape_address.clone()),
            ]
            .into_iter()
            .collect()
        } else {
            let first_record = records.len();
            let addresses = emit_unit(
                &mut records,
                &artifact_id,
                &owner,
                YUL_SSA_LEVEL,
                observation.unit.unit,
                &observation.unit.name,
                &observation.unit.graph_key,
                &observation.unit.graph,
            )?;
            unit_cache.insert(
                graph_wire,
                CachedUnit {
                    digest_records: records[first_record + 1..].to_vec(),
                    identity_address: addresses["identity"].to_hex(),
                    shape_address: addresses["shape"].to_hex(),
                },
            );
            addresses
                .into_iter()
                .map(|(mode, address)| (mode, address.to_hex()))
                .collect()
        };
        records.push(Record::Observation {
            artifact_id,
            owner: owner.clone(),
            unit: observation.unit.unit.to_string(),
            level: YUL_SSA_LEVEL.to_string(),
            name: observation.unit.name.clone(),
            schema: SOLC_SSA_OBSERVATION_SCHEMA.to_string(),
            stage_kind: observation.stage_kind.clone(),
            stage: observation.stage.clone(),
            ordinal: observation.ordinal,
            function_graph_id: observation.function_graph_id,
            duration_us: observation.duration_us,
            metrics: observation.metrics.clone(),
            identity_address: addresses["identity"].clone(),
            shape_address: addresses["shape"].clone(),
        });
    }

    let file_stem = trace_file_stem(&args.path, &owner, &trace.metadata.source);
    corpus.replace(&file_stem, &records)?;
    let report = json!({
        "schema": SOLC_SSA_OBSERVATION_SCHEMA,
        "owner": owner,
        "source": trace.metadata.source,
        "object": trace.metadata.object,
        "observations": trace.observations.len(),
        "records": records.len(),
    });
    if json_output {
        println!("{report}");
    } else {
        println!(
            "ingested {} SSA observations for {}",
            trace.observations.len(),
            report["owner"].as_str().unwrap_or_default()
        );
    }
    Ok(())
}

pub fn list(
    corpus: &Corpus,
    selector: Option<&str>,
    function: Option<&str>,
    stage_kind: Option<&str>,
    json_output: bool,
) -> Result<()> {
    let selector = selector.unwrap_or_default();
    let mut rows = corpus
        .load_non_graph_records()?
        .into_iter()
        .filter_map(|record| match record {
            Record::Observation {
                artifact_id,
                owner,
                name,
                stage_kind: row_kind,
                stage,
                ordinal,
                function_graph_id,
                duration_us,
                metrics,
                shape_address,
                ..
            } if (selector.is_empty()
                || artifact_id.contains(selector)
                || owner.contains(selector)
                || name.contains(selector))
                && function.is_none_or(|wanted| {
                    wanted == name || (wanted == "<main>" && function_graph_id == 0)
                })
                && stage_kind.is_none_or(|wanted| wanted == row_kind) =>
            {
                Some((
                    function_graph_id,
                    row_kind,
                    ordinal,
                    stage,
                    name,
                    duration_us,
                    metrics,
                    shape_address,
                ))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| stage_rank(&left.1).cmp(&stage_rank(&right.1)))
            .then_with(|| left.2.cmp(&right.2))
            .then_with(|| left.3.cmp(&right.3))
    });
    if rows.is_empty() {
        bail!("no SSA observations matched the requested filters");
    }

    let mut table = Table::new(&[
        "graph", "function", "kind", "#", "stage", "live-in", "live-out", "op-live", "spills",
        "stack-in", "shuffles", "gas", "bytes", "applied", "us", "shape",
    ]);
    for (graph_id, kind, ordinal, stage, name, duration, metrics, address) in rows {
        table.row(vec![
            graph_id.to_string(),
            if graph_id == 0 { "<main>".into() } else { name },
            kind,
            ordinal.to_string(),
            stage,
            metric(&metrics, "max_live_in"),
            metric(&metrics, "max_live_out"),
            metric(&metrics, "max_operation_live_out"),
            metric(&metrics, "spills"),
            metric(&metrics, "max_stack_in"),
            metric(&metrics, "shuffle_operations"),
            metric(&metrics, "shuffle_gas"),
            metric(&metrics, "bytecode_size"),
            metric(&metrics, "applied_policy_decisions"),
            duration.to_string(),
            address[..16].to_string(),
        ]);
    }
    if json_output {
        println!("{}", table.to_json());
    } else {
        table.print();
    }
    Ok(())
}

fn snapshot_id(owner: &str, source: &str, observation: &TraceObservation) -> String {
    let mut hasher = Sha256::new();
    for value in [
        owner,
        source,
        &observation.stage_kind,
        &observation.stage,
        &observation.function_graph_id.to_string(),
        &observation.ordinal.to_string(),
    ] {
        hasher.update((value.len() as u64).to_le_bytes());
        hasher.update(value.as_bytes());
    }
    hex::encode(hasher.finalize())[..16].to_string()
}

fn trace_file_stem(path: &Path, owner: &str, source: &str) -> String {
    let mut hasher = Sha256::new();
    for value in [
        path.as_os_str().as_encoded_bytes(),
        owner.as_bytes(),
        source.as_bytes(),
    ] {
        hasher.update((value.len() as u64).to_le_bytes());
        hasher.update(value);
    }
    format!("ssa-trace-{}", &hex::encode(hasher.finalize())[..24])
}

fn metric(metrics: &std::collections::BTreeMap<String, u64>, name: &str) -> String {
    metrics
        .get(name)
        .map(u64::to_string)
        .unwrap_or_else(|| "-".to_string())
}

fn stage_rank(stage_kind: &str) -> u8 {
    match stage_kind {
        "transform" => 0,
        "layout" => 1,
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use riff_catalog_yul::lower::LoweredUnit;
    use serde_json::{Value, json};

    static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

    struct ScratchDir(PathBuf);

    impl ScratchDir {
        fn new() -> Self {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/riffcat-test-scratch")
                .join(format!(
                    "ssa-trace-{}-{}",
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

    #[test]
    fn snapshot_ids_are_stable_and_stage_specific() {
        let graph_key = riff_catalog_core::GraphKey::new(
            riff_catalog_core::EntityKey::new("test", "owner", "unit").unwrap(),
            "unit",
        )
        .unwrap();
        let unit = LoweredUnit {
            graph: riff_catalog_core::Graph::new(graph_key.clone()),
            graph_key,
            unit: "unit",
            name: "f".into(),
        };
        let mut observation = TraceObservation {
            stage_kind: "transform".into(),
            stage: "input".into(),
            ordinal: 0,
            function_graph_id: 1,
            function: Some("f".into()),
            duration_us: 0,
            metrics: Default::default(),
            unit,
        };
        let first = snapshot_id("owner", "source", &observation);
        assert_eq!(first, snapshot_id("owner", "source", &observation));
        observation.ordinal = 1;
        assert_ne!(first, snapshot_id("owner", "source", &observation));
    }

    #[test]
    fn ingest_is_idempotent_and_addresses_track_graph_content() {
        let scratch = ScratchDir::new();
        let trace_path = scratch.0.join("trace.jsonl");
        let corpus = Corpus::open(&scratch.0.join("corpus")).unwrap();
        let one_block = json!({
            "type": "Main",
            "name": "",
            "entry": "Block0",
            "blocks": [{
                "id": "Block0",
                "instructions": [],
                "exit": {"type": "MainExit"}
            }]
        });
        let two_blocks = json!({
            "type": "Main",
            "name": "",
            "entry": "Block0",
            "blocks": [
                {
                    "id": "Block0",
                    "instructions": [],
                    "exit": {"type": "Jump", "targets": ["Block1"]}
                },
                {
                    "id": "Block1",
                    "instructions": [],
                    "exit": {"type": "MainExit"}
                }
            ]
        });
        let records = [
            json!({
                "record": "metadata",
                "schema": SOLC_SSA_OBSERVATION_SCHEMA,
                "source": "fixture.yul",
                "object": "Fixture"
            }),
            observation("input", 0, one_block.clone(), json!({"max_live_in": 2})),
            observation(
                "fold-constant-conditions",
                1,
                one_block,
                json!({"max_live_in": 2}),
            ),
            observation(
                "clean-unreachable-blocks",
                2,
                two_blocks,
                json!({"max_live_in": 2, "spills": 1, "shuffle_operations": 5}),
            ),
        ];
        let input = records
            .iter()
            .map(|record| serde_json::to_string(record).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&trace_path, input).unwrap();
        let args = TraceIngestArgs {
            path: trace_path,
            owner: Some("trace:test".into()),
        };
        ingest(&corpus, &args, false).unwrap();
        ingest(&corpus, &args, false).unwrap();

        let observations = corpus
            .load_non_graph_records()
            .unwrap()
            .into_iter()
            .filter_map(|record| match record {
                Record::Observation {
                    stage,
                    metrics,
                    shape_address,
                    ..
                } => Some((stage, metrics, shape_address)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(observations.len(), 3);
        assert_eq!(observations[0].2, observations[1].2);
        assert_ne!(observations[1].2, observations[2].2);
        assert_eq!(observations[2].1["spills"], 1);
        assert_eq!(observations[2].1["shuffle_operations"], 5);
    }

    fn observation(stage: &str, ordinal: u64, graph: Value, metrics: Value) -> Value {
        json!({
            "record": "ssa_observation",
            "schema": SOLC_SSA_OBSERVATION_SCHEMA,
            "stage_kind": "transform",
            "stage": stage,
            "ordinal": ordinal,
            "function_graph_id": 0,
            "function": null,
            "duration_us": 0,
            "metrics": metrics,
            "graph": graph
        })
    }
}
