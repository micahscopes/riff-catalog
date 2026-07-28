//! The conformance gates (M8/M9): the same Yul program through both front
//! doors must agree.
//!
//! Level 0: `parse_object(ir text) == from_solc_value(irAst JSON)` — derived
//! `PartialEq` over the shared AST.
//! Level 1: lowering both ASTs (identical owner) and hashing under both view
//! modes must produce identical digests for every graph, every dimension.
//!
//! Runs over every vendored fixture contract × {ir, irOptimized} × optimizer
//! {on,off}. Requires solc on PATH (skips cleanly without it); the fixture
//! sources are vendored, so no network and no personal checkout are needed.

use std::collections::BTreeMap;
use std::path::PathBuf;

use riff_catalog_core::{CyclePolicy, DigestRequest, HashPolicy, ViewMode, digest_graph};
use riff_catalog_solc::{CompileOptions, SolcRunner, solidity_input};
use riff_catalog_yul::lower::{LowerOptions, YUL_AST_LEVEL, lower_object};
use riff_catalog_yul::{from_solc_value, parse_object};

/// The vendored, self-contained Solidity fixtures: flattened Sourcify
/// exact_match mainnet deployments committed under the workspace's
/// `tests/fixtures/solidity/`. Read via a `CARGO_MANIFEST_DIR`-relative path so
/// the sweep runs on any machine: no `$HOME`, no network, no personal checkout.
/// The fixtures are tracked in the repo, so a missing directory is a hard
/// error, not a skip.
fn fixture_sources() -> Vec<PathBuf> {
    let root = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/solidity"
    ));
    let mut sources = Vec::new();
    let entries = std::fs::read_dir(&root)
        .unwrap_or_else(|error| panic!("fixtures dir {} unreadable: {error}", root.display()));
    for file in entries.flatten() {
        if file.path().extension().is_some_and(|ext| ext == "sol") {
            sources.push(file.path());
        }
    }
    sources.sort();
    sources
}

fn solc() -> Option<SolcRunner> {
    let runner = SolcRunner::locate(None);
    runner.version().ok().map(|_| runner)
}

#[test]
fn dual_path_conformance_over_fixtures() {
    let Some(solc) = solc() else {
        eprintln!("skipping: no solc on PATH");
        return;
    };
    let sources = fixture_sources();
    assert!(!sources.is_empty(), "no vendored .sol fixtures found");

    let mut checked_objects = 0usize;
    let mut checked_graphs = 0usize;

    for path in &sources {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let content = std::fs::read_to_string(path).unwrap();
        let inputs: BTreeMap<String, String> = [(name.clone(), content)].into();

        for optimize in [false, true] {
            let options = CompileOptions {
                optimize,
                ..Default::default()
            };
            let output = match solc.compile(&solidity_input(&inputs, &options)) {
                Ok(output) => output,
                Err(error) => panic!("solc failed on {name}: {error}"),
            };
            if output.check_errors().is_err() {
                // Some corpus entries may need imports we don't provide;
                // skip them rather than fail the whole sweep.
                eprintln!("skipping {name} (solc errors)");
                continue;
            }

            for (source, contract) in output.contract_names() {
                for (variant, text, json) in [
                    (
                        "ir",
                        output.ir(&source, &contract),
                        output.ir_ast(&source, &contract),
                    ),
                    (
                        "iropt",
                        output.ir_optimized(&source, &contract),
                        output.ir_optimized_ast(&source, &contract),
                    ),
                ] {
                    let (Ok(text), Ok(json)) = (text, json) else {
                        continue;
                    };
                    if text.trim().is_empty() {
                        // Interfaces/abstract contracts compile to empty IR.
                        continue;
                    }
                    let from_text = parse_object(text).unwrap_or_else(|error| {
                        panic!("{name}/{contract}/{variant}: parse failed: {error}")
                    });
                    let from_json = from_solc_value(json).unwrap_or_else(|error| {
                        panic!("{name}/{contract}/{variant}: deserialize failed: {error}")
                    });

                    // Level 0: AST equality.
                    assert_eq!(
                        from_text, from_json,
                        "{name}/{contract}/{variant}: AST mismatch between text and JSON paths"
                    );
                    checked_objects += 1;

                    // Level 1: identical digests from both paths.
                    let owner = format!("yulir:{name}:{contract}:{variant}");
                    let lowered_text =
                        lower_object(&from_text, &owner, &LowerOptions::default()).unwrap();
                    let lowered_json =
                        lower_object(&from_json, &owner, &LowerOptions::default()).unwrap();

                    for view_mode in [ViewMode::IdentityBound, ViewMode::AnonymousShape] {
                        let policy =
                            HashPolicy::new(YUL_AST_LEVEL, view_mode, CyclePolicy::CondenseScc)
                                .unwrap();
                        let units_text = std::iter::once(&lowered_text.object)
                            .chain(lowered_text.functions.iter());
                        let units_json = std::iter::once(&lowered_json.object)
                            .chain(lowered_json.functions.iter());
                        for (unit_text, unit_json) in units_text.zip(units_json) {
                            let hash = |unit: &riff_catalog_yul::lower::LoweredUnit| {
                                digest_graph(
                                    &DigestRequest::all_dimensions(
                                        unit.graph_key.clone(),
                                        policy.clone(),
                                    ),
                                    &unit.graph,
                                )
                                .unwrap()
                                .hashes
                            };
                            assert_eq!(
                                hash(unit_text).graph,
                                hash(unit_json).graph,
                                "{name}/{contract}/{variant}/{}: digest mismatch ({view_mode:?})",
                                unit_text.name
                            );
                            checked_graphs += 1;
                        }
                    }
                }
            }
        }
    }

    assert!(checked_objects > 0, "conformance sweep checked nothing");
    println!("conformance: {checked_objects} objects, {checked_graphs} graph digest pairs");
}

/// The first helper-dedup smoke: solc generates identical helpers across
/// different contracts; their yul-fn graphs must collide at the anonymous
/// full facet.
#[test]
fn solc_helpers_dedupe_across_contracts() {
    let Some(solc) = solc() else {
        eprintln!("skipping: no solc on PATH");
        return;
    };
    let sources = fixture_sources();
    assert!(
        sources.len() >= 2,
        "helper-dedup smoke needs at least two fixture contracts"
    );

    let policy = HashPolicy::new(
        YUL_AST_LEVEL,
        ViewMode::AnonymousShape,
        CyclePolicy::CondenseScc,
    )
    .unwrap();

    // map: helper name -> set of distinct anonymous structure digests
    let mut digests_by_helper: BTreeMap<String, Vec<riff_catalog_core::Digest>> = BTreeMap::new();
    let mut contracts_seen = 0;

    for path in sources.iter().take(3) {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let content = std::fs::read_to_string(path).unwrap();
        let inputs: BTreeMap<String, String> = [(name.clone(), content)].into();
        let Ok(output) = solc.compile(&solidity_input(&inputs, &CompileOptions::default())) else {
            continue;
        };
        if output.check_errors().is_err() {
            continue;
        }
        for (source, contract) in output.contract_names() {
            // Interfaces and abstract contracts compile to empty IR (no
            // functions to dedup); skip them like the dual-path sweep does,
            // otherwise from_solc_value chokes on the empty irAst.
            let Ok(ir_text) = output.ir(&source, &contract) else {
                continue;
            };
            if ir_text.trim().is_empty() {
                continue;
            }
            let Ok(ir_ast) = output.ir_ast(&source, &contract) else {
                continue;
            };
            let object = from_solc_value(ir_ast).unwrap();
            let owner = format!("yulir:{name}:{contract}:ir");
            let lowered = lower_object(&object, &owner, &LowerOptions::default()).unwrap();
            contracts_seen += 1;
            for unit in &lowered.functions {
                let hashes = digest_graph(
                    &DigestRequest::all_dimensions(unit.graph_key.clone(), policy.clone()),
                    &unit.graph,
                )
                .unwrap()
                .hashes;
                digests_by_helper
                    .entry(unit.name.clone())
                    .or_default()
                    .push(
                        *hashes
                            .graph
                            .get(riff_catalog_core::Dimension::Structure)
                            .unwrap(),
                    );
            }
        }
    }

    if contracts_seen < 2 {
        eprintln!("skipping: fewer than two contracts compiled");
        return;
    }

    // allocate_unbounded is generated identically for every contract.
    let allocate = digests_by_helper
        .get("allocate_unbounded")
        .expect("allocate_unbounded helper present");
    assert!(allocate.len() >= 2, "helper appears in multiple contracts");
    assert!(
        allocate.windows(2).all(|pair| pair[0] == pair[1]),
        "allocate_unbounded must hash identically across contracts"
    );
}
