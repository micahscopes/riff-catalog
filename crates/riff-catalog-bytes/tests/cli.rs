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
