//! Ingest solc stack-layout decision streams as ordinary riff-cat graphs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use riff_catalog_yul::stack_trace::{StackTraceObservation, parse_solc_stack_trace};
use serde_json::json;
use sha2::{Digest as _, Sha256};

use crate::corpus::{Corpus, Record};
use crate::ingest::emit_unit;

pub struct StackTraceIngestArgs {
    pub path: PathBuf,
    pub owner: Option<String>,
}

pub fn ingest(corpus: &Corpus, args: &StackTraceIngestArgs, json_output: bool) -> Result<()> {
    let source = std::fs::read_to_string(&args.path)
        .with_context(|| format!("reading stack trace {}", args.path.display()))?;
    let provisional = parse_solc_stack_trace(&source, "solc-stack:provisional")
        .with_context(|| format!("parsing stack trace {}", args.path.display()))?;
    let owner = args.owner.clone().unwrap_or_else(|| {
        let source_name = Path::new(&provisional.metadata.source)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("input");
        let policy = provisional
            .metadata
            .stack_in_policy_digest
            .as_deref()
            .map(|digest| &digest[..digest.len().min(16)])
            .unwrap_or("default");
        format!(
            "solc-stack:{source_name}:{}:policy-{policy}",
            provisional.metadata.object
        )
    });
    let trace = parse_solc_stack_trace(&source, &owner)
        .with_context(|| format!("lowering stack trace {}", args.path.display()))?;

    let mut records = Vec::new();
    let mut counts = BTreeMap::<String, u64>::new();
    for observation in &trace.observations {
        *counts.entry(observation.record_kind.clone()).or_default() += 1;
        let artifact_id = event_id(&owner, &trace.metadata.source, observation);
        records.push(Record::Artifact {
            artifact_id: artifact_id.clone(),
            owner: owner.clone(),
            origin: trace.metadata.source.clone(),
            pipeline: trace.metadata.schema.clone(),
            optimize: true,
            compiler: None,
            label: None,
        });
        let addresses = emit_unit(
            &mut records,
            &artifact_id,
            &owner,
            observation.level,
            observation.unit.unit,
            &observation.unit.name,
            &observation.unit.graph_key,
            &observation.unit.graph,
        )?;
        records.push(Record::Observation {
            artifact_id,
            owner: owner.clone(),
            unit: observation.unit.unit.to_string(),
            level: observation.level.to_string(),
            name: observation.unit.name.clone(),
            schema: observation.schema.clone(),
            stage_kind: observation.stage_kind.clone(),
            stage: observation.stage.clone(),
            ordinal: observation.ordinal,
            function_graph_id: observation.function_graph_id,
            duration_us: observation.duration_us,
            metrics: observation.metrics.clone(),
            identity_address: addresses["identity"].to_hex(),
            shape_address: addresses["shape"].to_hex(),
        });
    }

    let file_stem = trace_file_stem(&args.path, &owner, &trace.metadata.source);
    corpus.replace(&file_stem, &records)?;
    let report = json!({
        "schema": trace.metadata.schema,
        "owner": owner,
        "source": trace.metadata.source,
        "object": trace.metadata.object,
        "stack_in_policy_digest": trace.metadata.stack_in_policy_digest,
        "events": trace.observations.len(),
        "event_counts": counts,
        "records": records.len(),
    });
    if json_output {
        println!("{report}");
    } else {
        println!(
            "ingested {} stack events for {}",
            trace.observations.len(),
            report["owner"].as_str().unwrap_or_default()
        );
    }
    Ok(())
}

fn event_id(owner: &str, source: &str, observation: &StackTraceObservation) -> String {
    let mut hasher = Sha256::new();
    for value in [
        owner,
        source,
        &observation.schema,
        &observation.unit.graph_key.canonical_key(),
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
    format!("stack-trace-{}", &hex::encode(hasher.finalize())[..24])
}
