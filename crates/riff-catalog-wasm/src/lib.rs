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

// --- chip colors ---------------------------------------------------------
// The storybook colors each function by its shape digest, so the same shape is
// the same color everywhere. HSL made that look uneven: a yellow and a blue at
// the same "lightness" number do not read as equally bright. OKLCH is
// perceptually uniform, so every chip reads with the same weight. Lightness is
// fixed; chroma is reduced (via palette) until the color lands inside sRGB, so
// each hue stays as vivid as it can honestly carry without clipping.

/// Core, host-testable: a stable, gamut-safe `oklch(...)` CSS color for a digest.
fn oklch_chip(digest_hex: &str) -> String {
    use palette::{FromColor, Oklch, Srgb};
    // Spread the hash by the golden angle so digests that are close in hex still
    // land on visibly different hues.
    let mut acc: u64 = 0;
    for b in digest_hex.bytes().take(12) {
        acc = acc.wrapping_mul(131).wrapping_add(u64::from(b));
    }
    let hue = ((acc as f64) * 137.507_764).rem_euclid(360.0) as f32;
    let lightness = 0.72_f32;
    let mut chroma = 0.15_f32;
    while chroma > 0.0 {
        let rgb = Srgb::from_color(Oklch::new(lightness, chroma, hue));
        if [rgb.red, rgb.green, rgb.blue]
            .iter()
            .all(|c| (0.0..=1.0).contains(c))
        {
            break;
        }
        chroma -= 0.005;
    }
    format!("oklch({:.0}% {:.3} {:.1})", lightness * 100.0, chroma, hue)
}

/// A perceptually-uniform chip color for a shape digest, as a CSS `oklch(...)`
/// string: deterministic in the digest (same shape, same color in every
/// chapter) and gamut-safe (always renders).
#[wasm_bindgen]
pub fn chip_color(digest_hex: &str) -> String {
    oklch_chip(digest_hex)
}

/// Fingerprint a chord written as a real notation string. Parses it with the
/// vibe-grammars pest parser, derives its pitch-class set, and returns the
/// note-set facet address (plus full). A real string parsed to a structural
/// fingerprint: the music analogue of parsing Solidity source.
#[wasm_bindgen]
pub fn fingerprint_chord(notation: &str) -> Result<String, JsValue> {
    use riff_catalog_core::Dimension;
    use riff_catalog_music::{
        PITCH_CLASS_SET, chord::chord_to_pitch_classes, encode_pitch_class_set, facet_hex,
        set_theory::{interval_vector, prime_form, transposition_normal_form},
    };
    let pcs = chord_to_pitch_classes(notation).map_err(js_err)?;
    let (key, graph) = encode_pitch_class_set("chord", &pcs).map_err(js_err)?;
    // Three rungs: note_set keys on the literal pitch-class set (two spellings
    // of the same notes collapse); transposition_normal keys on the minimal
    // rotation (all transpositions collapse, matching polyphonotopes-math's
    // normalFormBits); set_class keys on the Forte prime form (transposition and
    // inversion, so major, minor, and other inversions of one class collapse).
    let tnf = transposition_normal_form(&pcs);
    let (tk, tg) = encode_pitch_class_set("tnf", &tnf).map_err(js_err)?;
    let pf = prime_form(&pcs);
    let (ck, cg) = encode_pitch_class_set("class", &pf).map_err(js_err)?;
    let full = Dimension::ALL.to_vec();
    let out = json!({
        "notation": notation,
        "pitch_classes": pcs,
        "prime_form": pf,
        "interval_vector": interval_vector(&pcs),
        "note_set": facet_hex(&key, &graph, &PITCH_CLASS_SET).map_err(js_err)?,
        "transposition_normal": facet_hex(&tk, &tg, &PITCH_CLASS_SET).map_err(js_err)?,
        "set_class": facet_hex(&ck, &cg, &PITCH_CLASS_SET).map_err(js_err)?,
        "full": facet_hex(&key, &graph, &full).map_err(js_err)?,
    });
    serde_json::to_string(&out).map_err(js_err)
}

#[cfg(test)]
mod tests {
    use super::oklch_chip;

    #[test]
    fn chip_color_stable_distinct_and_formatted() {
        let a = oklch_chip("c80e6c0c0bab");
        assert_eq!(a, oklch_chip("c80e6c0c0bab"), "same digest, same color");
        assert_ne!(a, oklch_chip("394376dc71f0"), "different digest, different color");
        assert!(a.starts_with("oklch(") && a.ends_with(')'), "got {a}");
    }
}
