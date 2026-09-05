use riff_catalog_bloat::{Capture, CaptureFile, SCHEMA_VERSION, capture_id, validate};
use serde_json::{Value, json};

fn capture(body: Value) -> CaptureFile {
    let capture: Capture = serde_json::from_value(body).expect("test body fits the capture schema");
    CaptureFile {
        schema: SCHEMA_VERSION.into(),
        capture_id: capture_id(&capture).unwrap(),
        capture,
    }
}

fn body() -> Value {
    json!({
        "label": "independent contract checks",
        "provenance": {
            "producer": "synthetic test",
            "producer_revision": "fixture-v1",
            "command": "none"
        },
        "alignment": {"source_id": "fixture", "compiler_id": "fixture"},
        "stages": [{
            "id": "before", "kind": "ir",
            "functions": [
                {"id": "root", "display_name": "root", "instructions": 3},
                {"id": "helper", "display_name": "helper", "instructions": 4}
            ],
            "selected_entries": ["root"]
        }, {
            "id": "after", "kind": "ir", "predecessors": ["before"],
            "functions": [
                {"id": "root", "display_name": "root", "instructions": 6},
                {"id": "helper", "display_name": "helper", "instructions": 4}
            ],
            "selected_entries": ["root"]
        }]
    })
}

#[test]
fn artifact_byte_evidence_must_agree_with_its_manifest() {
    let mut value = body();
    value["artifacts"] = json!([{
        "id": "shader", "role": "emitted_wgsl", "path": "shader.wgsl",
        "blake3": "0".repeat(64), "bytes": 7
    }]);
    value["stages"][1]["measurements"] = json!([{
        "name": "artifact_size",
        "scope": {"scope": "artifact", "artifact": "shader"},
        "quantity": {"unit": "bytes", "value": 999},
        "evidence": {"kind": "artifact_measurement", "artifact": "shader"}
    }]);
    assert!(
        validate(&capture(value)).is_err(),
        "an exact byte claim cannot contradict its own artifact record"
    );
}

#[test]
fn an_inline_event_cannot_run_backwards_in_the_stage_dag() {
    let mut value = body();
    value["inline_events"] = json!([{
        "id": "backwards",
        "caller": {"stage": "after", "function": "root"},
        "callee": {"stage": "after", "function": "helper"},
        "output_stage": "before", "callsites": 1, "cloned_instructions": 4,
        "surviving_original_ids": 4,
        "evidence": {"kind": "compiler_event", "producer": "synthetic test"}
    }]);
    assert!(
        validate(&capture(value)).is_err(),
        "referential validity is not sufficient for a transform event"
    );
}

#[test]
fn an_original_id_survival_count_cannot_exceed_the_original_clone_count() {
    let mut value = body();
    value["inline_events"] = json!([{
        "id": "overcount",
        "caller": {"stage": "before", "function": "root"},
        "callee": {"stage": "before", "function": "helper"},
        "output_stage": "after", "callsites": 1, "cloned_instructions": 4,
        "surviving_original_ids": 5,
        "evidence": {"kind": "compiler_event", "producer": "synthetic test"}
    }]);
    assert!(validate(&capture(value)).is_err());
}

#[test]
fn comparison_detects_graph_derived_growth_and_completeness_changes() {
    let mut baseline = body();
    baseline["stages"][0]["call_graph"] = json!({"completeness": "complete"});
    let before = capture(baseline.clone());
    validate(&before).unwrap();

    let mut changed = baseline.clone();
    changed["stages"][0]["functions"][0]["instructions"] = json!(30);
    let after = capture(changed);
    validate(&after).unwrap();
    assert!(
        !riff_catalog_bloat::compare(&before, &after)
            .measurement_differences
            .is_empty(),
        "comparison must include the graph-derived totals shown by report"
    );

    baseline["stages"][0]["call_graph"] = json!({
        "completeness": "incomplete", "reason": "some callees were not captured"
    });
    let incomplete = capture(baseline);
    validate(&incomplete).unwrap();
    assert!(
        !riff_catalog_bloat::compare(&before, &incomplete)
            .measurement_differences
            .is_empty(),
        "equal counts with different completeness are different evidence"
    );
}
