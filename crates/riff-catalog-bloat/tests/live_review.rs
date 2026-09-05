use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use riff_catalog_bloat::{Capture, CaptureCompletion, FeEventsImport, import_fe_events};

struct Request(PathBuf);

impl Request {
    fn fixture() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = PathBuf::from("/workspace/scratch").join(format!(
            "riffcat-live-review-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structured-request");
        for name in ["events.jsonl", "shader.wgsl"] {
            fs::copy(fixture.join(name), path.join(name)).unwrap();
        }
        Self(path)
    }

    fn rewrite(&self, edit: impl FnOnce(&mut Vec<serde_json::Value>)) {
        let text = fs::read_to_string(self.0.join("events.jsonl")).unwrap();
        let mut records = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        edit(&mut records);
        let text = records
            .iter()
            .map(|record| serde_json::to_string(record).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        fs::write(self.0.join("events.jsonl"), text).unwrap();
    }

    fn import(&self) -> anyhow::Result<Capture> {
        import_fe_events(FeEventsImport {
            events: self.0.join("events.jsonl"),
            label: "review".into(),
            source_id: "fixture".into(),
            compiler_id: "fixture".into(),
            producer_revision: "fixture".into(),
            command: "fixture".into(),
            settings: BTreeMap::new(),
        })
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove this test's isolated request directory");
    }
}

#[test]
fn same_length_artifact_change_is_not_resealed_as_valid_producer_evidence() {
    let request = Request::fixture();
    request.import().unwrap();
    let path = request.0.join("shader.wgsl");
    let mut bytes = fs::read(&path).unwrap();
    bytes[0] ^= 1;
    fs::write(path, bytes).unwrap();
    let error = request.import().unwrap_err().to_string();
    assert!(error.to_lowercase().contains("sha"), "{error}");
}

#[test]
fn truncation_before_completion_stays_incomplete() {
    let request = Request::fixture();
    request.rewrite(|records| {
        records.pop();
    });
    assert!(matches!(
        request.import().unwrap().completion,
        CaptureCompletion::Incomplete { .. }
    ));
}

#[test]
fn truncation_before_helper_resolution_is_not_a_false_policy_mismatch() {
    let request = Request::fixture();
    request.rewrite(|records| records.truncate(2));
    assert!(matches!(
        request.import().unwrap().completion,
        CaptureCompletion::Incomplete { .. }
    ));
}

#[test]
fn completion_cannot_name_a_nonexistent_stage() {
    let request = Request::fixture();
    request.rewrite(|records| records.last_mut().unwrap()["final_stage"] = "imaginary".into());
    let error = request.import().unwrap_err().to_string();
    assert!(
        error.contains("final stage") && error.contains("imaginary"),
        "{error}"
    );
}

#[test]
fn artifact_measurement_cannot_name_a_nonexistent_stage() {
    let request = Request::fixture();
    request.rewrite(|records| {
        let artifacts = records
            .iter_mut()
            .find(|record| record["event"] == "artifacts")
            .unwrap();
        artifacts["stage"] = "imaginary".into();
    });
    let error = request.import().unwrap_err().to_string();
    assert!(
        error.contains("artifact stage") && error.contains("imaginary"),
        "{error}"
    );
}
