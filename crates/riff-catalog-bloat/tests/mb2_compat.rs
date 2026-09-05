use std::{collections::BTreeMap, fs, path::PathBuf, process::Command};

use riff_catalog_bloat::*;

struct Request(PathBuf);

impl Request {
    fn new(kind: &str, case: &str) -> Self {
        let root = PathBuf::from("/workspace/scratch").join(format!(
            "riffcat-mb2-compat-{}-{kind}-{case}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/mb2-observe")
            .join(kind);
        for name in ["events.jsonl", "shader.wgsl", "shader.spv"] {
            fs::copy(fixture.join(name), root.join(name)).unwrap();
        }
        Self(root)
    }

    fn import(&self) -> anyhow::Result<Capture> {
        import_fe_events(FeEventsImport {
            events: self.0.join("events.jsonl"),
            label: "mb2 typed recorder compatibility".into(),
            source_id: "sha256:cf552b2a58e8d263d8a4ac618a49a894f172704b6e6a2cd391147658e13bd1bd".into(),
            compiler_id: "fe:6cc22779a;patch-sha256:7b916085cb6f70f414b1ad8c1c97c6034ecc66c68828543c6ef11123b2d5092b;sonatina:ca5210d1ff41af48d893c82f2a8380ada3e3f5c6".into(),
            producer_revision: "6cc22779a+checkpoint-patch".into(),
            command: "archived MB2 capture, see fixture README".into(),
            settings: BTreeMap::new(),
        })
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn real_complete_and_budget_partial_replay_deterministically() {
    let mut censuses = Vec::new();
    for kind in ["complete", "partial"] {
        let request = Request::new(kind, "replay");
        let capture = request.import().unwrap();
        match (&capture.completion, kind) {
            (CaptureCompletion::Complete { .. }, "complete") => {}
            (CaptureCompletion::Incomplete { reason }, "partial") => {
                assert!(reason.contains("no completion marker"));
            }
            other => panic!("unexpected classification: {other:?}"),
        }
        let path = request.0.join("capture.json");
        let sealed = save_capture(&path, capture).unwrap();
        verify_artifacts(&path, &sealed).unwrap();
        let census_path = request.0.join("census.json");
        let census = save_census(
            &census_path,
            CensusSource::Capture {
                path: path.clone(),
                artifact_id: "shader.wgsl".into(),
                regions: None,
            },
        )
        .unwrap();
        assert_eq!(
            census.census.capture_context.as_ref().unwrap().completion,
            sealed.capture.completion
        );
        assert_eq!(
            replay_census(&census_path).unwrap().census_id,
            census.census_id
        );
        censuses.push(census);
        let replay = || {
            let output = Command::new(env!("CARGO_BIN_EXE_riffcat-bloat"))
                .arg("replay")
                .arg(&path)
                .arg("--json")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            output.stdout
        };
        assert_eq!(replay(), replay());
        assert_eq!(
            sealed.capture_id,
            capture_id(&request.import().unwrap()).unwrap()
        );
        for name in ["shader.wgsl", "shader.spv"] {
            let artifact = request.0.join(name);
            let original = fs::read(&artifact).unwrap();
            let mut altered = original.clone();
            altered[0] ^= 1;
            fs::write(&artifact, altered).unwrap();
            assert!(verify_artifacts(&path, &sealed).is_err());
            assert!(replay_census(&census_path).is_err());
            assert!(request.import().is_err());
            fs::write(&artifact, original).unwrap();
        }
    }
    let comparison = compare_censuses(&censuses[0], &censuses[1]);
    assert!(
        render_census_comparison(&comparison, 5).contains("WARNING: right capture is not complete")
    );
}

#[test]
fn raw_checkpoint_preserves_exact_output_under_budget_exhaustion() {
    let complete = Request::new("complete", "bytes");
    let partial = Request::new("partial", "bytes");
    for (name, size) in [("shader.wgsl", 518), ("shader.spv", 1300)] {
        let bytes = fs::read(complete.0.join(name)).unwrap();
        assert_eq!(bytes.len(), size);
        assert_eq!(bytes, fs::read(partial.0.join(name)).unwrap());
    }
    assert_eq!(
        fs::read_to_string(complete.0.join("events.jsonl"))
            .unwrap()
            .lines()
            .count(),
        23
    );
    assert_eq!(
        fs::read_to_string(partial.0.join("events.jsonl"))
            .unwrap()
            .lines()
            .count(),
        22
    );
}
