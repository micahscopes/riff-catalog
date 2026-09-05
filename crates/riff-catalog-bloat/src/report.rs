use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    CaptureCompletion, CaptureFile, CloneObservation, CompatibilityObservation, Decision, Evidence,
    InlineEvent, Intervention, MeasurementScope, Quantity, reachable_union,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema: String,
    pub capture_id: String,
    pub label: String,
    pub completion: CaptureCompletion,
    pub intervention: Intervention,
    pub stages: Vec<StageReport>,
    pub helper_observations: Vec<HelperReport>,
    pub latest_helper_observations: Vec<HelperReport>,
    pub inline_events: Vec<InlineEvent>,
    pub clone_observations: Vec<CloneObservation>,
    pub decisions: Vec<Decision>,
    pub unparsed_compiler_lines: usize,
    pub caveats: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageReport {
    pub id: String,
    pub kind: String,
    pub recorded_measurements: Vec<RecordedMeasurement>,
    pub computed_scopes: Vec<ComputedScope>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedMeasurement {
    pub name: String,
    pub scope: MeasurementScope,
    pub quantity: Quantity,
    pub evidence: Evidence,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComputedScope {
    pub scope: String,
    pub instructions: u64,
    pub functions: usize,
    pub complete: bool,
    pub unknown_indirect_callsites: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelperReport {
    pub segment: u64,
    pub observation_stage: String,
    pub compiler_frontier: u64,
    pub compiler_stage: String,
    pub callee_name: String,
    pub callsites: u64,
    pub cloned_instructions_total: u64,
    pub surviving_original_ids: u64,
    pub line: u64,
    pub artifact: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompareReport {
    pub left_capture_id: String,
    pub right_capture_id: String,
    pub source_aligned: bool,
    pub compiler_aligned: bool,
    pub settings_aligned: bool,
    pub alignment_differences: Vec<String>,
    pub setting_differences: Vec<String>,
    pub environment_differences: Vec<String>,
    pub measurement_differences: Vec<MeasurementDifference>,
    pub conclusion: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeasurementDifference {
    pub key: String,
    pub left: Option<String>,
    pub right: Option<String>,
}

pub fn report(file: &CaptureFile) -> Report {
    let stages = file
        .capture
        .stages
        .iter()
        .map(|stage| {
            let recorded_measurements = stage
                .measurements
                .iter()
                .map(|m| RecordedMeasurement {
                    name: m.name.clone(),
                    scope: m.scope.clone(),
                    quantity: m.quantity.clone(),
                    evidence: m.evidence.clone(),
                })
                .collect();
            let mut computed_scopes = Vec::new();
            if !stage.functions.is_empty() {
                let module_instructions = stage
                    .functions
                    .iter()
                    .try_fold(0u64, |sum, f| sum.checked_add(f.instructions))
                    .expect("validated sum");
                computed_scopes.push(ComputedScope {
                    scope: "all_module".into(),
                    instructions: module_instructions,
                    functions: stage.functions.len(),
                    complete: true,
                    unknown_indirect_callsites: 0,
                });
                for entry in &stage.selected_entries {
                    let one = reachable_union(stage, std::slice::from_ref(entry))
                        .expect("validated graph");
                    let body = stage
                        .functions
                        .iter()
                        .find(|f| f.id == *entry)
                        .expect("validated entry");
                    computed_scopes.push(ComputedScope {
                        scope: format!("root_body:{entry}"),
                        instructions: body.instructions,
                        functions: 1,
                        complete: true,
                        unknown_indirect_callsites: 0,
                    });
                    computed_scopes.push(ComputedScope {
                        scope: format!("reachable:{entry}"),
                        instructions: one.instructions,
                        functions: one.functions.len(),
                        complete: one.complete,
                        unknown_indirect_callsites: one.unknown_indirect_callsites,
                    });
                }
                if stage.selected_entries.len() > 1 {
                    let union =
                        reachable_union(stage, &stage.selected_entries).expect("validated graph");
                    computed_scopes.push(ComputedScope {
                        scope: "reachable_union:selected_entries".into(),
                        instructions: union.instructions,
                        functions: union.functions.len(),
                        complete: union.complete,
                        unknown_indirect_callsites: union.unknown_indirect_callsites,
                    });
                }
            }
            StageReport {
                id: stage.id.clone(),
                kind: stage.kind.clone(),
                recorded_measurements,
                computed_scopes,
            }
        })
        .collect();
    let helper_observations = file
        .capture
        .compatibility_observations
        .iter()
        .filter_map(helper_report)
        .collect::<Vec<_>>();
    let mut latest = BTreeMap::new();
    for helper in &helper_observations {
        latest.insert((helper.segment, helper.callee_name.clone()), helper.clone());
    }
    let unparsed_compiler_lines = file
        .capture
        .compatibility_observations
        .iter()
        .filter(|o| matches!(o, CompatibilityObservation::UnknownLine { .. }))
        .count();
    Report {
        schema: "riff-catalog-bloat-report/1".into(),
        capture_id: file.capture_id.clone(),
        label: file.capture.label.clone(),
        completion: file.capture.completion.clone(),
        intervention: file.capture.intervention.clone(),
        stages,
        helper_observations,
        latest_helper_observations: latest.into_values().collect(),
        inline_events: file.capture.inline_events.clone(),
        clone_observations: file.capture.clone_observations.clone(),
        decisions: file.capture.decisions.clone(),
        unparsed_compiler_lines,
        caveats: file.capture.provenance.notes.clone(),
    }
}

fn helper_report(value: &CompatibilityObservation) -> Option<HelperReport> {
    let CompatibilityObservation::HelperClones {
        segment,
        observation_stage,
        compiler_frontier,
        compiler_stage,
        callee_name,
        callsites,
        cloned_instructions,
        surviving_original_ids,
        line,
        artifact,
    } = value
    else {
        return None;
    };
    Some(HelperReport {
        segment: *segment,
        observation_stage: observation_stage.clone(),
        compiler_frontier: *compiler_frontier,
        compiler_stage: compiler_stage.clone(),
        callee_name: callee_name.clone(),
        callsites: *callsites,
        cloned_instructions_total: *cloned_instructions,
        surviving_original_ids: *surviving_original_ids,
        line: *line,
        artifact: artifact.clone(),
    })
}

pub fn render_table(report: &Report) -> String {
    let completion = match &report.completion {
        CaptureCompletion::Complete { producer_marker } => {
            format!("complete ({producer_marker})")
        }
        CaptureCompletion::Failed {
            message,
            last_stage,
        } => format!(
            "failed at {} ({message})",
            last_stage.as_deref().unwrap_or("unknown stage")
        ),
        CaptureCompletion::Incomplete { reason } => format!("incomplete ({reason})"),
        CaptureCompletion::LegacyUnknown => "legacy unknown".into(),
    };
    let intervention = if report.intervention.is_none() {
        "none".into()
    } else {
        format!(
            "{} requested=[{}] resolved={} consequential={}",
            report.intervention.kind,
            report.intervention.requested.join(","),
            report.intervention.resolved.len(),
            report.intervention.consequential.len()
        )
    };
    let mut out = format!(
        "capture\t{}\ncompletion\t{}\nintervention\t{}\nstructured\tinline_events={} clone_observations={} decisions={} (use --json for typed rows)\nstage\tmeasurement\tscope\tvalue\n",
        report.capture_id,
        completion,
        intervention,
        report.inline_events.len(),
        report.clone_observations.len(),
        report.decisions.len()
    );
    for stage in &report.stages {
        for m in &stage.recorded_measurements {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                stage.kind,
                m.name,
                short_scope(&m.scope),
                short_quantity(&m.quantity)
            ));
        }
        for c in &stage.computed_scopes {
            out.push_str(&format!(
                "{}\tcomputed_instructions\t{}\t{}{}\n",
                stage.kind,
                c.scope,
                c.instructions,
                if c.complete {
                    ""
                } else {
                    " (incomplete attribution)"
                }
            ));
        }
    }
    if !report.latest_helper_observations.is_empty() {
        out.push_str("latest helper census per segment (cumulative, never sum stages):\n");
        for h in &report.latest_helper_observations {
            out.push_str(&format!("segment {}\t{}\tfrontier {}/{}\tcallsites={} cloned_total={} surviving_original_ids={}\n",
                h.segment, h.callee_name, h.compiler_frontier, h.compiler_stage, h.callsites, h.cloned_instructions_total, h.surviving_original_ids));
        }
    }
    if report.unparsed_compiler_lines != 0 {
        out.push_str(&format!(
            "unparsed recognized compiler lines: {}\n",
            report.unparsed_compiler_lines
        ));
    }
    out
}

fn short_scope(scope: &MeasurementScope) -> String {
    serde_json::to_string(scope).expect("scope serializes")
}
fn short_quantity(quantity: &Quantity) -> String {
    serde_json::to_string(quantity).expect("quantity serializes")
}

pub fn compare(left: &CaptureFile, right: &CaptureFile) -> CompareReport {
    let source_aligned = known_equal(
        &left.capture.alignment.source_id,
        &right.capture.alignment.source_id,
    );
    let compiler_aligned = known_equal(
        &left.capture.alignment.compiler_id,
        &right.capture.alignment.compiler_id,
    );
    let mut alignment_differences = Vec::new();
    if !source_aligned {
        alignment_differences.push(format!(
            "source_id: {} -> {}",
            left.capture.alignment.source_id, right.capture.alignment.source_id
        ));
    }
    if !compiler_aligned {
        alignment_differences.push(format!(
            "compiler_id: {} -> {}",
            left.capture.alignment.compiler_id, right.capture.alignment.compiler_id
        ));
    }
    let setting_differences = map_differences(
        &left.capture.alignment.settings,
        &right.capture.alignment.settings,
    );
    let environment_differences = map_differences(
        &left.capture.provenance.environment,
        &right.capture.provenance.environment,
    );
    let settings_aligned = setting_differences.is_empty();
    let lm = measurement_map(left);
    let rm = measurement_map(right);
    let keys = lm.keys().chain(rm.keys()).collect::<BTreeSet<_>>();
    let measurement_differences = keys
        .into_iter()
        .filter_map(|key| {
            let l = lm.get(key);
            let r = rm.get(key);
            (l != r).then(|| MeasurementDifference {
                key: key.clone(),
                left: l.cloned(),
                right: r.cloned(),
            })
        })
        .collect();
    let conclusion = if source_aligned
        && compiler_aligned
        && settings_aligned
        && environment_differences.is_empty()
    {
        "Declared source, compiler, settings, and environment align. This does not establish causality.".into()
    } else {
        "Captures are not fully aligned. Differences cannot be attributed to one compiler policy from this evidence.".into()
    };
    CompareReport {
        left_capture_id: left.capture_id.clone(),
        right_capture_id: right.capture_id.clone(),
        source_aligned,
        compiler_aligned,
        settings_aligned,
        alignment_differences,
        setting_differences,
        environment_differences,
        measurement_differences,
        conclusion,
    }
}

fn known_equal(left: &str, right: &str) -> bool {
    left != "unknown" && right != "unknown" && left == right
}
fn map_differences(
    left: &BTreeMap<String, String>,
    right: &BTreeMap<String, String>,
) -> Vec<String> {
    left.keys()
        .chain(right.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|key| {
            let l = left.get(key);
            let r = right.get(key);
            (l != r).then(|| {
                format!(
                    "{key}: {} -> {}",
                    l.map_or("<absent>", String::as_str),
                    r.map_or("<absent>", String::as_str)
                )
            })
        })
        .collect()
}
fn measurement_map(file: &CaptureFile) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for stage in &file.capture.stages {
        for m in &stage.measurements {
            let key = serde_json::to_string(&(
                stage.id.as_str(),
                "recorded_measurement",
                m.name.as_str(),
                &m.scope,
            ))
            .expect("measurement comparison key serializes");
            out.insert(key, short_quantity(&m.quantity));
        }
        let graph_key = serde_json::to_string(&(stage.id.as_str(), "graph_evidence"))
            .expect("graph comparison key serializes");
        let graph_value = serde_json::to_string(&(
            &stage.call_graph,
            &stage.selected_entries,
            &stage.direct_calls,
            &stage.unknown_indirect_calls,
        ))
        .expect("graph comparison evidence serializes");
        out.insert(graph_key, graph_value);
    }
    out.insert(
        serde_json::to_string(&("capture", "completion_and_intervention")).expect("key serializes"),
        serde_json::to_string(&(
            &file.capture.completion,
            &file.capture.intervention,
            &file.capture.decisions,
            &file.capture.inline_events,
            &file.capture.clone_observations,
        ))
        .expect("capture evidence serializes"),
    );
    for stage in report(file).stages {
        for computed in stage.computed_scopes {
            let key = serde_json::to_string(&(
                stage.id.as_str(),
                "computed_scope",
                computed.scope.as_str(),
            ))
            .expect("computed comparison key serializes");
            let value = serde_json::to_string(&(
                computed.instructions,
                computed.functions,
                computed.complete,
                computed.unknown_indirect_callsites,
            ))
            .expect("computed comparison value serializes");
            out.insert(key, value);
        }
    }
    out
}
pub fn render_compare_table(report: &CompareReport) -> String {
    let mut out = format!(
        "source aligned: {}\ncompiler aligned: {}\nsettings aligned: {}\n",
        report.source_aligned, report.compiler_aligned, report.settings_aligned
    );
    for d in &report.setting_differences {
        out.push_str(&format!("setting: {d}\n"));
    }
    for d in &report.alignment_differences {
        out.push_str(&format!("alignment: {d}\n"));
    }
    for d in &report.environment_differences {
        out.push_str(&format!("environment: {d}\n"));
    }
    for d in &report.measurement_differences {
        out.push_str(&format!(
            "measurement: {}: {} -> {}\n",
            d.key,
            d.left.as_deref().unwrap_or("unknown"),
            d.right.as_deref().unwrap_or("unknown")
        ));
    }
    out.push_str(&report.conclusion);
    out.push('\n');
    out
}
