use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{BufReader, Read},
    path::PathBuf,
};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::*;

const EVENT_SCHEMA: &str = "fe-bloat-event/1";
const MAX_EVENT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EVENTS: usize = 100_000;

pub struct FeEventsImport {
    pub events: PathBuf,
    pub label: String,
    pub source_id: String,
    pub compiler_id: String,
    pub producer_revision: String,
    pub command: String,
    pub settings: BTreeMap<String, String>,
}

pub fn import_fe_events(input: FeEventsImport) -> Result<Capture> {
    let event_path = fs::canonicalize(&input.events)
        .with_context(|| format!("resolve Fe event stream {}", input.events.display()))?;
    let metadata = fs::metadata(&event_path)?;
    if metadata.len() > MAX_EVENT_BYTES {
        bail!("Fe event stream exceeds {MAX_EVENT_BYTES} byte limit");
    }
    let source = fs::read_to_string(&event_path)?;
    let mut request_id = None;
    let mut start = None;
    let mut stages = Vec::new();
    let mut inline_events = Vec::new();
    let mut clone_observations = Vec::new();
    let mut decisions = Vec::new();
    let mut artifacts = Vec::new();
    let mut intervention = Intervention::none();
    let mut completion = CaptureCompletion::Incomplete {
        reason: "event stream has no completion marker".into(),
    };
    let mut completion_seen = false;
    let mut merge_edges = Vec::new();
    let mut artifact_stage = None;
    let mut completed_stage = None;
    let mut failed_stage = None;
    let mut forced_display_names = Vec::new();
    let mut helper_selection_seen = false;

    for (index, line) in source.lines().enumerate() {
        if index >= MAX_EVENTS {
            bail!("Fe event stream exceeds {MAX_EVENTS} event limit");
        }
        if line.trim().is_empty() {
            bail!("Fe event stream has a blank record at line {}", index + 1);
        }
        if completion_seen {
            bail!("Fe event stream contains records after its completion marker");
        }
        let envelope = parse_envelope(line)
            .with_context(|| format!("parse Fe event at line {}", index + 1))?;
        if envelope.schema != EVENT_SCHEMA {
            bail!("unsupported Fe event schema `{}`", envelope.schema);
        }
        if envelope.sequence != index {
            bail!("Fe event sequence is not contiguous at line {}", index + 1);
        }
        match &request_id {
            Some(expected) if expected != &envelope.request_id => {
                bail!("Fe event request ID changes at line {}", index + 1)
            }
            None => request_id = Some(envelope.request_id.clone()),
            _ => {}
        }
        match envelope.event {
            RawEvent::CaptureStarted {
                pipeline,
                compiler,
                entry_labels,
                environment,
                intervention: raw,
                graph_semantics,
            } => {
                if index != 0 || start.is_some() {
                    bail!("capture_started must be the first and only start marker");
                }
                start = Some((
                    pipeline,
                    compiler,
                    entry_labels,
                    environment,
                    graph_semantics,
                ));
                if raw.kind != "none" {
                    intervention.kind = raw.kind;
                    intervention.requested = raw
                        .requested
                        .into_iter()
                        .flat_map(|value| {
                            value
                                .split(',')
                                .map(str::trim)
                                .filter(|name| !name.is_empty())
                                .map(str::to_owned)
                                .collect::<Vec<_>>()
                        })
                        .collect();
                }
            }
            RawEvent::Stage {
                stage_id,
                stage_kind,
                predecessors,
                functions,
                selected_entries,
                direct_calls,
                call_graph,
                unknown_indirect_calls,
                measurements,
            } => {
                stages.push(Stage {
                    id: stage_id,
                    kind: stage_kind,
                    predecessors,
                    functions,
                    selected_entries,
                    direct_calls,
                    call_graph,
                    unknown_indirect_calls,
                    measurements: measurements
                        .into_iter()
                        .map(raw_measurement)
                        .collect::<Result<_>>()?,
                });
            }
            RawEvent::ExactFunctionMerge {
                input_stage,
                output_stage,
                candidate_functions,
                merged_functions,
                rewritten_references,
                refinement_rounds,
                evidence,
            } => {
                merge_edges.push((input_stage, output_stage.clone()));
                decisions.push(Decision {
                    stage: output_stage,
                    subject: None,
                    decision: DecisionKind::ExactFunctionMerge {
                        candidate_functions,
                        merged_functions,
                        rewritten_references,
                        refinement_rounds,
                    },
                    evidence,
                })
            }
            RawEvent::HelperAnalysis {
                stage,
                callable,
                backend_rejected,
                evidence,
            } => {
                decisions.extend(callable.into_iter().map(|helper| Decision {
                    stage: stage.clone(),
                    subject: Some(FunctionRef {
                        stage: stage.clone(),
                        function: helper.function,
                    }),
                    decision: DecisionKind::BackendCallable {
                        variants: helper.variants,
                        instructions: helper.instructions,
                        accesses_resource: helper.accesses_resource,
                        maximum_physical_parameters: helper.maximum_physical_parameters,
                    },
                    evidence: evidence.clone(),
                }));
                decisions.extend(backend_rejected.into_iter().map(|helper| Decision {
                    stage: stage.clone(),
                    subject: Some(FunctionRef {
                        stage: stage.clone(),
                        function: helper.function,
                    }),
                    decision: DecisionKind::BackendRejected {
                        reason: helper.reason,
                    },
                    evidence: evidence.clone(),
                }));
            }
            RawEvent::HelperSelection {
                stage,
                baseline_retained,
                selected_retained,
                forced_inline,
                consequential_inline,
                evidence,
            } => {
                helper_selection_seen = true;
                decisions.extend(baseline_retained.into_iter().map(|helper| Decision {
                    stage: stage.clone(),
                    subject: Some(FunctionRef {
                        stage: stage.clone(),
                        function: helper.function,
                    }),
                    decision: DecisionKind::BaselineRetained,
                    evidence: evidence.clone(),
                }));
                decisions.extend(selected_retained.into_iter().map(|helper| Decision {
                    stage: stage.clone(),
                    subject: Some(FunctionRef {
                        stage: stage.clone(),
                        function: helper.function,
                    }),
                    decision: DecisionKind::FrontendRetained,
                    evidence: evidence.clone(),
                }));
                forced_display_names = forced_inline
                    .iter()
                    .map(|helper| helper.display_name.clone())
                    .collect();
                intervention.resolved = forced_inline
                    .iter()
                    .map(|helper| FunctionRef {
                        stage: stage.clone(),
                        function: helper.function.clone(),
                    })
                    .collect();
                intervention.consequential = consequential_inline
                    .iter()
                    .map(|helper| FunctionRef {
                        stage: stage.clone(),
                        function: helper.function.clone(),
                    })
                    .collect();
                decisions.extend(forced_inline.into_iter().map(|helper| Decision {
                    stage: stage.clone(),
                    subject: Some(FunctionRef {
                        stage: stage.clone(),
                        function: helper.function,
                    }),
                    decision: DecisionKind::ForcedInline,
                    evidence: evidence.clone(),
                }));
                decisions.extend(consequential_inline.into_iter().map(|helper| Decision {
                    stage: stage.clone(),
                    subject: Some(FunctionRef {
                        stage: stage.clone(),
                        function: helper.function,
                    }),
                    decision: DecisionKind::ConsequentialInline,
                    evidence: evidence.clone(),
                }));
            }
            RawEvent::InlineEvent {
                event_id,
                caller,
                callee,
                output_stage,
                callsites,
                cloned_instructions,
                surviving_original_ids,
                evidence,
            } => inline_events.push(InlineEvent {
                id: event_id,
                caller,
                callee,
                output_stage,
                callsites,
                cloned_instructions,
                surviving_original_ids: Some(surviving_original_ids),
                evidence,
            }),
            RawEvent::CloneCensus {
                observation_stage,
                compiler_frontier,
                compiler_stage,
                caller,
                callee,
                callsites,
                cloned_instructions,
                surviving_original_ids,
                semantics,
                evidence,
            } => {
                if semantics != "cumulative_literal_original_instruction_ids" {
                    bail!("unsupported clone census semantics `{semantics}`");
                }
                clone_observations.push(CloneObservation {
                    observation_stage,
                    compiler_frontier,
                    compiler_stage,
                    caller,
                    callee,
                    callsites,
                    cloned_instructions,
                    surviving_original_ids,
                    evidence,
                });
            }
            RawEvent::Artifacts {
                stage,
                artifacts: raw_artifacts,
            } => {
                if !artifacts.is_empty() {
                    bail!("Fe event stream contains multiple artifact records");
                }
                artifact_stage = Some(stage);
                let directory = event_path
                    .parent()
                    .ok_or_else(|| anyhow!("event stream has no directory"))?;
                for raw in raw_artifacts {
                    artifacts.push(import_artifact(directory, raw)?);
                }
            }
            RawEvent::CaptureCompleted { final_stage } => {
                completion = CaptureCompletion::Complete {
                    producer_marker: format!("capture_completed:{final_stage}"),
                };
                completed_stage = Some(final_stage);
                completion_seen = true;
            }
            RawEvent::CaptureFailed {
                last_stage,
                message,
            } => {
                failed_stage = (last_stage != "unknown").then_some(last_stage.clone());
                completion = CaptureCompletion::Failed {
                    message,
                    last_stage: (last_stage != "unknown").then_some(last_stage),
                };
                completion_seen = true;
            }
        }
    }
    let (pipeline, compiler, entry_labels, environment, graph_semantics) =
        start.ok_or_else(|| anyhow!("Fe event stream has no capture_started marker"))?;
    let stage_ids = stages
        .iter()
        .map(|stage| stage.id.as_str())
        .collect::<BTreeSet<_>>();
    for (input_stage, output_stage) in &merge_edges {
        let output = stages
            .iter()
            .find(|stage| stage.id == *output_stage)
            .ok_or_else(|| anyhow!("exact merge output stage `{output_stage}` does not exist"))?;
        if !stage_ids.contains(input_stage.as_str()) {
            bail!("exact merge input stage `{input_stage}` does not exist");
        }
        if !output.predecessors.iter().any(|value| value == input_stage) {
            bail!("exact merge output `{output_stage}` does not follow input `{input_stage}`");
        }
    }
    if let Some(stage) = artifact_stage.as_deref()
        && !stage_ids.contains(stage)
    {
        bail!("artifact stage `{stage}` does not exist");
    }
    if let Some(stage) = completed_stage.as_deref() {
        if !stage_ids.contains(stage) {
            bail!("completed final stage `{stage}` does not exist");
        }
        if artifacts.is_empty() {
            bail!("completed capture has no linked output artifacts");
        }
        if artifact_stage.as_deref() != Some(stage) {
            bail!("completed final stage `{stage}` is not the artifact stage");
        }
    }
    if let Some(stage) = failed_stage.as_deref()
        && !stage_ids.contains(stage)
    {
        bail!("failed capture last stage `{stage}` does not exist");
    }
    let requested = intervention
        .requested
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let resolved = forced_display_names.into_iter().collect::<BTreeSet<_>>();
    if (helper_selection_seen || completed_stage.is_some()) && requested != resolved {
        bail!("requested helper names do not exactly match resolved forced-inline helpers");
    }
    if let Some(stage_id) = artifact_stage {
        let stage = stages
            .iter_mut()
            .find(|stage| stage.id == stage_id)
            .expect("artifact stage checked above");
        stage
            .measurements
            .extend(artifacts.iter().map(|artifact| Measurement {
                name: "emitted_artifact_bytes".into(),
                scope: MeasurementScope::Artifact {
                    artifact: artifact.id.clone(),
                },
                quantity: Quantity::Bytes(artifact.bytes),
                evidence: Evidence::ArtifactMeasurement {
                    artifact: artifact.id.clone(),
                },
            }));
    }
    let event_artifact = {
        let (blake3, bytes) = artifact_digest(&event_path)?;
        Artifact {
            id: "fe-events".into(),
            role: ArtifactRole::CompilerTrace,
            path: event_path.display().to_string(),
            blake3,
            bytes,
            producer_digests: BTreeMap::new(),
        }
    };
    artifacts.insert(0, event_artifact);
    Ok(Capture {
        label: input.label,
        provenance: Provenance {
            producer: format!(
                "{} {} structured bloat events",
                compiler.name, compiler.version
            ),
            producer_revision: input.producer_revision,
            command: input.command,
            environment,
            notes: vec![
                format!("pipeline={pipeline}; entries={}", entry_labels.join(",")),
                graph_semantics,
            ],
        },
        alignment: Alignment {
            source_id: input.source_id,
            compiler_id: input.compiler_id,
            settings: input.settings,
        },
        completion,
        intervention,
        artifacts,
        stages,
        inline_events,
        clone_observations,
        decisions,
        compatibility_observations: Vec::new(),
    })
}

fn raw_measurement(raw: RawMeasurement) -> Result<Measurement> {
    let scope = match raw.scope.as_str() {
        "all_module" => MeasurementScope::AllModule,
        "selected_entry_bodies" => MeasurementScope::SelectedEntryBodies {
            entries_known: false,
        },
        other => bail!("unsupported Fe measurement scope `{other}`"),
    };
    let quantity = match raw.unit.as_str() {
        "instructions" => Quantity::Instructions(raw.value),
        "functions" => Quantity::Functions(raw.value),
        other => bail!("unsupported Fe measurement unit `{other}`"),
    };
    Ok(Measurement {
        name: raw.name,
        scope,
        quantity,
        evidence: raw.evidence,
    })
}

fn import_artifact(directory: &std::path::Path, raw: RawArtifact) -> Result<Artifact> {
    let path = fs::canonicalize(directory.join(&raw.path))?;
    let directory = fs::canonicalize(directory)?;
    if !path.starts_with(&directory) {
        bail!("Fe artifact path escapes its request directory");
    }
    let (blake3, bytes) = artifact_digest(&path)?;
    if bytes != raw.bytes {
        bail!(
            "Fe artifact `{}` byte count disagrees with its event",
            raw.id
        );
    }
    let producer_sha256 = sha256_digest(&path)?;
    if producer_sha256 != raw.sha256.to_ascii_lowercase() {
        bail!("Fe artifact `{}` SHA-256 disagrees with its event", raw.id);
    }
    let role = match raw.role.as_str() {
        "emitted_wgsl" => ArtifactRole::EmittedWgsl,
        "emitted_spirv" => ArtifactRole::Other,
        other => bail!("unsupported Fe artifact role `{other}`"),
    };
    Ok(Artifact {
        id: raw.id,
        role,
        path: path.display().to_string(),
        blake3,
        bytes,
        producer_digests: BTreeMap::from([("sha256".into(), raw.sha256)]),
    })
}

fn sha256_digest(path: &std::path::Path) -> Result<String> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

struct Envelope {
    schema: String,
    request_id: String,
    sequence: usize,
    event: RawEvent,
}

fn parse_envelope(line: &str) -> Result<Envelope> {
    let mut object = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(line)?;
    let schema = serde_json::from_value(
        object
            .remove("schema")
            .ok_or_else(|| anyhow!("missing field `schema`"))?,
    )?;
    let request_id = serde_json::from_value(
        object
            .remove("request_id")
            .ok_or_else(|| anyhow!("missing field `request_id`"))?,
    )?;
    let sequence = serde_json::from_value(
        object
            .remove("sequence")
            .ok_or_else(|| anyhow!("missing field `sequence`"))?,
    )?;
    let event = serde_json::from_value(serde_json::Value::Object(object))?;
    Ok(Envelope {
        schema,
        request_id,
        sequence,
        event,
    })
}

#[derive(Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
enum RawEvent {
    CaptureStarted {
        pipeline: String,
        compiler: RawCompiler,
        entry_labels: Vec<String>,
        environment: BTreeMap<String, String>,
        intervention: RawIntervention,
        graph_semantics: String,
    },
    Stage {
        stage_id: String,
        stage_kind: String,
        predecessors: Vec<String>,
        functions: Vec<Function>,
        selected_entries: Vec<String>,
        direct_calls: Vec<DirectCall>,
        call_graph: CallGraphCompleteness,
        unknown_indirect_calls: Vec<UnknownIndirectCall>,
        measurements: Vec<RawMeasurement>,
    },
    ExactFunctionMerge {
        input_stage: String,
        output_stage: String,
        candidate_functions: u64,
        merged_functions: u64,
        rewritten_references: u64,
        refinement_rounds: u64,
        evidence: Evidence,
    },
    HelperAnalysis {
        stage: String,
        callable: Vec<RawCallable>,
        backend_rejected: Vec<RawRejected>,
        evidence: Evidence,
    },
    HelperSelection {
        stage: String,
        baseline_retained: Vec<RawNamedFunction>,
        selected_retained: Vec<RawNamedFunction>,
        forced_inline: Vec<RawNamedFunction>,
        consequential_inline: Vec<RawNamedFunction>,
        evidence: Evidence,
    },
    InlineEvent {
        event_id: String,
        caller: FunctionRef,
        callee: FunctionRef,
        output_stage: String,
        callsites: u64,
        cloned_instructions: u64,
        surviving_original_ids: u64,
        evidence: Evidence,
    },
    CloneCensus {
        observation_stage: String,
        compiler_frontier: u64,
        compiler_stage: String,
        caller: FunctionRef,
        callee: FunctionRef,
        callsites: u64,
        cloned_instructions: u64,
        surviving_original_ids: u64,
        semantics: String,
        evidence: Evidence,
    },
    Artifacts {
        stage: String,
        artifacts: Vec<RawArtifact>,
    },
    CaptureCompleted {
        final_stage: String,
    },
    CaptureFailed {
        last_stage: String,
        message: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCompiler {
    name: String,
    version: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawIntervention {
    kind: String,
    requested: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMeasurement {
    name: String,
    scope: String,
    unit: String,
    value: u64,
    evidence: Evidence,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCallable {
    function: String,
    #[allow(dead_code)]
    display_name: String,
    variants: u64,
    instructions: u64,
    accesses_resource: bool,
    maximum_physical_parameters: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRejected {
    function: String,
    #[allow(dead_code)]
    display_name: String,
    reason: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNamedFunction {
    function: String,
    #[allow(dead_code)]
    display_name: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawArtifact {
    id: String,
    role: String,
    path: String,
    sha256: String,
    bytes: u64,
}
