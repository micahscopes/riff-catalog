//! M6 gate: drive the real solc against a rosetta contract and assert every
//! output we depend on is present. Skips (with a note) when solc or the
//! rosetta corpus is missing, so offline CI stays green.

use std::collections::BTreeMap;

use riff_catalog_solc::*;

const ROSETTA_ERC20: &str = concat!(
    env!("HOME"),
    "/hacker-stuff-2023/fe-stuff/rosetta-fe/examples/erc20/sol/SolidityERC20.sol"
);

fn solc_available() -> bool {
    SolcRunner::locate(None).version().is_ok()
}

fn erc20_source() -> Option<(String, String)> {
    // The rosetta layout keeps Solidity sources under examples/*/sol/.
    let candidates = [
        ROSETTA_ERC20.to_string(),
        format!(
            "{}/hacker-stuff-2023/fe-stuff/rosetta-fe/examples/erc20/sol/ERC20.sol",
            env!("HOME")
        ),
    ];
    for path in candidates {
        if let Ok(content) = std::fs::read_to_string(&path) {
            let name = std::path::Path::new(&path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();
            return Some((name, content));
        }
    }
    None
}

#[test]
fn version_parses() {
    if !solc_available() {
        eprintln!("skipping: no solc on PATH");
        return;
    }
    let version = SolcRunner::locate(None).version().unwrap();
    assert_eq!(version.major, 0);
    assert!(version.minor >= 8);
}

#[test]
fn solidity_via_ir_produces_all_outputs() {
    if !solc_available() {
        eprintln!("skipping: no solc on PATH");
        return;
    }
    let Some((name, content)) = erc20_source() else {
        eprintln!("skipping: rosetta corpus not found");
        return;
    };
    let sources: BTreeMap<String, String> = [(name.clone(), content)].into();
    let output = SolcRunner::locate(None)
        .compile(&solidity_input(&sources, &CompileOptions::default()))
        .unwrap();
    output.check_errors().unwrap();

    let contracts = output.contract_names();
    assert!(!contracts.is_empty());
    let (source, contract) = &contracts[0];

    output.source_ast(source).unwrap();
    assert!(!output.ir(source, contract).unwrap().is_empty());
    output.ir_ast(source, contract).unwrap();
    output.ir_optimized_ast(source, contract).unwrap();
    output.yul_cfg_json(source, contract).unwrap();
    assert!(!output.bytecode(source, contract).unwrap().is_empty());
    assert!(
        !output
            .deployed_bytecode(source, contract)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn direct_yul_produces_ssa_cfg() {
    if !solc_available() {
        eprintln!("skipping: no solc on PATH");
        return;
    }
    let yul = r#"object "Demo" { code { let x := add(1, 2) sstore(0, x) } }"#;
    let output = SolcRunner::locate(None)
        .compile(&yul_input("in.yul", yul, false))
        .unwrap();
    output.check_errors().unwrap();
    let cfg = output.yul_cfg_json("in.yul", "Demo").unwrap();
    assert!(cfg.is_object());
    assert!(!output.bytecode("in.yul", "Demo").unwrap().is_empty());
}

#[test]
fn cache_round_trips() {
    if !solc_available() {
        eprintln!("skipping: no solc on PATH");
        return;
    }
    let dir = std::env::temp_dir().join("riffcat-solc-cache-test");
    let _ = std::fs::remove_dir_all(&dir);
    let cached = CachedSolc::new(SolcRunner::locate(None), &dir);
    let yul = r#"object "Demo" { code { sstore(0, 1) } }"#;
    let input = yul_input("in.yul", yul, false);
    let first = cached.compile(&input).unwrap();
    // second run must hit the disk cache (same content either way)
    let second = cached.compile(&input).unwrap();
    assert_eq!(first.raw(), second.raw());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
}
