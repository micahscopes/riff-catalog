use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use riff_catalog_bloat::*;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn general_capture() -> Capture {
    serde_json::from_slice(
        &fs::read(fixture("general-shared-recursive.capture-body.json")).unwrap(),
    )
    .unwrap()
}

fn scratch(name: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    PathBuf::from("/workspace/scratch").join(format!(
        "riffcat-bloat-test-{}-{}-{}-{name}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn shared_helpers_are_counted_once_cycles_terminate_and_dead_code_is_excluded() {
    let capture = general_capture();
    let stage = &capture.stages[0];
    let union = reachable_union(stage, &stage.selected_entries).unwrap();
    assert_eq!(union.instructions, 36);
    assert_eq!(
        union.functions,
        ["entry-a", "entry-b", "recursive", "shared"]
    );
    assert!(union.complete);
    assert!(!union.functions.contains(&"dead".to_owned()));
}

#[test]
fn unknown_target_is_visible_and_attribution_is_incomplete() {
    let capture = general_capture();
    let stage = &capture.stages[1];
    let union = reachable_union(stage, &stage.selected_entries).unwrap();
    assert_eq!(union.instructions, 3);
    assert_eq!(union.unknown_indirect_callsites, 1);
    assert!(!union.complete);
}

#[test]
fn fe_import_segments_runs_and_retains_literal_cumulative_clone_observations() {
    let capture = import_fe_trace(FeImport {
        trace: fixture("fe-realistic.stderr"),
        wgsl: None,
        label: "fixture".into(),
        source_id: "fixture-source".into(),
        compiler_id: "fixture-fe".into(),
        producer_revision: "fixture-rev".into(),
        command: "fixture command".into(),
        settings: BTreeMap::new(),
        environment: BTreeMap::new(),
    })
    .unwrap();
    assert!(
        capture
            .stages
            .iter()
            .any(|s| s.id == "segment-2-post-merge")
    );
    let helpers = capture
        .compatibility_observations
        .iter()
        .filter(|o| matches!(o, CompatibilityObservation::HelperClones { .. }))
        .count();
    assert_eq!(helpers, 3);
    assert!(
        capture.inline_events.is_empty(),
        "stderr lacks caller IDs and must not fabricate general inline events"
    );
    assert!(
        capture
            .provenance
            .notes
            .iter()
            .any(|n| n.contains("not prove"))
    );
    let producer = capture
        .stages
        .iter()
        .flat_map(|s| &s.measurements)
        .find(|m| m.name == "producer_wgsl_bytes")
        .unwrap();
    assert_eq!(producer.quantity, Quantity::Bytes(408158));
    let shipped = capture
        .stages
        .iter()
        .flat_map(|s| &s.measurements)
        .find(|m| m.name == "reported_shipped_wgsl_bytes")
        .unwrap();
    assert_eq!(shipped.quantity, Quantity::Bytes(408152));
}

#[test]
fn predecessor_cycles_and_missing_ids_are_rejected() {
    let mut capture = general_capture();
    capture.stages[0].predecessors.push("unknown-calls".into());
    let file = CaptureFile {
        schema: SCHEMA_VERSION.into(),
        capture_id: capture_id(&capture).unwrap(),
        capture,
    };
    assert!(validate(&file).unwrap_err().to_string().contains("cycle"));

    let mut capture = general_capture();
    capture.stages[0].direct_calls[0].callee = "missing".into();
    let file = CaptureFile {
        schema: SCHEMA_VERSION.into(),
        capture_id: capture_id(&capture).unwrap(),
        capture,
    };
    assert!(validate(&file).is_err());
}

#[test]
fn rewritten_clone_survival_is_literal_and_cannot_exceed_clones() {
    let mut capture = general_capture();
    capture.inline_events.push(InlineEvent {
        id: "inline-1".into(),
        caller: FunctionRef {
            stage: "lowered".into(),
            function: "entry-a".into(),
        },
        callee: FunctionRef {
            stage: "lowered".into(),
            function: "shared".into(),
        },
        output_stage: "unknown-calls".into(),
        callsites: 1,
        cloned_instructions: 10,
        surviving_original_ids: Some(11),
        evidence: Evidence::CompilerEvent {
            producer: "fixture".into(),
        },
    });
    let file = CaptureFile {
        schema: SCHEMA_VERSION.into(),
        capture_id: capture_id(&capture).unwrap(),
        capture,
    };
    assert!(
        validate(&file)
            .unwrap_err()
            .to_string()
            .contains("surviving original")
    );
}

#[test]
fn nested_attribution_events_are_rows_not_an_additive_inclusive_metric() {
    let mut capture = general_capture();
    capture.inline_events.push(InlineEvent {
        id: "outer".into(),
        caller: FunctionRef {
            stage: "lowered".into(),
            function: "entry-a".into(),
        },
        callee: FunctionRef {
            stage: "lowered".into(),
            function: "shared".into(),
        },
        output_stage: "unknown-calls".into(),
        callsites: 1,
        cloned_instructions: 20,
        surviving_original_ids: Some(15),
        evidence: Evidence::CompilerEvent {
            producer: "fixture".into(),
        },
    });
    let file = CaptureFile {
        schema: SCHEMA_VERSION.into(),
        capture_id: capture_id(&capture).unwrap(),
        capture,
    };
    validate(&file).unwrap();
    assert_eq!(file.capture.inline_events[0].cloned_instructions, 20);
    assert!(report(&file).stages.iter().all(|stage| {
        stage
            .recorded_measurements
            .iter()
            .all(|m| m.name != "inclusive_clone_sum")
    }));
}

#[test]
fn sealing_and_reports_are_repeatable_and_artifact_tampering_fails() {
    let artifact = scratch("artifact.wgsl");
    let output = scratch("capture.json");
    fs::write(&artifact, "fn main() {}\n").unwrap();
    let (digest, bytes) = artifact_digest(&artifact).unwrap();
    let mut capture = general_capture();
    capture.artifacts.push(Artifact {
        id: "wgsl".into(),
        role: ArtifactRole::EmittedWgsl,
        path: artifact.display().to_string(),
        blake3: digest,
        bytes,
        producer_digests: BTreeMap::new(),
    });
    let first = save_capture(&output, capture.clone()).unwrap();
    let second = save_capture(&output, capture).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        serde_json::to_vec(&report(&first)).unwrap(),
        serde_json::to_vec(&report(&load_capture(&output).unwrap())).unwrap()
    );
    verify_artifacts(&output, &first).unwrap();
    fs::write(&artifact, "fn changed() {}\n").unwrap();
    assert!(verify_artifacts(&output, &first).is_err());
    fs::remove_file(artifact).unwrap();
    fs::remove_file(output).unwrap();
}

#[test]
fn strict_numeric_and_duplicate_fact_validation_fails_closed() {
    let fixture =
        fs::read_to_string(fixture("general-shared-recursive.capture-body.json")).unwrap();
    let negative = fixture.replacen("\"instructions\": 5", "\"instructions\": -1", 1);
    assert!(serde_json::from_str::<Capture>(&negative).is_err());
    let nonfinite = fixture.replacen("\"instructions\": 5", "\"instructions\": 1e999", 1);
    assert!(serde_json::from_str::<Capture>(&nonfinite).is_err());

    let mut capture = general_capture();
    capture.stages[0].functions[0].instructions = u64::MAX;
    let file = CaptureFile {
        schema: SCHEMA_VERSION.into(),
        capture_id: capture_id(&capture).unwrap(),
        capture,
    };
    assert!(
        validate(&file)
            .unwrap_err()
            .to_string()
            .contains("overflows")
    );

    let mut capture = general_capture();
    let duplicate = capture.stages[0].direct_calls[0].clone();
    capture.stages[0].direct_calls.push(duplicate);
    let file = CaptureFile {
        schema: SCHEMA_VERSION.into(),
        capture_id: capture_id(&capture).unwrap(),
        capture,
    };
    assert!(
        validate(&file)
            .unwrap_err()
            .to_string()
            .contains("duplicate direct-call")
    );
}

#[test]
fn structured_fe_events_import_graphs_decisions_clones_and_exact_artifacts() {
    let capture = import_fe_events(FeEventsImport {
        events: fixture("structured-request/events.jsonl"),
        label: "structured fixture".into(),
        source_id: "sha256:fixture-source".into(),
        compiler_id: "fe:fixture".into(),
        producer_revision: "fixture-revision".into(),
        command: "fixture command".into(),
        settings: BTreeMap::new(),
    })
    .unwrap();
    assert!(matches!(
        capture.completion,
        CaptureCompletion::Complete { .. }
    ));
    assert_eq!(capture.intervention.requested, ["mix_words"]);
    assert_eq!(capture.intervention.resolved.len(), 1);
    assert_eq!(capture.inline_events.len(), 1);
    assert_eq!(capture.clone_observations.len(), 1);
    assert!(capture.stages.iter().any(|stage| {
        stage.measurements.iter().any(|measurement| {
            measurement.name == "inliner_calls_spliced"
                && measurement.quantity == Quantity::Callsites(1)
        })
    }));
    assert!(
        capture
            .decisions
            .iter()
            .any(|decision| matches!(decision.decision, DecisionKind::ForcedInline))
    );
    assert!(
        capture
            .artifacts
            .iter()
            .any(|artifact| artifact.role == ArtifactRole::EmittedWgsl && artifact.bytes == 73)
    );
    let file = CaptureFile {
        schema: SCHEMA_VERSION.into(),
        capture_id: capture_id(&capture).unwrap(),
        capture,
    };
    validate(&file).unwrap();
}

#[test]
fn structured_event_import_is_fail_visible_for_truncation_and_bad_sequence() {
    let original = fs::read_to_string(fixture("structured-request/events.jsonl")).unwrap();
    let truncated_path = scratch("truncated-events.jsonl");
    let mut lines = original.lines().collect::<Vec<_>>();
    lines.pop();
    lines.pop();
    fs::write(&truncated_path, format!("{}\n", lines.join("\n"))).unwrap();
    let imported = import_fe_events(FeEventsImport {
        events: truncated_path.clone(),
        label: "truncated".into(),
        source_id: "fixture".into(),
        compiler_id: "fixture".into(),
        producer_revision: "fixture".into(),
        command: "fixture".into(),
        settings: BTreeMap::new(),
    })
    .unwrap();
    assert!(matches!(
        imported.completion,
        CaptureCompletion::Incomplete { .. }
    ));
    let imported_file = CaptureFile {
        schema: SCHEMA_VERSION.into(),
        capture_id: capture_id(&imported).unwrap(),
        capture: imported,
    };
    assert!(render_table(&report(&imported_file)).contains("completion\tincomplete"));
    let mut failed_file = imported_file.clone();
    failed_file.capture.completion = CaptureCompletion::Failed {
        message: "fixture failure".into(),
        last_stage: None,
    };
    failed_file.capture_id = capture_id(&failed_file.capture).unwrap();
    assert!(render_table(&report(&failed_file)).contains("completion\tfailed"));
    fs::remove_file(&truncated_path).unwrap();

    let bad_path = scratch("bad-sequence-events.jsonl");
    fs::write(
        &bad_path,
        original.replacen("\"sequence\":1", "\"sequence\":8", 1),
    )
    .unwrap();
    assert!(
        import_fe_events(FeEventsImport {
            events: bad_path.clone(),
            label: "bad".into(),
            source_id: "fixture".into(),
            compiler_id: "fixture".into(),
            producer_revision: "fixture".into(),
            command: "fixture".into(),
            settings: BTreeMap::new()
        })
        .is_err()
    );
    fs::remove_file(bad_path).unwrap();
}
