//! riff-catalog-sourcify: fetch verified contracts from Sourcify (an Argot
//! Collective service, like solc — this is dogfooding) and turn them into
//! standard-json inputs for local recompilation.
//!
//! Endpoint notes live in `api`; everything is cache-first so a demo re-run
//! is fully offline.

mod api;
mod error;

pub use api::{ContractId, SourcifyClient, VerifiedContract};
pub use error::SourcifyError;

use riff_catalog_solc::{Pipeline, SolcOutput, SolcResolver, with_output_selection};

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
        map.remove("libraries");
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
    runner.compile(&input).map_err(SourcifyError::Solc)
}
