use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, anyhow, bail};

use crate::*;

pub fn validate(file: &CaptureFile) -> Result<()> {
    if file.schema != SCHEMA_VERSION {
        bail!("unsupported schema `{}`", file.schema);
    }
    if file.capture_id != capture_id(&file.capture)? {
        bail!("capture ID does not match the immutable capture body");
    }
    nonempty("capture label", &file.capture.label)?;
    nonempty("source ID", &file.capture.alignment.source_id)?;
    nonempty("compiler ID", &file.capture.alignment.compiler_id)?;

    let artifacts = unique(
        file.capture.artifacts.iter().map(|a| a.id.as_str()),
        "artifact",
    )?;
    for artifact in &file.capture.artifacts {
        nonempty("artifact path", &artifact.path)?;
        if artifact.blake3.len() != 64 || !artifact.blake3.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("artifact `{}` has an invalid blake3 digest", artifact.id);
        }
    }

    let stage_ids = unique(file.capture.stages.iter().map(|s| s.id.as_str()), "stage")?;
    let stage_map = file
        .capture
        .stages
        .iter()
        .map(|s| (s.id.as_str(), s))
        .collect::<BTreeMap<_, _>>();
    for stage in &file.capture.stages {
        let funcs = unique(stage.functions.iter().map(|f| f.id.as_str()), "function")?;
        let preds = unique(stage.predecessors.iter().map(String::as_str), "predecessor")?;
        unique(
            stage.selected_entries.iter().map(String::as_str),
            "selected entry",
        )?;
        stage
            .functions
            .iter()
            .try_fold(0u64, |sum, function| sum.checked_add(function.instructions))
            .ok_or_else(|| anyhow!("stage `{}` module instruction total overflows", stage.id))?;
        if let CallGraphCompleteness::Incomplete { reason } = &stage.call_graph {
            nonempty("incomplete call graph reason", reason)?;
        }
        for pred in preds {
            if pred == stage.id || !stage_ids.contains(pred) {
                bail!("stage `{}` has invalid predecessor `{pred}`", stage.id);
            }
        }
        for entry in &stage.selected_entries {
            require_func(stage, &funcs, entry, "selected entry")?;
        }
        let mut direct_facts = BTreeSet::new();
        for call in &stage.direct_calls {
            require_func(stage, &funcs, &call.caller, "call caller")?;
            require_func(stage, &funcs, &call.callee, "call callee")?;
            if call.callsites == 0 {
                bail!("stage `{}` has a zero-callsite direct edge", stage.id);
            }
            if !direct_facts.insert((&call.caller, &call.callee)) {
                bail!("stage `{}` has duplicate direct-call facts", stage.id);
            }
        }
        for call in &stage.unknown_indirect_calls {
            require_func(stage, &funcs, &call.caller, "indirect caller")?;
            if call.callsites == 0 || call.reason.is_empty() {
                bail!("stage `{}` has an invalid unknown indirect call", stage.id);
            }
        }
        let mut measurement_keys = BTreeSet::new();
        for measurement in &stage.measurements {
            let key = (
                measurement.name.as_str(),
                serde_json::to_string(&measurement.scope)?,
            );
            if !measurement_keys.insert(key) {
                bail!(
                    "stage `{}` has conflicting duplicate measurement `{}`",
                    stage.id,
                    measurement.name
                );
            }
            validate_scope(stage, &funcs, &artifacts, &measurement.scope)?;
            validate_evidence(&artifacts, &stage_ids, &measurement.evidence)?;
            if let (MeasurementScope::Artifact { artifact }, Quantity::Bytes(claimed)) =
                (&measurement.scope, &measurement.quantity)
            {
                let manifest = file
                    .capture
                    .artifacts
                    .iter()
                    .find(|item| item.id == *artifact)
                    .expect("artifact reference checked");
                if *claimed != manifest.bytes {
                    bail!(
                        "artifact measurement for `{artifact}` claims {claimed} bytes but manifest records {}",
                        manifest.bytes
                    );
                }
            }
        }
        // This also checks arithmetic overflow and exercises cycle-safe union accounting.
        if !stage.selected_entries.is_empty() {
            reachable_union(stage, &stage.selected_entries)?;
        }
    }
    validate_dag(&stage_map)?;

    let event_ids = unique(
        file.capture.inline_events.iter().map(|e| e.id.as_str()),
        "inline event",
    )?;
    let mut event_facts = BTreeSet::new();
    for event in &file.capture.inline_events {
        let _ = &event_ids;
        validate_function_ref(&stage_map, &event.caller)?;
        validate_function_ref(&stage_map, &event.callee)?;
        if !stage_ids.contains(event.output_stage.as_str()) {
            bail!(
                "inline event `{}` references missing output stage `{}`",
                event.id,
                event.output_stage
            );
        }
        if !is_strict_ancestor(&event.caller.stage, &event.output_stage, &stage_map)
            || !is_strict_ancestor(&event.callee.stage, &event.output_stage, &stage_map)
        {
            bail!(
                "inline event `{}` output stage does not follow its caller and callee stages",
                event.id
            );
        }
        if event.callsites == 0 {
            bail!("inline event `{}` has zero callsites", event.id);
        }
        if event
            .surviving_original_ids
            .is_some_and(|surviving| surviving > event.cloned_instructions)
        {
            bail!(
                "inline event `{}` has more surviving original IDs than total cloned instructions",
                event.id
            );
        }
        let fact = (
            &event.caller.stage,
            &event.caller.function,
            &event.callee.stage,
            &event.callee.function,
            &event.output_stage,
        );
        if !event_facts.insert(fact) {
            bail!("conflicting inline events describe the same stage-local fact");
        }
        validate_evidence(&artifacts, &stage_ids, &event.evidence)?;
    }
    let mut compatibility_facts = BTreeSet::new();
    for observation in &file.capture.compatibility_observations {
        match observation {
            CompatibilityObservation::HelperClones {
                observation_stage,
                compiler_frontier,
                compiler_stage,
                callee_name,
                artifact,
                cloned_instructions,
                surviving_original_ids,
                ..
            } => {
                if !stage_ids.contains(observation_stage.as_str()) {
                    bail!("helper observation references missing stage `{observation_stage}`");
                }
                if !artifacts.contains(artifact.as_str()) {
                    bail!("helper observation references missing artifact `{artifact}`");
                }
                if surviving_original_ids > cloned_instructions {
                    bail!("helper observation survival exceeds cloned instructions");
                }
                if compiler_stage.is_empty() || callee_name.is_empty() {
                    bail!("helper observation has an empty compiler stage or callee");
                }
                if !compatibility_facts.insert((
                    observation_stage,
                    compiler_frontier,
                    compiler_stage,
                    callee_name,
                )) {
                    bail!("duplicate helper census observation for the same stage and callee");
                }
            }
            CompatibilityObservation::UnknownLine { artifact, .. } => {
                if !artifacts.contains(artifact.as_str()) {
                    bail!("unknown line references missing artifact `{artifact}`");
                }
            }
        }
    }
    Ok(())
}

fn nonempty(label: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("{label} must not be empty");
    }
    Ok(())
}

fn unique<'a>(items: impl Iterator<Item = &'a str>, label: &str) -> Result<BTreeSet<&'a str>> {
    let mut out = BTreeSet::new();
    for item in items {
        nonempty(label, item)?;
        if !out.insert(item) {
            bail!("duplicate {label} ID `{item}`");
        }
    }
    Ok(out)
}

fn require_func(stage: &Stage, funcs: &BTreeSet<&str>, id: &str, role: &str) -> Result<()> {
    if !funcs.contains(id) {
        bail!(
            "stage `{}` {role} references missing function `{id}`",
            stage.id
        );
    }
    Ok(())
}

fn validate_scope(
    stage: &Stage,
    funcs: &BTreeSet<&str>,
    artifacts: &BTreeSet<&str>,
    scope: &MeasurementScope,
) -> Result<()> {
    match scope {
        MeasurementScope::RootBody { entry }
        | MeasurementScope::FunctionBody { function: entry } => {
            require_func(stage, funcs, entry, "measurement")?
        }
        MeasurementScope::ReachableUnion { entries } => {
            reachable_union(stage, entries)?;
        }
        MeasurementScope::Artifact { artifact } if !artifacts.contains(artifact.as_str()) => {
            bail!("measurement references missing artifact `{artifact}`")
        }
        _ => {}
    }
    Ok(())
}

fn validate_evidence(
    artifacts: &BTreeSet<&str>,
    stages: &BTreeSet<&str>,
    evidence: &Evidence,
) -> Result<()> {
    match evidence {
        Evidence::ArtifactMeasurement { artifact }
        | Evidence::CompatibilityTrace { artifact, .. }
            if !artifacts.contains(artifact.as_str()) =>
        {
            bail!("evidence references missing artifact `{artifact}`")
        }
        Evidence::DerivedCallGraph { stage } if !stages.contains(stage.as_str()) => {
            bail!("evidence references missing stage `{stage}`")
        }
        _ => Ok(()),
    }
}

fn validate_function_ref(stages: &BTreeMap<&str, &Stage>, reference: &FunctionRef) -> Result<()> {
    let stage = stages
        .get(reference.stage.as_str())
        .ok_or_else(|| anyhow!("missing stage `{}`", reference.stage))?;
    if !stage.functions.iter().any(|f| f.id == reference.function) {
        bail!(
            "stage `{}` has no function `{}`",
            reference.stage,
            reference.function
        );
    }
    Ok(())
}

fn validate_dag(stages: &BTreeMap<&str, &Stage>) -> Result<()> {
    fn visit<'a>(
        id: &'a str,
        stages: &BTreeMap<&'a str, &'a Stage>,
        active: &mut BTreeSet<&'a str>,
        done: &mut BTreeSet<&'a str>,
    ) -> Result<()> {
        if done.contains(id) {
            return Ok(());
        }
        if !active.insert(id) {
            bail!("stage predecessor cycle contains `{id}`");
        }
        for pred in &stages[id].predecessors {
            visit(pred, stages, active, done)?;
        }
        active.remove(id);
        done.insert(id);
        Ok(())
    }
    let mut active = BTreeSet::new();
    let mut done = BTreeSet::new();
    for id in stages.keys() {
        visit(id, stages, &mut active, &mut done)?;
    }
    Ok(())
}

fn is_strict_ancestor(ancestor: &str, descendant: &str, stages: &BTreeMap<&str, &Stage>) -> bool {
    if ancestor == descendant {
        return false;
    }
    let mut pending = vec![descendant];
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(stage) = stages.get(id) else {
            continue;
        };
        for predecessor in &stage.predecessors {
            if predecessor == ancestor {
                return true;
            }
            pending.push(predecessor);
        }
    }
    false
}

pub fn reachable_union(stage: &Stage, entries: &[String]) -> Result<Reachability> {
    let functions = stage
        .functions
        .iter()
        .map(|f| (f.id.as_str(), f))
        .collect::<BTreeMap<_, _>>();
    let mut pending = entries.to_vec();
    let mut seen = BTreeSet::new();
    let mut instructions = 0u64;
    let mut unknown = 0u64;
    while let Some(id) = pending.pop() {
        let function = functions.get(id.as_str()).ok_or_else(|| {
            anyhow!(
                "reachable entry/callee `{id}` is missing in stage `{}`",
                stage.id
            )
        })?;
        if !seen.insert(id.clone()) {
            continue;
        }
        instructions = instructions
            .checked_add(function.instructions)
            .ok_or_else(|| anyhow!("reachable instruction count overflows"))?;
        for call in stage.direct_calls.iter().filter(|c| c.caller == id) {
            pending.push(call.callee.clone());
        }
        for call in stage
            .unknown_indirect_calls
            .iter()
            .filter(|c| c.caller == id)
        {
            unknown = unknown
                .checked_add(call.callsites)
                .ok_or_else(|| anyhow!("unknown call count overflows"))?;
        }
    }
    let declared_complete = matches!(stage.call_graph, CallGraphCompleteness::Complete);
    Ok(Reachability {
        entries: entries.to_vec(),
        functions: seen.into_iter().collect(),
        instructions,
        complete: declared_complete && unknown == 0,
        unknown_indirect_callsites: unknown,
    })
}
