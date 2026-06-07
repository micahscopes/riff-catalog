//! M12 gates: yulCFGJson lowering works for .sol AND direct .yul input; a
//! loop produces a genuine SCC (WL exercised); the SSA conformance loop
//! (level 2) holds: Solidity→yulCFGJson directly vs Solidity→ir text→solc
//! as Yul→yulCFGJson hash identical at the anonymous facet.

use std::collections::BTreeMap;

use riff_catalog_core::{
    CyclePolicy, DigestRequest, Dimension, GraphHashes, HashPolicy, ViewMode, digest_graph,
};
use riff_catalog_solc::{CompileOptions, SolcRunner, solidity_input, yul_input};
use riff_catalog_yul::lower::LoweredUnit;
use riff_catalog_yul::ssa::{YUL_SSA_LEVEL, lower_yul_cfg};

const LOOPY: &str = r#"// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;
contract Loopy {
    function f(uint256 a) public pure returns (uint256 r) {
        for (uint256 i; i < a; i++) { r += i; }
    }
}
"#;

fn solc() -> Option<SolcRunner> {
    let runner = SolcRunner::locate(None);
    runner.version().ok().map(|_| runner)
}

fn hash(unit: &LoweredUnit, view_mode: ViewMode) -> GraphHashes {
    let policy = HashPolicy::new(YUL_SSA_LEVEL, view_mode, CyclePolicy::CondenseScc).unwrap();
    digest_graph(
        &DigestRequest::all_dimensions(unit.graph_key.clone(), policy),
        &unit.graph,
    )
    .unwrap()
    .hashes
}

#[test]
fn solidity_ssa_cfg_lowers_and_loops_form_sccs() {
    let Some(solc) = solc() else {
        eprintln!("skipping: no solc on PATH");
        return;
    };
    let sources: BTreeMap<String, String> = [("loopy.sol".to_string(), LOOPY.to_string())].into();
    let output = solc
        .compile(&solidity_input(&sources, &CompileOptions::default()))
        .unwrap();
    output.check_errors().unwrap();
    let cfg = output.yul_cfg_json("loopy.sol", "Loopy").unwrap();
    let lowered = lower_yul_cfg(cfg, "yulssa:loopy.sol:Loopy:ir").unwrap();

    assert!(!lowered.objects.is_empty());
    let fun_f = lowered
        .functions
        .iter()
        .find(|unit| unit.name == "fun_f")
        .expect("fun_f present in deployed object");
    let hashes = hash(fun_f, ViewMode::AnonymousShape);
    let max_scc = hashes
        .components
        .iter()
        .map(|component| component.members.len())
        .max()
        .unwrap_or(0);
    assert!(
        max_scc > 1,
        "the for-loop must produce a multi-node SCC, got max component size {max_scc}"
    );
}

#[test]
fn direct_yul_ssa_cfg_lowers() {
    let Some(solc) = solc() else {
        eprintln!("skipping: no solc on PATH");
        return;
    };
    let yul = r#"object "Demo" {
        code {
            function triple(x) -> y { y := mul(x, 3) }
            sstore(0, triple(callvalue()))
        }
    }"#;
    let output = solc.compile(&yul_input("in.yul", yul, false)).unwrap();
    output.check_errors().unwrap();
    let cfg = output.yul_cfg_json("in.yul", "Demo").unwrap();
    let lowered = lower_yul_cfg(cfg, "yulssa:in.yul:Demo").unwrap();
    assert!(!lowered.objects.is_empty());
    assert!(lowered.functions.iter().any(|f| f.name == "triple"));
    hash(&lowered.objects[0], ViewMode::AnonymousShape);
}

/// Conformance level 2, all Argot tooling: feed solc's own `ir` text back in
/// as language:Yul and the SSA CFG must hash identical (anonymously) to the
/// one produced directly from Solidity.
#[test]
fn ssa_conformance_loop() {
    let Some(solc) = solc() else {
        eprintln!("skipping: no solc on PATH");
        return;
    };
    let sources: BTreeMap<String, String> = [("loopy.sol".to_string(), LOOPY.to_string())].into();
    let output = solc
        .compile(&solidity_input(&sources, &CompileOptions::default()))
        .unwrap();
    output.check_errors().unwrap();

    let direct_cfg = output.yul_cfg_json("loopy.sol", "Loopy").unwrap();
    let direct = lower_yul_cfg(direct_cfg, "conf:direct").unwrap();

    let ir_text = output.ir("loopy.sol", "Loopy").unwrap();
    let round_trip_output = solc.compile(&yul_input("in.yul", ir_text, false)).unwrap();
    round_trip_output.check_errors().unwrap();
    let contracts = round_trip_output.contract_names();
    let (source, contract) = &contracts[0];
    let round_cfg = round_trip_output.yul_cfg_json(source, contract).unwrap();
    let round = lower_yul_cfg(round_cfg, "conf:round").unwrap();

    // Compare the per-function multiset of anonymous digests.
    let collect = |lowered: &riff_catalog_yul::ssa::LoweredSsa| {
        let mut digests: Vec<(String, riff_catalog_core::Digest)> = lowered
            .functions
            .iter()
            .map(|unit| {
                (
                    unit.name.clone(),
                    *hash(unit, ViewMode::AnonymousShape)
                        .graph
                        .get(Dimension::Structure)
                        .unwrap(),
                )
            })
            .collect();
        digests.sort();
        digests
    };
    assert_eq!(
        collect(&direct),
        collect(&round),
        "SSA conformance loop: direct vs ir-round-trip digests diverge"
    );
}

/// Regression (external review, P2): the parent object's digest must commit
/// to its sub-objects' CFGs — a change inside the deployed object must move
/// the creation object's digest.
#[test]
fn parent_object_digest_commits_to_subobjects() {
    let cfg = |sstore_key: &str| {
        serde_json::json!({
            "Demo": {
                "blocks": [{
                    "id": "Block0",
                    "instructions": [],
                    "exit": { "type": "Terminated" }
                }],
                "functions": {},
                "subObjects": {
                    "Demo_deployed": {
                        "blocks": [{
                            "id": "Block0",
                            "instructions": [
                                { "op": "sstore", "in": [sstore_key, "0x01"], "out": [] }
                            ],
                            "exit": { "type": "Terminated" }
                        }],
                        "functions": {},
                        "subObjects": {}
                    },
                    "type": "SubObjects"
                }
            },
            "type": "Object"
        })
    };
    let left = lower_yul_cfg(&cfg("0x00"), "ssa:sub-test").unwrap();
    let right = lower_yul_cfg(&cfg("0x05"), "ssa:sub-test").unwrap();
    // both emit parent + deployed object units; compare the PARENT ("Demo")
    let parent = |lowered: &riff_catalog_yul::ssa::LoweredSsa| {
        let unit = lowered
            .objects
            .iter()
            .find(|unit| unit.name == "Demo")
            .expect("parent object unit");
        hash(unit, ViewMode::AnonymousShape).graph
    };
    assert_ne!(
        parent(&left).get(Dimension::Constants),
        parent(&right).get(Dimension::Constants),
        "deployed-code change must move the parent object digest"
    );
}
