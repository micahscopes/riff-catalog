//! `riffcat conformance`: the canonical-encoding drift detector (the interop
//! plan's #1 hazard), runnable over any directory of .sol files. Nonzero
//! exit on any divergence.
//!
//! Level 0: parse(ir text) == deserialize(irAst JSON) — shared-AST equality.
//! Level 1: lowering both and hashing must agree on every digest, every
//!          dimension, both view modes.
//! Level 2: Solidity → yulCFGJson directly vs ir text fed back through solc
//!          as language:Yul → yulCFGJson — anonymous digests must agree.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use riff_catalog_core::{
    CyclePolicy, DigestRequest, Dimension, HashPolicy, ViewMode, digest_graph,
};
use riff_catalog_solc::{CachedSolc, CompileOptions, SolcRunner, solidity_input, yul_input};
use riff_catalog_yul::lower::{LowerOptions, LoweredUnit, YUL_AST_LEVEL, lower_object};
use riff_catalog_yul::ssa::{YUL_SSA_LEVEL, lower_yul_cfg};
use riff_catalog_yul::{from_solc_value, parse_object};

use crate::ingest::OptimizeChoice;
use crate::table::Table;

pub fn run(
    paths: &[PathBuf],
    optimize: OptimizeChoice,
    solc_path: Option<&std::path::Path>,
    cache_dir: &std::path::Path,
    keep_going: bool,
    json: bool,
) -> Result<bool> {
    let solc = CachedSolc::new(SolcRunner::locate(solc_path), cache_dir.join("solc"));
    let mut table = Table::new(&[
        "source", "contract", "variant", "level0", "level1", "level2",
    ]);
    let mut all_green = true;

    let mut sol_files = Vec::new();
    for path in paths {
        if path.is_dir() {
            collect_sol_files(path, &mut sol_files)?;
        } else if path.extension().is_some_and(|ext| ext == "sol") {
            sol_files.push(path.clone());
        }
    }
    sol_files.sort();

    'files: for path in &sol_files {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        let content =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let inputs: BTreeMap<String, String> = [(name.clone(), content)].into();

        for optimize in optimize_variants(optimize) {
            let options = CompileOptions {
                optimize,
                ..Default::default()
            };
            let Ok(output) = solc.compile(&solidity_input(&inputs, &options)) else {
                continue;
            };
            if output.check_errors().is_err() {
                continue;
            }

            for (source, contract) in output.contract_names() {
                for (variant, text, ast) in [
                    (
                        "ir",
                        output.ir(&source, &contract).ok(),
                        output.ir_ast(&source, &contract).ok(),
                    ),
                    (
                        "iropt",
                        output.ir_optimized(&source, &contract).ok(),
                        output.ir_optimized_ast(&source, &contract).ok(),
                    ),
                ] {
                    let (Some(text), Some(ast)) = (text, ast) else {
                        continue;
                    };
                    if text.trim().is_empty() {
                        continue;
                    }
                    let variant_tag =
                        format!("{variant}:{}", if optimize { "opt" } else { "noopt" });

                    let (level0, level1) = check_levels_01(text, ast, &source, &contract);
                    // Level 2 only for the unoptimized base variant (one
                    // round-trip per contract is the meaningful one).
                    let level2 = if variant == "ir" {
                        check_level2(&solc, &output, &source, &contract)
                    } else {
                        Ok("-".to_string())
                    };

                    let level0_text = verdict(&level0);
                    let level1_text = verdict(&level1);
                    let level2_text = verdict(&level2);
                    if level0.is_err() || level1.is_err() || level2.is_err() {
                        all_green = false;
                    }
                    table.row(vec![
                        name.clone(),
                        contract.clone(),
                        variant_tag,
                        level0_text,
                        level1_text,
                        level2_text,
                    ]);
                    if !all_green && !keep_going {
                        break 'files;
                    }
                }
            }
        }
    }

    if json {
        println!("{}", table.to_json());
    } else {
        table.print();
        println!(
            "\nconformance: {}",
            if all_green { "GREEN" } else { "DRIFT DETECTED" }
        );
    }
    Ok(all_green)
}

fn optimize_variants(choice: OptimizeChoice) -> Vec<bool> {
    match choice {
        OptimizeChoice::On => vec![true],
        OptimizeChoice::Off => vec![false],
        OptimizeChoice::Both => vec![false, true],
    }
}

fn verdict(result: &Result<String>) -> String {
    match result {
        Ok(text) => text.clone(),
        Err(error) => format!("FAIL: {error}"),
    }
}

fn check_levels_01(
    text: &str,
    ast: &serde_json::Value,
    source: &str,
    contract: &str,
) -> (Result<String>, Result<String>) {
    let from_text = match parse_object(text) {
        Ok(object) => object,
        Err(error) => {
            return (
                Err(anyhow::anyhow!("parse: {error}")),
                Err(anyhow::anyhow!("blocked by level 0")),
            );
        }
    };
    let from_json = match from_solc_value(ast) {
        Ok(object) => object,
        Err(error) => {
            return (
                Err(anyhow::anyhow!("deserialize: {error}")),
                Err(anyhow::anyhow!("blocked by level 0")),
            );
        }
    };
    let level0 = if from_text == from_json {
        Ok("ok".to_string())
    } else {
        Err(anyhow::anyhow!("AST mismatch"))
    };

    let level1 = (|| {
        let owner = format!("conf:{source}:{contract}");
        let lowered_text = lower_object(&from_text, &owner, &LowerOptions::default())?;
        let lowered_json = lower_object(&from_json, &owner, &LowerOptions::default())?;
        let mut compared = 0usize;
        for view_mode in [ViewMode::IdentityBound, ViewMode::AnonymousShape] {
            let policy = HashPolicy::new(YUL_AST_LEVEL, view_mode, CyclePolicy::CondenseScc)?;
            let pairs = std::iter::once((&lowered_text.object, &lowered_json.object)).chain(
                lowered_text
                    .functions
                    .iter()
                    .zip(lowered_json.functions.iter()),
            );
            for (left, right) in pairs {
                let hash = |unit: &LoweredUnit| -> Result<_> {
                    Ok(digest_graph(
                        &DigestRequest::all_dimensions(unit.graph_key.clone(), policy.clone()),
                        &unit.graph,
                    )?
                    .hashes
                    .graph)
                };
                if hash(left)? != hash(right)? {
                    anyhow::bail!("digest mismatch on {}", left.name);
                }
                compared += 1;
            }
        }
        Ok(format!("ok ({compared})"))
    })();

    (level0, level1)
}

fn check_level2(
    solc: &CachedSolc,
    output: &riff_catalog_solc::SolcOutput,
    source: &str,
    contract: &str,
) -> Result<String> {
    let Ok(direct_cfg) = output.yul_cfg_json(source, contract) else {
        return Ok("-".to_string());
    };
    let direct = lower_yul_cfg(direct_cfg, "conf:direct")?;

    let ir_text = output.ir(source, contract)?;
    let round_output = solc.compile(&yul_input("roundtrip.yul", ir_text, false))?;
    round_output
        .check_errors()
        .map_err(|error| anyhow::anyhow!("round-trip compile failed: {error}"))?;
    let contracts = round_output.contract_names();
    let Some((round_source, round_contract)) = contracts.first() else {
        anyhow::bail!("round-trip produced no contracts");
    };
    let round_cfg = round_output.yul_cfg_json(round_source, round_contract)?;
    let round = lower_yul_cfg(round_cfg, "conf:round")?;

    let collect = |lowered: &riff_catalog_yul::ssa::LoweredSsa| -> Result<Vec<(String, String)>> {
        let policy = HashPolicy::new(
            YUL_SSA_LEVEL,
            ViewMode::AnonymousShape,
            CyclePolicy::CondenseScc,
        )?;
        let mut digests = Vec::new();
        for unit in &lowered.functions {
            let hashes = digest_graph(
                &DigestRequest::all_dimensions(unit.graph_key.clone(), policy.clone()),
                &unit.graph,
            )?
            .hashes;
            digests.push((
                unit.name.clone(),
                hashes
                    .graph
                    .get(Dimension::Structure)
                    .expect("computed")
                    .to_hex(),
            ));
        }
        digests.sort();
        Ok(digests)
    };
    if collect(&direct)? != collect(&round)? {
        anyhow::bail!("SSA round-trip digests diverge");
    }
    Ok(format!("ok ({})", direct.functions.len()))
}

fn collect_sol_files(dir: &std::path::Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_sol_files(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "sol") {
            out.push(path);
        }
    }
    Ok(())
}
