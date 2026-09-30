//! Function regions of a solc-built EVM runtime, from its source map and AST
//! (`riff_catalog_solc::sourcemap`): the same `riffcat-regions/1` manifest the
//! Fe trace produces, so the run census, the dataflow lift and the facets
//! run on solc output unchanged.

use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use riff_catalog_evm::decode::decode;
use riff_catalog_solc::SolcOutput;
use riff_catalog_solc::sourcemap::{
    SourceOwner, attribute, function_spans, generated_function_spans, parse_source_map,
    solidity_source_ids,
};
use serde::{Deserialize, Serialize};

use crate::census::{EvmRunOptions, RegionManifest, RegionSpec};

pub const SOLC_REGIONS_ADAPTER: &str = "solc-source-map-functions/1";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SolcFunctions {
    pub source: String,
    pub contract: String,
    pub runtime_bytes: usize,
    /// End of the instructions (the CBOR metadata trailer starts here).
    pub code_end: usize,
    /// Bytes by owner: `Contract.function`, `(contract) C`, `(file) N`,
    /// `(generated yul)`, `(no source)`, and `(metadata)`.
    pub by_owner: Vec<(String, u64)>,
}

/// Read one contract's runtime from solc standard-JSON output and attribute
/// every instruction to its source function. Returns the runtime bytes, the
/// report, and a manifest whose `function` regions are the maximal runs of
/// instructions with one owner.
pub fn solc_functions(
    output: &SolcOutput,
    source: &str,
    contract: &str,
    evm_runs: Option<EvmRunOptions>,
) -> Result<(Vec<u8>, SolcFunctions, RegionManifest)> {
    let raw = output.raw();
    let pointer = |field: &str| {
        format!(
            "/contracts/{}/{}/evm/deployedBytecode/{field}",
            source.replace('~', "~0").replace('/', "~1"),
            contract.replace('~', "~0").replace('/', "~1")
        )
    };
    let object = raw
        .pointer(&pointer("object"))
        .and_then(|v| v.as_str())
        .context("deployedBytecode.object")?;
    let map = raw
        .pointer(&pointer("sourceMap"))
        .and_then(|v| v.as_str())
        .context("deployedBytecode.sourceMap")?;
    let hex = object.trim_start_matches("0x");
    let code: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect::<Result<_, _>>()
        .context("bytecode hex")?;
    let (code_end, _) = riff_catalog_evm::split_metadata(&code);
    let insts = decode(&code[..code_end]);
    let entries = parse_source_map(map);
    ensure!(
        entries.len() <= insts.len(),
        "source map has {} entries for {} instructions",
        entries.len(),
        insts.len()
    );
    let (mut functions, contracts) = function_spans(output);
    if let Some(deployed) = raw.pointer(&pointer("").trim_end_matches('/').to_string()) {
        functions.extend(generated_function_spans(deployed));
    }
    let owners = attribute(
        &entries,
        &functions,
        &contracts,
        &solidity_source_ids(output),
    );
    let name = |o: Option<&SourceOwner>| -> String {
        match o {
            Some(SourceOwner::Function(i)) => functions[*i].name.clone(),
            Some(SourceOwner::Contract(c)) => format!("(contract) {c}"),
            Some(SourceOwner::File(f)) => format!("(file) {f}"),
            Some(SourceOwner::Generated) => "(generated yul)".into(),
            Some(SourceOwner::NoSource) | None => "(no source)".into(),
        }
    };
    let mut by_owner: BTreeMap<String, u64> = BTreeMap::new();
    let mut regions: Vec<RegionSpec> = Vec::new();
    for (k, inst) in insts.iter().enumerate() {
        let owner = name(owners.get(k));
        *by_owner.entry(owner.clone()).or_default() += u64::from(inst.len);
        let (start, end) = (inst.pc as usize, (inst.pc + inst.len) as usize);
        match regions.last_mut() {
            Some(last) if last.name == owner && last.end == start => last.end = end,
            _ => regions.push(RegionSpec {
                id: format!("function:{start}"),
                kind: "function".into(),
                name: owner,
                start,
                end,
            }),
        }
    }
    if code_end < code.len() {
        *by_owner.entry("(metadata)".into()).or_default() += (code.len() - code_end) as u64;
        regions.push(RegionSpec {
            id: format!("data:{code_end}"),
            kind: "data".into(),
            name: "CBOR metadata".into(),
            start: code_end,
            end: code.len(),
        });
    }
    let mut by_owner: Vec<(String, u64)> = by_owner.into_iter().collect();
    by_owner.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let manifest = RegionManifest {
        schema: "riffcat-regions/1".into(),
        artifact_blake3: blake3::hash(&code).to_hex().to_string(),
        adapter: SOLC_REGIONS_ADAPTER.into(),
        regions,
        evm_runs,
    };
    let report = SolcFunctions {
        source: source.into(),
        contract: contract.into(),
        runtime_bytes: code.len(),
        code_end,
        by_owner,
    };
    Ok((code, report, manifest))
}
