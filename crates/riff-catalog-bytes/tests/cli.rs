//! Command-line tests of `riffcat-bytes`: refusals, schemas and whole
//! commands on small fixtures.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

/// A fresh scratch directory for one test.
fn scratch(name: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "riffcat-bytes-cli-{}-{}-{name}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_riffcat-bytes"))
        .args(args)
        .output()
        .expect("run riffcat-bytes")
}

/// Run and require failure; returns stderr.
fn refused(args: &[&str]) -> String {
    let out = run(args);
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        !out.status.success(),
        "{args:?} succeeded; stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    stderr
}

/// Run and require success; returns stdout.
fn ok(args: &[&str]) -> String {
    let out = run(args);
    assert!(
        out.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn write(dir: &Path, name: &str, text: &str) -> String {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path.to_str().unwrap().to_string()
}

#[test]
fn reports_without_the_expected_schema_are_refused() {
    let dir = scratch("schemas");
    // A byte-cause ledger from another schema version.
    let causes = write(
        &dir,
        "causes.json",
        r#"{"schema":"riffcat-evm-byte-causes/99","artifact_bytes":1,"code_end":1,"order":[],"buckets":{"x":{"bytes":1,"instructions":1}},"by_region":[]}"#,
    );
    let err = refused(&[
        "evm-byte-causes-compare",
        "--left",
        &causes,
        "--right",
        &causes,
    ]);
    assert!(err.contains("riffcat-evm-byte-causes/99"), "{err}");
    // A dataflow blocks file without a schema field.
    let blocks = write(
        &dir,
        "blocks.json",
        r#"{"code_bytes":0,"facets":{},"blocks":[]}"#,
    );
    let err = refused(&[
        "evm-dataflow-compare",
        "--left",
        &blocks,
        "--right",
        &blocks,
    ]);
    assert!(err.contains("blocks.json"), "{err}");
    // A sonatina-functions census in the old bare-array form.
    let code = write(&dir, "code.bin", "600000");
    let regions = write(
        &dir,
        "regions.json",
        &format!(
            r#"{{"schema":"riffcat-regions/1","artifact_blake3":"{}","adapter":"t/1","regions":[]}}"#,
            blake3::hash(&[0x60, 0x00, 0x00]).to_hex()
        ),
    );
    let bare = write(&dir, "son.json", "[]");
    let out = dir.join("bc.json");
    let err = refused(&[
        "evm-byte-causes",
        &code,
        "--regions",
        &regions,
        "--cause",
        &format!("duplicates:d={bare}#0"),
        "--json-out",
        out.to_str().unwrap(),
    ]);
    assert!(err.contains("son.json"), "{err}");
}

fn fixture(path: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(path);
    p.to_str().unwrap().to_string()
}

/// The Fe base fixture's regions manifest, written by fe-trace-bytes.
fn base_regions(dir: &Path) -> String {
    let manifest = dir.join("regions.json");
    let artifact = dir.join("runtime-copy.bin");
    ok(&[
        "fe-trace-bytes",
        "--trace",
        &fixture("fe-base/trace.jsonl"),
        "--attribution",
        &fixture("fe-base/details.json"),
        "--contract",
        "C",
        "--artifact",
        &fixture("fe-base/runtime.bin"),
        "--census-dir",
        dir.to_str().unwrap(),
    ]);
    std::fs::rename(dir.join("runtime.bin"), &artifact).unwrap();
    manifest.to_str().unwrap().to_string()
}

#[test]
fn region_manifests_for_another_artifact_are_refused() {
    let dir = scratch("regions");
    let regions = base_regions(&dir);
    // MSTORE8 where the fixture has MSTORE: same length, other bytes.
    let other = dir.join("other.bin");
    std::fs::write(&other, [0x60, 0x80, 0x53, 0x00]).unwrap();
    let other = other.to_str().unwrap();
    let out = dir.join("out.json");
    let out = out.to_str().unwrap();
    for args in [
        vec!["evm-dataflow", other, "--out", out, "--regions", &regions],
        vec![
            "evm-byte-causes",
            other,
            "--regions",
            &regions,
            "--json-out",
            out,
        ],
    ] {
        let err = refused(&args);
        assert!(
            err.contains("regions.json") && err.contains("blake3"),
            "{args:?}: {err}"
        );
    }
    // The same manifest with its own artifact is accepted.
    ok(&[
        "evm-dataflow",
        &fixture("fe-base/runtime.bin"),
        "--out",
        out,
        "--regions",
        &regions,
    ]);
}

#[test]
fn census_inputs_must_be_a_census_of_this_artifact_with_evm_runs() {
    let dir = scratch("census");
    let regions = base_regions(&dir);
    let runtime = fixture("fe-base/runtime.bin");
    let manifest: riff_catalog_bloat::RegionManifest =
        serde_json::from_slice(&std::fs::read(&regions).unwrap()).unwrap();
    let census = riff_catalog_bloat::census_regions(&[0x60, 0x80, 0x52, 0x00], manifest).unwrap();
    let census_path = write(
        &dir,
        "census.json",
        &serde_json::to_string(&census).unwrap(),
    );
    let other = dir.join("other.bin");
    std::fs::write(&other, [0x60, 0x80, 0x53, 0x00]).unwrap();
    let other_regions = write(
        &dir,
        "other-regions.json",
        &format!(
            r#"{{"schema":"riffcat-regions/1","artifact_blake3":"{}","adapter":"t/1","regions":[]}}"#,
            blake3::hash(&[0x60, 0x80, 0x53, 0x00]).to_hex()
        ),
    );
    let out = dir.join("out.json");
    let causes = |artifact: &str, regions: &str, census: &str| {
        vec![
            "evm-byte-causes".to_string(),
            artifact.to_string(),
            "--regions".into(),
            regions.to_string(),
            "--cause".into(),
            format!("repeats:r={census}"),
            "--json-out".into(),
            out.to_str().unwrap().to_string(),
        ]
    };
    let args = |v: &Vec<String>| v.clone();
    // A census of another artifact.
    let err = refused(
        &args(&causes(
            other.to_str().unwrap(),
            &other_regions,
            &census_path,
        ))
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>(),
    );
    assert!(err.contains("census.json"), "{err}");
    // A region manifest passed where a census is expected.
    let err = refused(
        &args(&causes(&runtime, &regions, &regions))
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    assert!(err.contains("regions.json"), "{err}");
    // A census of this artifact whose manifest asked for no EVM runs.
    let mut plain: riff_catalog_bloat::RegionManifest =
        serde_json::from_slice(&std::fs::read(&regions).unwrap()).unwrap();
    plain.evm_runs = None;
    plain.schema = "riffcat-regions/1".into();
    let plain = riff_catalog_bloat::census_regions(&[0x60, 0x80, 0x52, 0x00], plain).unwrap();
    let plain_path = write(&dir, "plain.json", &serde_json::to_string(&plain).unwrap());
    let err = refused(
        &args(&causes(&runtime, &regions, &plain_path))
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    assert!(err.contains("EVM run"), "{err}");
    // The census itself is accepted, and so is the census file that
    // `census --output` saves.
    ok(&causes(&runtime, &regions, &census_path)
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>());
    let sidecar = dir.join("sidecar.json");
    riff_catalog_bloat::save_census(
        &sidecar,
        riff_catalog_bloat::CensusSource::Artifact {
            path: runtime.clone().into(),
            regions: Some(regions.clone().into()),
        },
    )
    .unwrap();
    ok(&causes(&runtime, &regions, sidecar.to_str().unwrap())
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>());
}

fn base_args<'a>(command: &'a str, artifact: &'a str) -> Vec<String> {
    vec![
        command.to_string(),
        "--trace".into(),
        fixture("fe-base/trace.jsonl"),
        "--attribution".into(),
        fixture("fe-base/details.json"),
        "--contract".into(),
        "C".into(),
        "--artifact".into(),
        artifact.to_string(),
    ]
}

fn strs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

#[test]
fn fe_trace_stages_checks_the_trace_against_the_artifact() {
    let dir = scratch("stages");
    let regions = base_regions(&dir);
    let runtime = fixture("fe-base/runtime.bin");
    let mut args = base_args("fe-trace-stages", &runtime);
    args.extend(["--regions".to_string(), regions.clone()]);
    ok(&strs(&args));
    // Another artifact of the same length: the trace does not describe it
    // (a manifest for it is written so that only the trace can refuse it).
    let other = dir.join("other.bin");
    std::fs::write(&other, [0x60, 0x80, 0x53, 0x00]).unwrap();
    let other_regions = write(
        &dir,
        "other-regions.json",
        &format!(
            r#"{{"schema":"riffcat-regions/1","artifact_blake3":"{}","adapter":"t/1","regions":[]}}"#,
            blake3::hash(&[0x60, 0x80, 0x53, 0x00]).to_hex()
        ),
    );
    let mut args = base_args("fe-trace-stages", other.to_str().unwrap());
    args.extend(["--regions".to_string(), other_regions]);
    let err = refused(&strs(&args));
    assert!(err.contains("does not describe"), "{err}");
}

/// solc-functions on the committed solc output: the runtime, its manifest
/// and its report.
fn solc_build(dir: &Path) -> (String, String, String) {
    let artifact = dir.join("solc.bin");
    let manifest = dir.join("solc-regions.json");
    let report = dir.join("solc.json");
    ok(&[
        "solc-functions",
        "--standard-output",
        &fixture("solc/out-true.json"),
        "--source",
        "a.sol",
        "--contract",
        "A",
        "--artifact-out",
        artifact.to_str().unwrap(),
        "--manifest-out",
        manifest.to_str().unwrap(),
        "--json-out",
        report.to_str().unwrap(),
    ]);
    (
        artifact.to_str().unwrap().to_string(),
        manifest.to_str().unwrap().to_string(),
        report.to_str().unwrap().to_string(),
    )
}

#[test]
fn the_code_end_defaults_to_the_code_and_may_not_pass_the_artifact() {
    let dir = scratch("code-end");
    let (artifact, manifest, _) = solc_build(&dir);
    let blocks = dir.join("blocks.json");
    let blocks = blocks.to_str().unwrap();
    let code_bytes = |args: &[&str]| {
        ok(args);
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(blocks).unwrap()).unwrap();
        v["code_bytes"].as_u64().unwrap()
    };
    // 406 bytes, of which the last 53 are solc's CBOR metadata.
    assert_eq!(
        code_bytes(&["evm-dataflow", &artifact, "--out", blocks]),
        353
    );
    assert_eq!(
        code_bytes(&[
            "evm-dataflow",
            &artifact,
            "--out",
            blocks,
            "--regions",
            &manifest
        ]),
        353
    );
    assert_eq!(
        code_bytes(&[
            "evm-dataflow",
            &artifact,
            "--out",
            blocks,
            "--code-end",
            "100"
        ]),
        100
    );
    let err = refused(&[
        "evm-dataflow",
        &artifact,
        "--out",
        blocks,
        "--code-end",
        "407",
    ]);
    assert!(err.contains("407"), "{err}");
    let out = dir.join("bc.json");
    let err = refused(&[
        "evm-byte-causes",
        &artifact,
        "--regions",
        &manifest,
        "--code-end",
        "999",
        "--json-out",
        out.to_str().unwrap(),
    ]);
    assert!(err.contains("999"), "{err}");
    ok(&[
        "evm-byte-causes",
        &artifact,
        "--regions",
        &manifest,
        "--json-out",
        out.to_str().unwrap(),
    ]);
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(v["code_end"], 353);
}

#[test]
fn attribution_rows_must_be_the_artifacts_instructions() {
    let dir = scratch("rows");
    let details = fixture("fe-base/details.json");
    let out = dir.join("out.json");
    let out = out.to_str().unwrap();
    // PUSH2 where the fixture has PUSH1: the rows no longer fall on
    // instruction boundaries. And a longer artifact the rows do not cover.
    for (name, bytes) in [
        ("shifted.bin", vec![0x61u8, 0x80, 0x52, 0x00]),
        ("longer.bin", vec![0x60u8, 0x80, 0x52, 0x00, 0x00]),
    ] {
        let path = dir.join(name);
        std::fs::write(&path, &bytes).unwrap();
        let regions = write(
            &dir,
            &format!("{name}.regions.json"),
            &format!(
                r#"{{"schema":"riffcat-regions/1","artifact_blake3":"{}","adapter":"t/1","regions":[]}}"#,
                blake3::hash(&bytes).to_hex()
            ),
        );
        let path = path.to_str().unwrap();
        for args in [
            vec![
                "evm-dataflow",
                path,
                "--out",
                out,
                "--attribution",
                &details,
                "--contract",
                "C",
            ],
            vec![
                "evm-byte-causes",
                path,
                "--regions",
                &regions,
                "--attribution",
                &details,
                "--contract",
                "C",
                "--json-out",
                out,
            ],
        ] {
            let err = refused(&args);
            assert!(err.contains("details.json"), "{name} {args:?}: {err}");
        }
    }
    ok(&[
        "evm-dataflow",
        &fixture("fe-base/runtime.bin"),
        "--out",
        out,
        "--attribution",
        &details,
        "--contract",
        "C",
    ]);
}

#[test]
fn causes_must_name_instructions_once() {
    let dir = scratch("causes");
    let regions = base_regions(&dir);
    let runtime = fixture("fe-base/runtime.bin");
    let out = dir.join("out.json");
    let out = out.to_str().unwrap();
    let run_causes = |causes: &[String]| {
        let mut args = vec![
            "evm-byte-causes".to_string(),
            runtime.clone(),
            "--regions".into(),
            regions.clone(),
            "--json-out".into(),
            out.to_string(),
        ];
        for c in causes {
            args.extend(["--cause".to_string(), c.clone()]);
        }
        args
    };
    // pc 1 is inside PUSH1's immediate, 99 is past the code, 2^32 is not a
    // pc at all.
    for pcs in ["[1]", "[99]", "[4294967296]", r#"{"entries":[{"pc":"x"}]}"#] {
        let file = write(&dir, "pcs.json", pcs);
        let err = refused(&strs(&run_causes(&[format!("pcs:p={file}")])));
        assert!(err.contains("pcs.json"), "{pcs}: {err}");
    }
    let good = write(&dir, "good.json", "[0, 2]");
    ok(&strs(&run_causes(&[format!("pcs:p={good}")])));
    // Two causes with one name, a cause named like a role bucket, and an
    // empty name in a needle list.
    for causes in [
        vec![format!("pcs:x={good}"), "pattern:x=6080".to_string()],
        vec![format!("pcs:role: stack shuffle (DUP, SWAP, POP)={good}")],
        vec!["regions:r=C|".to_string()],
    ] {
        let err = refused(&strs(&run_causes(&causes)));
        assert!(!err.is_empty(), "{causes:?}");
    }
    // fe-trace-stages takes pc sets too.
    let bad = write(&dir, "bad.json", "[1]");
    let mut args = base_args("fe-trace-stages", &runtime);
    args.extend([
        "--regions".to_string(),
        regions.clone(),
        "--pc-set".into(),
        format!("s={bad}"),
    ]);
    let err = refused(&strs(&args));
    assert!(err.contains("bad.json"), "{err}");
}
