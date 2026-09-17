use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Result, ensure};
use riff_catalog_core::{
    CyclePolicy, DigestRequest, Dimension, HashPolicy, ViewMode, digest_graph,
};
use riff_catalog_region::{address_graph, from_yul_graph, wl_observation};
use riff_catalog_solc::{SolcRunner, yul_input};
use riff_catalog_yul::ssa::{YUL_SSA_LEVEL, lower_yul_cfg};
use serde_json::json;

fn main() -> Result<()> {
    let destination = std::env::args_os().nth(1).map(PathBuf::from);
    let source = include_str!("../fixtures/regions.yul");
    let solc = SolcRunner::locate(None);
    let mut input = yul_input("regions.yul", source, false);
    input["settings"]["experimental"] = json!(true);
    let output = solc.compile(&input)?;
    output.check_errors()?;
    let cfg = output.yul_cfg_json("regions.yul", "RegionPilot")?;
    let lowered = lower_yul_cfg(cfg, "pilot:ported-regions")?;
    let units: BTreeMap<_, _> = lowered
        .functions
        .iter()
        .map(|unit| (unit.name.as_str(), unit))
        .collect();
    let (reference, _) = from_yul_graph(&units["straight"].graph)?.normalize()?;
    let precise = reference.address(true)?;
    let shape = reference.address(false)?;
    let mut rows = Vec::new();
    println!("Real solc SSA, optimizer disabled, explicit three-operation selection");
    println!("case               inputs outputs body    literal-blind max-SCC");
    for name in [
        "straight",
        "wrapped",
        "changed_header",
        "kernel",
        "external_a",
        "external_b",
        "changed_literal",
        "alias_xxy",
        "alias_xyy",
        "alias_xyx",
    ] {
        let unit = units[name];
        let (normal, bindings) = from_yul_graph(&unit.graph)?.normalize()?;
        let body = normal.address(true)?;
        let literal_blind = normal.address(false)?;
        let same = body == precise;
        ensure!(
            same == !matches!(
                name,
                "changed_literal" | "alias_xxy" | "alias_xyy" | "alias_xyx"
            ),
            "unexpected body equality: {name}"
        );
        ensure!(
            (literal_blind == shape) == !matches!(name, "alias_xxy" | "alias_xyy" | "alias_xyx"),
            "unexpected literal-blind equality: {name}"
        );
        ensure!(
            normal.outputs == vec![2],
            "expected single exported result: {name}"
        );
        let policy = HashPolicy::new(
            YUL_SSA_LEVEL,
            ViewMode::AnonymousShape,
            CyclePolicy::CondenseScc,
        )?;
        let hashes = digest_graph(
            &DigestRequest::all_dimensions(unit.graph_key.clone(), policy),
            &unit.graph,
        )?
        .hashes;
        let max_scc = hashes
            .components
            .iter()
            .map(|c| c.members.len())
            .max()
            .unwrap_or(0);
        let unresolved = hashes
            .components
            .iter()
            .map(|c| {
                let mut cells = BTreeMap::new();
                for colors in c.member_colors.values() {
                    *cells
                        .entry(*colors.get(Dimension::Structure).unwrap())
                        .or_insert(0usize) += 1;
                }
                cells.values().filter(|&&size| size > 1).count()
            })
            .sum::<usize>();
        println!(
            "{name:<19}{:<7}{:<8}{:<8}{:<14}{max_scc}",
            normal.inputs,
            normal.outputs.len(),
            if same { "same" } else { "changed" },
            if literal_blind == shape {
                "same"
            } else {
                "changed"
            }
        );
        rows.push(
            json!({ "case": name, "body": body.to_hex(), "literal_blind": literal_blind.to_hex(),
            "container": address_graph(&unit.graph, YUL_SSA_LEVEL, true)?.to_hex(),
            "same_body": same, "normalized_region": normal, "occurrence_input_bindings": bindings,
            "maximum_scc": max_scc, "contextual_structure_nonsingleton_cells": unresolved }),
        );
    }
    let row = |name: &str| rows.iter().find(|row| row["case"] == name).unwrap();
    ensure!(
        row("wrapped")["container"] != row("changed_header")["container"],
        "header mutation must change container"
    );
    ensure!(
        row("external_a")["container"] != row("external_b")["container"],
        "external mutation must change container"
    );
    ensure!(
        row("alias_xxy")["body"] != row("alias_xyy")["body"],
        "equal port counts must not erase aliasing"
    );
    ensure!(
        row("alias_xxy")["body"] != row("alias_xyx")["body"],
        "equal port use counts must not erase use positions"
    );
    let wl = wl_observation::run()?;
    println!(
        "\nWL observation: prism and K3,3 share a coarse address; exact oracle distinguishes them."
    );
    println!("Both: 6 vertices, 9 edges, degree 3. Triangles: 2 versus 0.");
    println!("One 8-vertex SCC also has one WL cell containing distinct triangle-count roles.");
    let solc_version = std::process::Command::new(solc.path())
        .arg("--version")
        .output()?;
    let report = json!({ "schema": "riffcat-region-pilot/1", "solc_path": solc.path(),
        "solc_binary_blake3": blake3::hash(&std::fs::read(solc.path())?).to_hex().to_string(),
        "solc_version": String::from_utf8_lossy(&solc_version.stdout),
        "source_blake3": blake3::hash(source.as_bytes()).to_hex().to_string(),
        "optimizer": false, "selection": "unique ordered add/mul/xor window",
        "region_contract": riff_catalog_region::CONTRACT, "regions": rows,
        "extracted_caller_container": address_graph(&units["extracted"].graph, YUL_SSA_LEVEL, true)?.to_hex(),
        "wl_observation": wl });
    if let Some(directory) = destination {
        std::fs::create_dir_all(&directory)?;
        std::fs::write(
            directory.join("report.json"),
            serde_json::to_vec_pretty(&report)?,
        )?;
        std::fs::write(
            directory.join("solc-input.json"),
            serde_json::to_vec_pretty(&input)?,
        )?;
        std::fs::write(directory.join("ssa.json"), serde_json::to_vec_pretty(cfg)?)?;
        println!("Report: {}", directory.join("report.json").display());
    }
    Ok(())
}
