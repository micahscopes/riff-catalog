use std::{collections::BTreeMap, fs, path::PathBuf};

use anyhow::{Context, Result, anyhow, bail};

use crate::*;

pub struct FeImport {
    pub trace: PathBuf,
    pub wgsl: Option<PathBuf>,
    pub label: String,
    pub source_id: String,
    pub compiler_id: String,
    pub producer_revision: String,
    pub command: String,
    pub settings: BTreeMap<String, String>,
    pub environment: BTreeMap<String, String>,
}

pub fn import_fe_trace(input: FeImport) -> Result<Capture> {
    const MAX_TRACE_BYTES: u64 = 64 * 1024 * 1024;
    let trace_size = fs::metadata(&input.trace)?.len();
    if trace_size > MAX_TRACE_BYTES {
        bail!("Fe trace exceeds {MAX_TRACE_BYTES} byte import limit");
    }
    let trace_path = fs::canonicalize(&input.trace)
        .with_context(|| format!("resolve trace {}", input.trace.display()))?;
    let trace_text = fs::read_to_string(&trace_path)
        .with_context(|| format!("read Fe stderr trace {}", input.trace.display()))?;
    let (trace_digest, trace_bytes) = artifact_digest(&trace_path)?;
    let mut artifacts = vec![Artifact {
        id: "fe-stderr".into(),
        role: ArtifactRole::CompilerTrace,
        path: trace_path.display().to_string(),
        blake3: trace_digest,
        bytes: trace_bytes,
    }];
    if let Some(path) = &input.wgsl {
        let path =
            fs::canonicalize(path).with_context(|| format!("resolve WGSL {}", path.display()))?;
        let (digest, bytes) = artifact_digest(&path)?;
        artifacts.push(Artifact {
            id: "wgsl".into(),
            role: ArtifactRole::EmittedWgsl,
            path: path.display().to_string(),
            blake3: digest,
            bytes,
        });
    }

    let mut stages = Vec::new();
    let mut observations = Vec::new();
    let mut segment = 0u64;
    let mut last_stage: Option<String> = None;
    for (zero_line, raw) in trace_text.lines().enumerate() {
        let line = u64::try_from(zero_line + 1)?;
        let text = raw.trim();
        if let Some(rest) = text.strip_prefix("fe spirv exact function merge: ") {
            segment += 1;
            let values = fields(rest)?;
            let (functions_before, functions_after) = arrow(required(&values, "functions")?)?;
            let (instructions_before, instructions_after) =
                arrow(required(&values, "instructions")?)?;
            let pre = format!("segment-{segment}-pre-merge");
            stages.push(trace_stage(
                pre.clone(),
                "fe_exact_merge_input",
                None,
                vec![
                    compat_measure(
                        "functions",
                        MeasurementScope::AllModule,
                        Quantity::Functions(functions_before),
                    ),
                    compat_measure(
                        "instructions",
                        MeasurementScope::AllModule,
                        Quantity::Instructions(instructions_before),
                    ),
                ],
            ));
            let post = format!("segment-{segment}-post-merge");
            stages.push(trace_stage(
                post.clone(),
                "fe_exact_merge_output",
                Some(pre),
                vec![
                    compat_measure(
                        "functions",
                        MeasurementScope::AllModule,
                        Quantity::Functions(functions_after),
                    ),
                    compat_measure(
                        "instructions",
                        MeasurementScope::AllModule,
                        Quantity::Instructions(instructions_after),
                    ),
                    compat_measure(
                        "merge_candidates",
                        MeasurementScope::TraceSegment,
                        Quantity::Functions(number(required(&values, "candidates")?)?),
                    ),
                    compat_measure(
                        "merged_functions",
                        MeasurementScope::TraceSegment,
                        Quantity::Functions(number(required(&values, "merged")?)?),
                    ),
                ],
            ));
            last_stage = Some(post);
            continue;
        }
        if let Some(rest) = text.strip_prefix("fe spirv inliner: strategy=rooted, ") {
            let values = fields(rest)?;
            let measurements = vec![
                compat_measure(
                    "functions",
                    MeasurementScope::AllModule,
                    Quantity::Functions(number(required(&values, "functions")?)?),
                ),
                compat_measure(
                    "selected_root_bodies",
                    MeasurementScope::SelectedEntryBodies {
                        entries_known: false,
                    },
                    Quantity::Instructions(number(required(&values, "initial_insts")?)?),
                ),
                compat_measure(
                    "selected_entries",
                    MeasurementScope::TraceSegment,
                    Quantity::Functions(number(required(&values, "roots")?)?),
                ),
                compat_measure(
                    "preserved_helpers",
                    MeasurementScope::TraceSegment,
                    Quantity::Functions(number(required(&values, "preserved_helpers")?)?),
                ),
            ];
            last_stage = Some(push_line_stage(
                &mut stages,
                segment,
                line,
                "fe_rooted_inliner_start",
                last_stage,
                measurements,
            ));
            continue;
        }
        if let Some(rest) = text.strip_prefix("fe spirv rooted inliner: ") {
            let values = fields(rest)?;
            let frontier = required(&values, "frontier")?;
            let (kind, value) = if let Some(value) = values.get("after_inline_insts") {
                (format!("fe_frontier_{frontier}_after_inline"), *value)
            } else if let Some(value) = values.get("after_cleanup_insts") {
                (format!("fe_frontier_{frontier}_after_cleanup"), *value)
            } else {
                preserve_unknown(&mut observations, segment, line, text);
                continue;
            };
            let measurements = vec![compat_measure(
                "selected_root_bodies",
                MeasurementScope::SelectedEntryBodies {
                    entries_known: false,
                },
                Quantity::Instructions(number(value)?),
            )];
            last_stage = Some(push_line_stage(
                &mut stages,
                segment,
                line,
                &kind,
                last_stage,
                measurements,
            ));
            continue;
        }
        if let Some(rest) = text.strip_prefix("fe spirv rooted cleanup: ") {
            let values = fields(rest)?;
            let (phase, value) = if let Some(value) = values.get("before_insts") {
                ("before", *value)
            } else if let Some(value) = values.get("after_insts") {
                ("after", *value)
            } else {
                preserve_unknown(&mut observations, segment, line, text);
                continue;
            };
            let kind = format!(
                "fe_frontier_{}_cleanup_{}_{}",
                required(&values, "frontier")?,
                required(&values, "pass")?,
                phase
            );
            let measurements = vec![compat_measure(
                "selected_root_bodies",
                MeasurementScope::SelectedEntryBodies {
                    entries_known: false,
                },
                Quantity::Instructions(number(value)?),
            )];
            last_stage = Some(push_line_stage(
                &mut stages,
                segment,
                line,
                &kind,
                last_stage,
                measurements,
            ));
            continue;
        }
        if let Some(rest) = text.strip_prefix("fe spirv post-inline cleanup: ") {
            let values = fields(rest)?;
            let (phase, value) = if let Some(value) = values.get("before_insts") {
                ("before", *value)
            } else if let Some(value) = values.get("after_insts") {
                ("after", *value)
            } else {
                preserve_unknown(&mut observations, segment, line, text);
                continue;
            };
            let kind = format!(
                "fe_post_inline_cleanup_{}_{}",
                required(&values, "pass")?,
                phase
            );
            let measurements = vec![compat_measure(
                "selected_root_bodies",
                MeasurementScope::SelectedEntryBodies {
                    entries_known: false,
                },
                Quantity::Instructions(number(value)?),
            )];
            last_stage = Some(push_line_stage(
                &mut stages,
                segment,
                line,
                &kind,
                last_stage,
                measurements,
            ));
            continue;
        }
        if let Some(rest) = text.strip_prefix("fe spirv rooted helper clones: ") {
            let values = fields(rest)?;
            let observation_stage = last_stage.clone().ok_or_else(|| {
                anyhow!("helper clone observation at line {line} precedes a recognized stage")
            })?;
            observations.push(CompatibilityObservation::HelperClones {
                segment,
                observation_stage,
                compiler_frontier: number(required(&values, "frontier")?)?,
                compiler_stage: required(&values, "stage")?.to_owned(),
                callee_name: required(&values, "callee")?.to_owned(),
                callsites: number(required(&values, "callsites")?)?,
                cloned_instructions: number(required(&values, "cloned_insts")?)?,
                surviving_original_ids: number(required(&values, "surviving_insts")?)?,
                artifact: "fe-stderr".into(),
                line,
            });
            continue;
        }
        if let Some(rest) = text.strip_prefix("sonatina spirv: emitted wgsl, ") {
            let values = fields(rest)?;
            let mut measurements = vec![compat_measure(
                "producer_wgsl_bytes",
                MeasurementScope::TraceSegment,
                Quantity::Bytes(number(required(&values, "bytes")?)?),
            )];
            if let Some(value) = values.get("elapsed_ms") {
                measurements.push(compat_measure(
                    "wgsl_emission_elapsed",
                    MeasurementScope::TraceSegment,
                    Quantity::Nanoseconds(milliseconds(value)?),
                ));
            }
            if let Some(value) = values.get("total_elapsed_ms") {
                measurements.push(compat_measure(
                    "spirv_backend_total_elapsed",
                    MeasurementScope::TraceSegment,
                    Quantity::Nanoseconds(milliseconds(value)?),
                ));
            }
            last_stage = Some(push_line_stage(
                &mut stages,
                segment,
                line,
                "sonatina_emitted_wgsl",
                last_stage,
                measurements,
            ));
            continue;
        }
        if let Some(rest) = text.strip_prefix("streaming round-interaction WGSL: ") {
            if let Some(value) = rest.strip_suffix(" bytes") {
                let measurements = vec![compat_measure(
                    "reported_shipped_wgsl_bytes",
                    MeasurementScope::TraceSegment,
                    Quantity::Bytes(number(value)?),
                )];
                last_stage = Some(push_line_stage(
                    &mut stages,
                    segment,
                    line,
                    "fe_test_reported_wgsl",
                    last_stage,
                    measurements,
                ));
                continue;
            }
        }
        if text.starts_with("fe spirv ")
            || text.starts_with("fe naga ")
            || text.starts_with("sonatina ")
        {
            preserve_unknown(&mut observations, segment, line, text);
        }
    }
    if segment == 0 {
        bail!("trace contains no exact Fe function-merge marker");
    }
    if let Some(wgsl) = artifacts.iter().find(|a| a.id == "wgsl") {
        stages.push(trace_stage(
            "wgsl-artifact".into(),
            "emitted_wgsl",
            None,
            vec![Measurement {
                name: "artifact_size".into(),
                scope: MeasurementScope::Artifact {
                    artifact: "wgsl".into(),
                },
                quantity: Quantity::Bytes(wgsl.bytes),
                evidence: Evidence::ArtifactMeasurement {
                    artifact: "wgsl".into(),
                },
            }],
        ));
    }
    Ok(Capture {
        label: input.label,
        provenance: Provenance { producer: "Fe stderr compatibility importer".into(), producer_revision: input.producer_revision,
            command: input.command, environment: input.environment,
            notes: vec!["Trace observations are compatibility evidence, weaker than machine-readable compiler events.".into(),
                "Helper clone rows are cumulative snapshots. Do not sum them across cleanup stages or frontiers.".into(),
                "surviving_original_ids counts literal retained instruction IDs, not rewritten descendants.".into(),
                "The stderr trace does not prove a direct call graph, clone caller identity, removability, or semantic equivalence.".into()] },
        alignment: Alignment { source_id: input.source_id, compiler_id: input.compiler_id, settings: input.settings },
        artifacts, stages, inline_events: Vec::new(), compatibility_observations: observations,
    })
}

fn compat_measure(name: &str, scope: MeasurementScope, quantity: Quantity) -> Measurement {
    Measurement {
        name: name.into(),
        scope,
        quantity,
        evidence: Evidence::CompatibilityTrace {
            artifact: "fe-stderr".into(),
            grammar: "fe-spirv-stderr/2026-09-05".into(),
        },
    }
}

fn trace_stage(
    id: String,
    kind: &str,
    predecessor: Option<String>,
    measurements: Vec<Measurement>,
) -> Stage {
    Stage {
        id,
        kind: kind.into(),
        predecessors: predecessor.into_iter().collect(),
        functions: Vec::new(),
        selected_entries: Vec::new(),
        direct_calls: Vec::new(),
        call_graph: CallGraphCompleteness::Incomplete {
            reason: "Fe stderr does not encode a call graph".into(),
        },
        unknown_indirect_calls: Vec::new(),
        measurements,
    }
}

fn push_line_stage(
    stages: &mut Vec<Stage>,
    segment: u64,
    line: u64,
    kind: &str,
    predecessor: Option<String>,
    measurements: Vec<Measurement>,
) -> String {
    let id = format!("segment-{segment}-line-{line}");
    stages.push(trace_stage(id.clone(), kind, predecessor, measurements));
    id
}

fn preserve_unknown(
    observations: &mut Vec<CompatibilityObservation>,
    segment: u64,
    line: u64,
    text: &str,
) {
    observations.push(CompatibilityObservation::UnknownLine {
        segment,
        line,
        text: text.into(),
        artifact: "fe-stderr".into(),
    });
}

fn fields(text: &str) -> Result<BTreeMap<&str, &str>> {
    let mut out = BTreeMap::new();
    for part in text.split(", ") {
        let (key, value) = part
            .split_once('=')
            .ok_or_else(|| anyhow!("malformed Fe trace field `{part}`"))?;
        if out.insert(key, value).is_some() {
            bail!("duplicate Fe trace field `{key}`");
        }
    }
    Ok(out)
}

fn required<'a>(values: &'a BTreeMap<&str, &str>, key: &str) -> Result<&'a str> {
    values
        .get(key)
        .copied()
        .ok_or_else(|| anyhow!("Fe trace line lacks `{key}`"))
}

fn number(value: &str) -> Result<u64> {
    value
        .parse()
        .with_context(|| format!("invalid non-negative integer `{value}`"))
}
fn milliseconds(value: &str) -> Result<u64> {
    number(value)?
        .checked_mul(1_000_000)
        .ok_or_else(|| anyhow!("millisecond conversion overflows for `{value}`"))
}
fn arrow(value: &str) -> Result<(u64, u64)> {
    let (before, after) = value
        .split_once("->")
        .ok_or_else(|| anyhow!("invalid before/after pair `{value}`"))?;
    Ok((number(before)?, number(after)?))
}
