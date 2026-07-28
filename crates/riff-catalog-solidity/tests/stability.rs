//! M10 gates: the vendored fixture corpus lowers in strict mode, and identity
//! digests are stable across exactly the edits they should ignore.
//!
//! - comment insertion shifts every solc `id` and `src` — digests unchanged
//!   (proves the id/src exclusion and path-keying, invariant I15)
//! - renaming a local variable moves only the Names dimension (I2)

use std::collections::BTreeMap;
use std::path::PathBuf;

use riff_catalog_core::{
    CyclePolicy, DigestRequest, Dimension, GraphHashes, HashPolicy, ViewMode, digest_graph,
};
use riff_catalog_solc::{CompileOptions, SolcRunner, solidity_input};
use riff_catalog_solidity::{SOL_AST_LEVEL, WalkOptions, lower_source_unit};

fn solc() -> Option<SolcRunner> {
    let runner = SolcRunner::locate(None);
    runner.version().ok().map(|_| runner)
}

fn compile_ast(source_name: &str, content: &str) -> Option<serde_json::Value> {
    let solc = solc()?;
    let sources: BTreeMap<String, String> = [(source_name.to_string(), content.to_string())].into();
    let output = solc
        .compile(&solidity_input(&sources, &CompileOptions::default()))
        .ok()?;
    output.check_errors().ok()?;
    Some(output.source_ast(source_name).ok()?.clone())
}

fn hash_unit(unit: &riff_catalog_solidity::LoweredUnit, view_mode: ViewMode) -> GraphHashes {
    let policy = HashPolicy::new(SOL_AST_LEVEL, view_mode, CyclePolicy::CondenseScc).unwrap();
    digest_graph(
        &DigestRequest::all_dimensions(unit.graph_key.clone(), policy),
        &unit.graph,
    )
    .unwrap()
    .hashes
}

/// The vendored, self-contained Solidity fixtures under the workspace's
/// `tests/fixtures/solidity/` (flattened Sourcify exact_match mainnet
/// deployments). Read via a `CARGO_MANIFEST_DIR`-relative path so the sweep
/// runs on any machine: no `$HOME`, no network, no personal checkout. The
/// fixtures are tracked, so a missing directory is a hard error, not a skip.
fn fixture_sources() -> Vec<PathBuf> {
    let root = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/solidity"
    ));
    let entries = std::fs::read_dir(&root)
        .unwrap_or_else(|error| panic!("fixtures dir {} unreadable: {error}", root.display()));
    let mut sources: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sol"))
        .collect();
    sources.sort();
    sources
}

#[test]
fn fixture_corpus_lowers_strict() {
    if solc().is_none() {
        eprintln!("skipping: no solc on PATH");
        return;
    }
    let sources = fixture_sources();
    assert!(!sources.is_empty(), "no vendored .sol fixtures found");

    let mut lowered_units = 0usize;
    for path in &sources {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let content = std::fs::read_to_string(path).unwrap();
        let Some(ast) = compile_ast(&name, &content) else {
            continue;
        };
        let owner = format!("sol:{name}");
        let lowered = lower_source_unit(&ast, &owner, &WalkOptions { strict: true })
            .unwrap_or_else(|error| panic!("{name}: strict lowering failed: {error}"));
        assert!(lowered.warnings.is_empty());
        // every unit hashes under both modes without error
        for unit in std::iter::once(&lowered.source_unit)
            .chain(&lowered.contracts)
            .chain(&lowered.functions)
        {
            hash_unit(unit, ViewMode::IdentityBound);
            hash_unit(unit, ViewMode::AnonymousShape);
            lowered_units += 1;
        }
    }
    assert!(lowered_units > 0, "fixtures present but nothing compiled");
    println!("strict-lowered + hashed {lowered_units} units");
}

const BASE: &str = r#"// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;
contract Stable {
    uint256 private total;
    function bump(uint256 amount) public returns (uint256 result) {
        uint256 doubled = amount * 2;
        total += doubled;
        result = total;
    }
}
"#;

/// Same program with a comment inserted at the top: every solc id and src
/// offset shifts.
const COMMENTED: &str = r#"// SPDX-License-Identifier: MIT
// an extra comment line
// and another one, shifting every id and src offset in the AST
pragma solidity ^0.8.0;
contract Stable {
    uint256 private total;
    function bump(uint256 amount) public returns (uint256 result) {
        uint256 doubled = amount * 2;
        total += doubled;
        result = total;
    }
}
"#;

/// Same program with the local `doubled` renamed.
const RENAMED: &str = r#"// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;
contract Stable {
    uint256 private total;
    function bump(uint256 amount) public returns (uint256 result) {
        uint256 twice = amount * 2;
        total += twice;
        result = total;
    }
}
"#;

fn contract_hashes(content: &str, view_mode: ViewMode) -> Option<GraphHashes> {
    let ast = compile_ast("stable.sol", content)?;
    let lowered = lower_source_unit(&ast, "sol:stable.sol", &WalkOptions { strict: true }).unwrap();
    assert_eq!(lowered.contracts.len(), 1);
    Some(hash_unit(&lowered.contracts[0], view_mode))
}

#[test]
fn comment_insertion_is_invisible_even_identity_bound() {
    let Some(base) = contract_hashes(BASE, ViewMode::IdentityBound) else {
        eprintln!("skipping: no solc on PATH");
        return;
    };
    let commented = contract_hashes(COMMENTED, ViewMode::IdentityBound).unwrap();
    assert_eq!(
        base, commented,
        "ids/src shifted but digests moved — a volatile field leaked (I15)"
    );
}

#[test]
fn local_rename_moves_only_names() {
    let Some(base) = contract_hashes(BASE, ViewMode::AnonymousShape) else {
        eprintln!("skipping: no solc on PATH");
        return;
    };
    let renamed = contract_hashes(RENAMED, ViewMode::AnonymousShape).unwrap();
    assert_ne!(
        base.graph.get(Dimension::Names),
        renamed.graph.get(Dimension::Names),
        "rename must move Names"
    );
    for dimension in [
        Dimension::Structure,
        Dimension::Constants,
        Dimension::Types,
        Dimension::TraceEvents,
    ] {
        assert_eq!(
            base.graph.get(dimension),
            renamed.graph.get(dimension),
            "rename must not move {dimension:?}"
        );
    }
}
