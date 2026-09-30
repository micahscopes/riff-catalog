//! Function regions of a solc-built EVM runtime, from its source map and AST
//! (`riff_catalog_solc::sourcemap`): the same `riffcat-regions/1` manifest the
//! Fe trace produces, so the run census, the dataflow lift and the facets
//! run on solc output unchanged.

use std::collections::BTreeMap;

use anyhow::{Result, ensure};
use riff_catalog_evm::decode::decode;
use riff_catalog_solc::SolcOutput;
use riff_catalog_solc::sourcemap::{
    SourceOwner, attribute, function_spans, generated_function_spans, parse_source_map,
    solidity_source_ids,
};
use serde::{Deserialize, Serialize};

use riff_catalog_bloat::{EvmRunOptions, RegionManifest, RegionSpec};

pub const SOLC_REGIONS_ADAPTER: &str = "solc-source-map-functions/1";
pub const SOLC_FUNCTIONS_SCHEMA: &str = "riffcat-solc-functions/1";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SolcFunctions {
    pub schema: String,
    pub source: String,
    pub contract: String,
    pub runtime_bytes: usize,
    pub runtime_blake3: String,
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
    let code = output.deployed_bytecode(source, contract)?;
    let map = output.deployed_source_map(source, contract)?;
    let (code_end, _) = riff_catalog_evm::split_metadata(&code);
    let insts = decode(&code[..code_end]);
    let entries = parse_source_map(map)?;
    // One entry per instruction; solc leaves a final INVALID without one.
    let final_invalid = insts.last().is_some_and(|i| i.opcode == 0xfe);
    ensure!(
        !entries.is_empty()
            && (entries.len() == insts.len()
                || (final_invalid && entries.len() + 1 == insts.len())),
        "source map has {} entries for {} instructions: it does not describe this bytecode",
        entries.len(),
        insts.len()
    );
    let (mut functions, contracts) = function_spans(output);
    functions.extend(generated_function_spans(output.deployed(source, contract)?));
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
        schema: RegionManifest::schema_for(evm_runs.as_ref()).into(),
        artifact_blake3: blake3::hash(&code).to_hex().to_string(),
        adapter: SOLC_REGIONS_ADAPTER.into(),
        regions,
        evm_runs,
    };
    let report = SolcFunctions {
        schema: SOLC_FUNCTIONS_SCHEMA.into(),
        source: source.into(),
        contract: contract.into(),
        runtime_bytes: code.len(),
        runtime_blake3: blake3::hash(&code).to_hex().to_string(),
        code_end,
        by_owner,
    };
    Ok((code, report, manifest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(object: &str, map: &str) -> SolcOutput {
        SolcOutput::new(serde_json::json!({
            "sources": {"a.sol": {"id": 0, "ast": {
                "nodeType": "SourceUnit", "src": "0:100:0", "nodes": [
                    {"nodeType": "ContractDefinition", "name": "C", "src": "0:100:0", "nodes": [
                        {"nodeType": "FunctionDefinition", "name": "f", "src": "10:50:0"}
                    ]}
                ]}}},
            "contracts": {"a.sol": {"C": {"evm": {"deployedBytecode": {
                "object": object, "sourceMap": map
            }}}}}
        }))
    }

    #[test]
    fn a_source_map_must_describe_the_instructions() {
        // PUSH1 1, PUSH1 2, ADD, STOP: four instructions.
        for map in ["", "10:1:0", "10:1:0;;", "10:1:0;;;;"] {
            assert!(
                solc_functions(
                    &output("6001600201 00".replace(' ', "").as_str(), map),
                    "a.sol",
                    "C",
                    None
                )
                .is_err(),
                "map `{map}` accepted"
            );
        }
        solc_functions(&output("600160020100", "10:1:0;;;"), "a.sol", "C", None)
            .expect("one entry per instruction");
        // solc leaves the final INVALID of a runtime without an entry.
        solc_functions(&output("600160020100fe", "10:1:0;;;"), "a.sol", "C", None)
            .expect("no entry for a final INVALID");
    }

    #[test]
    fn bad_bytecode_hex_is_an_error() {
        assert!(solc_functions(&output("600", "10:1:0"), "a.sol", "C", None).is_err());
        assert!(solc_functions(&output("60zz", "10:1:0"), "a.sol", "C", None).is_err());
        assert!(solc_functions(&output("6000", "10:1:0"), "a.sol", "D", None).is_err());
        let (code, report, _) =
            solc_functions(&output("0x6001", "10:1:0"), "a.sol", "C", None).unwrap();
        assert_eq!(code, vec![0x60, 0x01]);
        assert_eq!(report.by_owner, vec![("C.f".to_string(), 2)]);
    }
}
