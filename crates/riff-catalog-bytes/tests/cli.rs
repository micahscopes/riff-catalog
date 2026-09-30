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
