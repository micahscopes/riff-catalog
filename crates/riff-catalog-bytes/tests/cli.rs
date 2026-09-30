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
        r#"{"schema":"riffcat-evm-byte-causes/99","artifact_blake3":"x","artifact_bytes":1,"code_end":1,"order":[],"buckets":{"x":{"bytes":1,"instructions":1}},"by_region":[]}"#,
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
        "--artifact",
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
        "--artifact-out",
        artifact.to_str().unwrap(),
        "--manifest-out",
        manifest.to_str().unwrap(),
    ]);
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
        vec![
            "evm-dataflow",
            "--artifact",
            other,
            "--blocks-out",
            out,
            "--regions",
            &regions,
        ],
        vec![
            "evm-byte-causes",
            "--artifact",
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
        "--artifact",
        &fixture("fe-base/runtime.bin"),
        "--blocks-out",
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
            "--artifact".into(),
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
        code_bytes(&[
            "evm-dataflow",
            "--artifact",
            &artifact,
            "--blocks-out",
            blocks
        ]),
        353
    );
    assert_eq!(
        code_bytes(&[
            "evm-dataflow",
            "--artifact",
            &artifact,
            "--blocks-out",
            blocks,
            "--regions",
            &manifest
        ]),
        353
    );
    assert_eq!(
        code_bytes(&[
            "evm-dataflow",
            "--artifact",
            &artifact,
            "--blocks-out",
            blocks,
            "--code-end",
            "100"
        ]),
        100
    );
    let err = refused(&[
        "evm-dataflow",
        "--artifact",
        &artifact,
        "--blocks-out",
        blocks,
        "--code-end",
        "407",
    ]);
    assert!(err.contains("407"), "{err}");
    let out = dir.join("bc.json");
    let err = refused(&[
        "evm-byte-causes",
        "--artifact",
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
        "--artifact",
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
                "--artifact",
                path,
                "--blocks-out",
                out,
                "--attribution",
                &details,
                "--contract",
                "C",
            ],
            vec![
                "evm-byte-causes",
                "--artifact",
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
        "--artifact",
        &fixture("fe-base/runtime.bin"),
        "--blocks-out",
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
            "--artifact".into(),
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

#[test]
fn byte_cause_comparisons_recheck_each_ledger_and_print_no_nan() {
    let dir = scratch("compare-causes");
    let regions = base_regions(&dir);
    let ledger = dir.join("ledger.json");
    ok(&[
        "evm-byte-causes",
        "--artifact",
        &fixture("fe-base/runtime.bin"),
        "--regions",
        &regions,
        "--json-out",
        ledger.to_str().unwrap(),
    ]);
    let ledger = ledger.to_str().unwrap();
    // Equal sizes: every share is undefined, not NaN.
    let out = ok(&[
        "evm-byte-causes-compare",
        "--left",
        ledger,
        "--right",
        ledger,
    ]);
    assert!(!out.contains("NaN"), "{out}");
    // A ledger whose buckets no longer cover its artifact, on both sides
    // alike so the excesses still add up.
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(ledger).unwrap()).unwrap();
    value["artifact_bytes"] = serde_json::json!(40);
    let tampered = write(&dir, "tampered.json", &value.to_string());
    let err = refused(&[
        "evm-byte-causes-compare",
        "--left",
        &tampered,
        "--right",
        &tampered,
    ]);
    assert!(err.contains("tampered.json") && err.contains("36"), "{err}");
}

#[test]
fn producer_outputs_are_not_replaced_without_force() {
    let dir = scratch("force");
    let regions = base_regions(&dir);
    let mut args = base_args("fe-trace-bytes", &fixture("fe-base/runtime.bin"));
    args.extend([
        "--artifact-out".to_string(),
        dir.join("runtime-copy.bin").to_str().unwrap().to_string(),
        "--manifest-out".into(),
        regions.clone(),
    ]);
    let err = refused(&strs(&args));
    assert!(err.contains("--force"), "{err}");
    args.push("--force".into());
    ok(&strs(&args));
}

/// The fixture trace with its first (metadata) line and code hash replaced.
fn trace_variant(dir: &Path, name: &str, metadata: Option<&str>, code_hash: &str) -> String {
    let text = std::fs::read_to_string(fixture("fe-base/trace.jsonl")).unwrap();
    let (first, rest) = text.split_once('\n').unwrap();
    assert!(first.contains("\"metadata\""));
    let hash = "\"blake3:83c1ebd700a30ea46e4b70ec7d3f44b1a0b48916c759a728cc73337ea2214b90\"";
    assert!(rest.contains(hash));
    let rest = rest.replace(hash, code_hash);
    let text = match metadata {
        Some(m) => format!("{m}\n{rest}"),
        None => rest,
    };
    write(dir, name, &text)
}

fn trace_bytes_args(trace: &str, artifact: &str) -> Vec<String> {
    let mut args = base_args("fe-trace-bytes", artifact);
    args[2] = trace.to_string();
    args
}

#[test]
fn fe_trace_bytes_reads_trace_schemas_one_and_two_only() {
    let dir = scratch("trace-schemas");
    let runtime = fixture("fe-base/runtime.bin");
    let hash = "\"blake3:83c1ebd700a30ea46e4b70ec7d3f44b1a0b48916c759a728cc73337ea2214b90\"";
    for (k, meta) in [
        r#"{"record":"metadata","schema_version":1,"input_path":"demo"}"#,
        r#"{"record":"metadata","schema_version":2,"input_path":"demo"}"#,
        r#"{ "input_path": "demo", "schema_version": 2, "record": "metadata" }"#,
    ]
    .into_iter()
    .enumerate()
    {
        let trace = trace_variant(&dir, &format!("ok{k}.jsonl"), Some(meta), hash);
        ok(&strs(&trace_bytes_args(&trace, &runtime)));
    }
    for (k, meta) in [
        Some(r#"{"record":"metadata","schema_version":3}"#),
        Some(r#"{"schema_version":99,"record":"metadata"}"#),
        Some(r#"{"record":"metadata","input_path":"demo"}"#),
        Some(r#"{"record":"metadata","schema_version":"2"}"#),
        None,
    ]
    .into_iter()
    .enumerate()
    {
        let trace = trace_variant(&dir, &format!("bad{k}.jsonl"), meta, hash);
        let err = refused(&strs(&trace_bytes_args(&trace, &runtime)));
        assert!(
            err.contains("schema") || err.contains("metadata"),
            "{meta:?}: {err}"
        );
    }
}

#[test]
fn fe_trace_bytes_checks_the_code_hash() {
    let dir = scratch("code-hash");
    let runtime = fixture("fe-base/runtime.bin");
    let meta = Some(r#"{"record":"metadata","schema_version":2}"#);
    // The fixture's own hash: checked.
    let out = ok(&strs(&base_args("fe-trace-bytes", &runtime)));
    assert!(out.contains("code hash included"), "{out}");
    // Another artifact's hash: refused.
    let other = format!("\"blake3:{}\"", blake3::hash(b"other").to_hex());
    let trace = trace_variant(&dir, "other.jsonl", meta, &other);
    let err = refused(&strs(&trace_bytes_args(&trace, &runtime)));
    assert!(err.contains("differs from trace code hash"), "{err}");
    // Two bytes of data after the code: proven by the hash, refused without.
    let longer = [0x60u8, 0x80, 0x52, 0x00, 0xaa, 0xbb];
    let longer_path = dir.join("longer.bin");
    std::fs::write(&longer_path, longer).unwrap();
    let longer_path = longer_path.to_str().unwrap();
    let hash = format!("\"blake3:{}\"", blake3::hash(&longer).to_hex());
    let trace = trace_variant(&dir, "longer.jsonl", meta, &hash);
    let out = ok(&strs(&trace_bytes_args(&trace, longer_path)));
    assert!(out.contains("2 data bytes after the code"), "{out}");
    let trace = trace_variant(&dir, "nohash.jsonl", meta, "null");
    let err = refused(&strs(&trace_bytes_args(&trace, longer_path)));
    assert!(err.contains("no code hash"), "{err}");
    // Without a hash the opcodes are still compared.
    let changed = dir.join("changed.bin");
    std::fs::write(&changed, [0x60, 0x80, 0x53, 0x00]).unwrap();
    let err = refused(&strs(&trace_bytes_args(&trace, changed.to_str().unwrap())));
    assert!(err.contains("opcode at pc 2"), "{err}");
}

#[test]
fn every_command_gives_the_same_output_twice() {
    let dir = scratch("determinism");
    let regions = base_regions(&dir);
    let runtime = fixture("fe-base/runtime.bin");
    let (solc_artifact, solc_manifest, _) = solc_build(&dir);
    let json = |k: usize| {
        dir.join(format!("run{k}.json"))
            .to_str()
            .unwrap()
            .to_string()
    };
    let commands: Vec<Vec<String>> = vec![
        base_args("fe-trace-bytes", &runtime),
        {
            let mut a = base_args("fe-trace-stages", &runtime);
            a.extend(["--regions".to_string(), regions.clone()]);
            a
        },
        vec![
            "evm-dataflow".into(),
            "--artifact".into(),
            solc_artifact.clone(),
            "--regions".into(),
            solc_manifest.clone(),
            "--blocks-out".into(),
            dir.join("blocks.json").to_str().unwrap().into(),
        ],
        vec![
            "evm-byte-causes".into(),
            "--artifact".into(),
            solc_artifact.clone(),
            "--regions".into(),
            solc_manifest.clone(),
            "--cause".into(),
            "regions:g=B.g".into(),
        ],
    ];
    for command in commands {
        let outputs: Vec<(String, Vec<u8>)> = (0..2)
            .map(|k| {
                let mut args = command.clone();
                args.extend(["--json-out".to_string(), json(k)]);
                let stdout = ok(&strs(&args));
                (stdout, std::fs::read(json(k)).unwrap())
            })
            .collect();
        assert_eq!(outputs[0], outputs[1], "{command:?}");
    }
}

#[test]
fn solc_functions_tiles_the_real_runtime() {
    let dir = scratch("solc");
    let (artifact, manifest, report) = solc_build(&dir);
    let code = std::fs::read(&artifact).unwrap();
    assert_eq!(code.len(), 406);
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&report).unwrap()).unwrap();
    assert_eq!(report["schema"], "riffcat-solc-functions/1");
    assert_eq!(report["code_end"], 353);
    let total: u64 = report["by_owner"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row[1].as_u64().unwrap())
        .sum();
    assert_eq!(total, 406);
    let manifest: riff_catalog_bloat::RegionManifest =
        serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    let mut end = 0;
    for r in &manifest.regions {
        assert_eq!(r.start, end, "{r:?}");
        end = r.end;
    }
    assert_eq!(end, 406);
    // The regions name source functions, and the census reads the pair.
    assert!(manifest.regions.iter().any(|r| r.name == "B.g"));
    riff_catalog_bloat::census_regions(&code, manifest).unwrap();
}
