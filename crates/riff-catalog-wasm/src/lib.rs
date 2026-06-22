//! Browser-side fingerprinting, compiled to wasm.
//!
//! solc itself runs separately (the official `soljson-vX.js` in a Web Worker);
//! this crate takes solc's output (irAst JSON, source AST JSON) and returns the
//! facet fingerprints. It is a thin wrapper over the exact same
//! `lower_* -> digest_graph` pipeline the CLI uses to emit units, so a digest
//! computed here is byte-identical to one computed natively. No changes to the
//! four lowering/core crates: they compile to wasm unchanged.

use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

use riff_catalog_core::{
    CyclePolicy, DigestRequest, Graph, GraphKey, HashPolicy, ViewMode, digest_graph,
};

fn js_err(msg: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&msg.to_string())
}

/// "identity" pins the artifact (path-bound); anything else is the anonymous
/// shape view (the default), where names/paths never enter the digest.
fn view(mode: &str) -> ViewMode {
    match mode {
        "identity" => ViewMode::IdentityBound,
        _ => ViewMode::AnonymousShape,
    }
}

/// One unit's fingerprints as `{unit, name, digests:{dimension: hex}}`.
fn digest_unit(
    level: &str,
    view_mode: ViewMode,
    unit: &str,
    name: &str,
    graph_key: &GraphKey,
    graph: &Graph,
) -> Result<Value, JsValue> {
    let policy = HashPolicy::new(level, view_mode, CyclePolicy::CondenseScc).map_err(js_err)?;
    let hashes = digest_graph(
        &DigestRequest::all_dimensions(graph_key.clone(), policy),
        graph,
    )
    .map_err(js_err)?
    .hashes;
    let digests = serde_json::to_value(&hashes.graph.values).map_err(js_err)?;
    Ok(json!({ "unit": unit, "name": name, "digests": digests }))
}

/// Fingerprint a contract's Yul. Input is solc's `irAst` JSON for one object
/// (the via-IR AST); output is a JSON array of unit fingerprints (the object
/// graph plus one per function). `mode` is "shape" (default) or "identity".
#[wasm_bindgen]
pub fn fingerprint_yul(ir_ast_json: &str, mode: &str) -> Result<String, JsValue> {
    use riff_catalog_yul::from_solc_value;
    use riff_catalog_yul::lower::{LowerOptions, YUL_AST_LEVEL, lower_object};
    let value: Value = serde_json::from_str(ir_ast_json).map_err(js_err)?;
    let object = from_solc_value(&value).map_err(js_err)?;
    let lowered = lower_object(&object, "wasm", &LowerOptions::default()).map_err(js_err)?;
    let view_mode = view(mode);
    let mut out = Vec::new();
    for unit in std::iter::once(&lowered.object).chain(lowered.functions.iter()) {
        out.push(digest_unit(
            YUL_AST_LEVEL,
            view_mode,
            unit.unit,
            &unit.name,
            &unit.graph_key,
            &unit.graph,
        )?);
    }
    serde_json::to_string(&out).map_err(js_err)
}

/// Fingerprint a contract's Solidity source. Input is solc's source `ast` JSON;
/// output is the source unit plus one fingerprint per contract and function.
/// This is the version-stable level (the source AST does not drift with the
/// compiler), so it is the right substrate for "same library code".
#[wasm_bindgen]
pub fn fingerprint_source(ast_json: &str, mode: &str) -> Result<String, JsValue> {
    use riff_catalog_solidity::{SOL_AST_LEVEL, WalkOptions, lower_source_unit};
    let ast: Value = serde_json::from_str(ast_json).map_err(js_err)?;
    let lowered = lower_source_unit(&ast, "wasm", &WalkOptions { strict: false }).map_err(js_err)?;
    let view_mode = view(mode);
    let mut out = Vec::new();
    for unit in std::iter::once(&lowered.source_unit)
        .chain(&lowered.contracts)
        .chain(&lowered.functions)
    {
        out.push(digest_unit(
            SOL_AST_LEVEL,
            view_mode,
            unit.unit,
            &unit.name,
            &unit.graph_key,
            &unit.graph,
        )?);
    }
    serde_json::to_string(&out).map_err(js_err)
}
