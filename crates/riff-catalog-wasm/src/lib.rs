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
    CyclePolicy, DigestRequest, Facet, Graph, GraphHashes, GraphKey, HashPolicy, ViewMode,
    digest_graph,
};

fn js_err(msg: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&msg.to_string())
}

/// The named facets the storybook dials between, each as `name -> hex`. These
/// are facet *addresses* (`FacetAddress::address_digest`), the exact value the
/// CLI compares for "equal at this facet", so a digest shown here is the same
/// one the corpus indexes on. `full` keeps every dimension, `names-blind`
/// forgets only names, `structure` forgets everything but shape.
fn facet_addresses(hashes: &GraphHashes) -> Result<Value, JsValue> {
    let pid = hashes.policy_id;
    let named = [
        ("full", Facet::full(pid)),
        ("names-blind", Facet::names_blind(pid)),
        ("structure", Facet::structure_only(pid)),
    ];
    let mut out = serde_json::Map::new();
    for (name, facet) in named {
        let address = hashes.facet_address(&facet).map_err(js_err)?;
        out.insert(name.to_string(), json!(address.address_digest().to_hex()));
    }
    Ok(Value::Object(out))
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
    let facets = facet_addresses(&hashes)?;
    Ok(json!({ "unit": unit, "name": name, "digests": digests, "facets": facets }))
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

/// Fingerprint a melodic riff (and its pitch-class set). This is the engine on a
/// second, audible domain: the very same facet machinery, no Solidity in sight.
/// Input is `{"notes":[{"pitch":<midi int>,"dur":<uint>}, ...]}`; output is the
/// "equal at facet" address for each musical facet, so the storybook can group
/// riffs by shape the way it groups code: rhythm (durations), harmonic
/// relationships (intervals, transposition invariant), pitch-class set, and full.
#[wasm_bindgen]
pub fn fingerprint_riff(riff_json: &str) -> Result<String, JsValue> {
    use riff_catalog_core::Dimension;
    use riff_catalog_music::{
        HARMONIC_RELATIONSHIPS, Note, PITCH_CLASS_SET, RHYTHM, encode_pitch_class_set, encode_riff,
        facet_hex,
    };
    let value: Value = serde_json::from_str(riff_json).map_err(js_err)?;
    let raw = value
        .get("notes")
        .and_then(|n| n.as_array())
        .ok_or_else(|| js_err("riff json must have a \"notes\" array"))?;
    let mut notes = Vec::with_capacity(raw.len());
    let mut pitches = Vec::with_capacity(raw.len());
    for n in raw {
        let pitch = n
            .get("pitch")
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| js_err("each note needs an integer \"pitch\""))? as i32;
        let dur = n
            .get("dur")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| js_err("each note needs an unsigned \"dur\""))? as u32;
        notes.push(Note { pitch, dur });
        pitches.push(pitch);
    }
    let (rk, rg) = encode_riff("riff", &notes).map_err(js_err)?;
    let (pk, pg) = encode_pitch_class_set("riff", &pitches).map_err(js_err)?;
    let full = Dimension::ALL.to_vec();
    let out = json!({
        "full": facet_hex(&rk, &rg, &full).map_err(js_err)?,
        "harmonic_relationships": facet_hex(&rk, &rg, &HARMONIC_RELATIONSHIPS).map_err(js_err)?,
        "rhythm": facet_hex(&rk, &rg, &RHYTHM).map_err(js_err)?,
        "pitch_class_set": facet_hex(&pk, &pg, &PITCH_CLASS_SET).map_err(js_err)?,
    });
    serde_json::to_string(&out).map_err(js_err)
}
