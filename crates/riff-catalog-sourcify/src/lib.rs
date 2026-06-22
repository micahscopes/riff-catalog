//! riff-catalog-sourcify: fetch verified contracts from Sourcify (an Argot
//! Collective service, like solc — this is dogfooding) and turn them into
//! standard-json inputs for local recompilation.
//!
//! Endpoint notes live in `api`; everything is cache-first so a demo re-run
//! is fully offline.

mod api;
mod error;
mod resolver;

pub use api::{ContractId, SourcifyClient, VerifiedContract};
pub use error::SourcifyError;
pub use resolver::{PinnedOrDownload, SolcBinResolver};

use riff_catalog_solc::{
    Pipeline, SolcOutput, SolcResolver, strip_json_ir_outputs, with_output_selection,
};

/// Build the standard-json input: prefer the verbatim `stdJsonInput` when
/// Sourcify has it; otherwise reconstruct from metadata (language, sources,
/// settings minus the verification-only `compilationTarget`).
pub fn to_standard_json(contract: &VerifiedContract) -> Result<serde_json::Value, SourcifyError> {
    if let Some(input) = &contract.std_json_input {
        return Ok(input.clone());
    }
    let metadata = &contract.metadata;
    let language = metadata
        .pointer("/language")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Solidity");
    let mut settings = metadata
        .pointer("/settings")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(map) = settings.as_object_mut() {
        map.remove("compilationTarget");
        // Metadata spells libraries flat ("file.sol:Lib": addr); standard
        // JSON nests them ({file: {Lib: addr}}). Dropping them entirely
        // produced unlinked placeholders in recompiled bytecode (external
        // review pass 2, P2) — convert instead.
        if let Some(libraries) = map.remove("libraries") {
            if let Some(flat) = libraries.as_object() {
                let mut nested = serde_json::Map::new();
                for (qualified, address) in flat {
                    let (file, library) = qualified
                        .split_once(':')
                        .unwrap_or(("", qualified.as_str()));
                    nested
                        .entry(file.to_string())
                        .or_insert_with(|| serde_json::json!({}))
                        .as_object_mut()
                        .expect("just inserted object")
                        .insert(library.to_string(), address.clone());
                }
                if !nested.is_empty() {
                    map.insert("libraries".into(), nested.into());
                }
            }
        }
    }
    let sources: serde_json::Map<String, serde_json::Value> = contract
        .sources
        .iter()
        .map(|(path, content)| (path.clone(), serde_json::json!({ "content": content })))
        .collect();
    Ok(serde_json::json!({
        "language": language,
        "sources": sources,
        "settings": settings,
    }))
}

/// Resolve a solc for the contract's pinned compiler version and compile
/// with our output selection forced.
pub fn compile(
    contract: &VerifiedContract,
    resolver: &dyn SolcResolver,
    pipeline: Pipeline,
) -> Result<SolcOutput, SourcifyError> {
    let runner = resolver
        .resolve(&contract.compiler_version)
        .map_err(SourcifyError::Solc)?;
    let input = with_output_selection(to_standard_json(contract)?, pipeline);
    let output = runner.compile(&input).map_err(SourcifyError::Solc)?;
    if output.check_errors().is_ok() {
        return Ok(output);
    }
    // solc can ICE serializing the JSON-IR outputs (irAst/irOptimizedAst/
    // yulCFGJson) on some ~0.8.25–0.8.29 contracts, which would otherwise lose
    // the whole contract. Retry without them: the source-, text-Yul-, and
    // bytecode-level fingerprints still land (the ingest parses the text IR;
    // only the SSA level is dropped).
    let reduced = strip_json_ir_outputs(input);
    runner.compile(&reduced).map_err(SourcifyError::Solc)
}
