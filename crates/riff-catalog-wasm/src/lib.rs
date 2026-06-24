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
    CyclePolicy, DigestRequest, DimensionDigests, Facet, Graph, GraphHashes, GraphKey, HashPolicy,
    NodeKey, ViewMode, digest_graph,
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

/// Project one node's `DimensionDigests` to `{dimension_name: hex}`, exactly the
/// shape the storybook reads (e.g. `{"structure": "9f01..", "names": ".."}`).
/// Mirrors how `digest_unit` serializes `hashes.graph.values`, one node deeper.
fn dimension_map(digests: &DimensionDigests) -> Value {
    let mut out = serde_json::Map::new();
    for (dimension, digest) in digests.iter() {
        out.insert(dimension.as_str().to_string(), json!(digest.to_hex()));
    }
    Value::Object(out)
}

/// Serialize the per-node `local`/`tree` digests, per-component digests, and the
/// whole-graph digest that `digest_graph` already computes for one graph. This is
/// the "the fold" chapter's substrate: it surfaces `GraphHashes.nodes` (the
/// `NodeHashes` per node, discarded by `digest_unit`), `GraphHashes.components`,
/// and `GraphHashes.graph`. Pure additive serialization, no new hashing.
///
/// `id` is each node's canonical key (a stable string); `parent`/`order` come
/// from the graph's `ChildEdge` skeleton (root parent is `null`), so the
/// consumer can lay out and fold bottom-up. Every digest is projected onto its
/// dimensions as `{dimension_name: hex}`, matching the facet readouts.
fn fold_payload(graph: &Graph, hashes: &GraphHashes) -> Value {
    // child key -> (parent key, ordinal), from the skeleton. A node with no
    // incoming child edge is a root (parent: null).
    let mut parent_of: std::collections::BTreeMap<&NodeKey, (&NodeKey, u32)> =
        std::collections::BTreeMap::new();
    for edge in &graph.children {
        parent_of.insert(&edge.child, (&edge.parent, edge.ordinal));
    }

    let nodes: Vec<Value> = hashes
        .nodes
        .iter()
        .map(|(key, node_hashes)| {
            let node = graph.nodes.get(key);
            let kind = node.map_or("node", |n| n.kind.as_str());
            let (parent, order) = match parent_of.get(key) {
                Some((parent, ordinal)) => (json!(parent.canonical_key()), *ordinal),
                None => (Value::Null, 0),
            };
            json!({
                "id": key.canonical_key(),
                "kind": kind,
                "parent": parent,
                "order": order,
                "local": dimension_map(&node_hashes.local),
                "tree": dimension_map(&node_hashes.tree),
            })
        })
        .collect();

    let components: Vec<Value> = hashes
        .components
        .iter()
        .map(|component| {
            let members: Vec<Value> = component
                .members
                .iter()
                .map(|m| json!(m.canonical_key()))
                .collect();
            json!({
                "index": component.component_index,
                "members": members,
                "digests": dimension_map(&component.digests),
            })
        })
        .collect();

    json!({
        "nodes": nodes,
        "components": components,
        "graph": dimension_map(&hashes.graph),
    })
}

/// Surface the per-node digests `digest_graph` computes for one lowered unit, for
/// the "the fold" chapter. Input is solc's `irAst` JSON for one object (same as
/// `fingerprint_yul`); it lowers the object and folds one small fn-like unit (see
/// `node_digests_impl` for the selection), so the payload reads as a small tree
/// with statements, an expression, and leaves. `mode` is "shape" (default,
/// anonymous) or "identity", exactly as the other exports. Returns the JSON the
/// `fold-merkle` element consumes: `nodes` (with `local`/`tree` per dimension),
/// `components`, and the `graph` digest.
#[wasm_bindgen]
pub fn node_digests(ir_ast_json: &str, mode: &str) -> Result<String, JsValue> {
    node_digests_impl(ir_ast_json, mode).map_err(js_err)
}

/// Native-testable core of [`node_digests`]: lower the object, fold one small
/// fn-like unit, and serialize its per-node digests. Errors are plain strings so
/// the wasm wrapper can map them to `JsValue` and tests can assert on them.
fn node_digests_impl(ir_ast_json: &str, mode: &str) -> Result<String, String> {
    use riff_catalog_yul::from_solc_value;
    use riff_catalog_yul::lower::{LowerOptions, YUL_AST_LEVEL, LoweredUnit, lower_object};
    let value: Value = serde_json::from_str(ir_ast_json).map_err(|e| e.to_string())?;
    let object = from_solc_value(&value).map_err(|e| e.to_string())?;
    let lowered =
        lower_object(&object, "wasm", &LowerOptions::default()).map_err(|e| e.to_string())?;

    // The fold reads best on a small function body: a few statements, an
    // expression, and a leaf, laid out in a handful of layers. Among functions
    // that carry both an assignment (a statement) and a call (an expression),
    // pick the one whose node count is closest to the chapter's reference size
    // (8), tie-broken by fewer nodes then by name so the choice is stable across
    // builds. Fall back to the largest function, then to the object graph.
    const FOLD_TARGET_NODES: usize = 8;
    let fn_like = |u: &&LoweredUnit| -> bool {
        let mut has_assign = false;
        let mut has_call = false;
        for node in u.graph.nodes.values() {
            match node.kind.as_str() {
                "yul.assign" => has_assign = true,
                "yul.call" => has_call = true,
                _ => {}
            }
        }
        has_assign && has_call
    };
    let unit = lowered
        .functions
        .iter()
        .filter(fn_like)
        .min_by(|a, b| {
            let n = |u: &LoweredUnit| u.graph.nodes.len();
            let key =
                |u: &LoweredUnit| (n(u).abs_diff(FOLD_TARGET_NODES), n(u), u.name.clone());
            key(a).cmp(&key(b))
        })
        .or_else(|| lowered.functions.iter().max_by_key(|u| u.graph.nodes.len()))
        .unwrap_or(&lowered.object);

    let policy = HashPolicy::new(YUL_AST_LEVEL, view(mode), CyclePolicy::CondenseScc)
        .map_err(|e| e.to_string())?;
    let hashes = digest_graph(
        &DigestRequest::all_dimensions(unit.graph_key.clone(), policy),
        &unit.graph,
    )
    .map_err(|e| e.to_string())?
    .hashes;

    serde_json::to_string(&fold_payload(&unit.graph, &hashes)).map_err(|e| e.to_string())
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

/// The full fingerprint of a raw pitch-class set: the three facet rungs and the
/// set-theory representations the engine derives from structure (no notation).
/// Shared by `fingerprint_chord` (which parses notation first) and
/// `fingerprint_pcs` (which is handed pitch classes directly), so the demo never
/// has to reimplement the Tn-type, prime form, or inversion in JavaScript.
fn pcs_fingerprint(pcs: &[i32]) -> Result<serde_json::Value, JsValue> {
    use riff_catalog_core::Dimension;
    use riff_catalog_music::{
        PITCH_CLASS_SET, encode_pitch_class_set, facet_hex,
        set_theory::{interval_vector, prime_form, transposition_normal_form},
    };
    let (key, graph) = encode_pitch_class_set("chord", pcs).map_err(js_err)?;
    // Three rungs: note_set keys on the literal pitch-class set (two spellings
    // of the same notes collapse); transposition_normal keys on the minimal
    // rotation (all transpositions collapse, matching polyphonotopes-math's
    // normalFormBits); set_class keys on the Forte prime form (transposition and
    // inversion, so major, minor, and other inversions of one class collapse).
    let tnf = transposition_normal_form(pcs);
    let (tk, tg) = encode_pitch_class_set("tnf", &tnf).map_err(js_err)?;
    let pf = prime_form(pcs);
    let (ck, cg) = encode_pitch_class_set("class", &pf).map_err(js_err)?;
    let full = Dimension::ALL.to_vec();
    Ok(json!({
        "pitch_classes": pcs,
        "prime_form": pf,
        "transposition_normal_form": tnf,
        "interval_vector": interval_vector(pcs),
        "note_set": facet_hex(&key, &graph, &PITCH_CLASS_SET).map_err(js_err)?,
        "transposition_normal": facet_hex(&tk, &tg, &PITCH_CLASS_SET).map_err(js_err)?,
        "set_class": facet_hex(&ck, &cg, &PITCH_CLASS_SET).map_err(js_err)?,
        "full": facet_hex(&key, &graph, &full).map_err(js_err)?,
    }))
}

/// Fingerprint a chord written as a real notation string. Parses it with the
/// vibe-grammars pest parser, derives its pitch-class set, and returns the
/// note-set facet address (plus full). A real string parsed to a structural
/// fingerprint: the music analogue of parsing Solidity source.
#[wasm_bindgen]
pub fn fingerprint_chord(notation: &str) -> Result<String, JsValue> {
    use riff_catalog_music::chord::chord_to_pitch_classes;
    let pcs = chord_to_pitch_classes(notation).map_err(js_err)?;
    let mut out = pcs_fingerprint(&pcs)?;
    out.as_object_mut()
        .unwrap()
        .insert("notation".into(), json!(notation));
    serde_json::to_string(&out).map_err(js_err)
}

/// Fingerprint a raw pitch-class set, optionally inverting it first by the I
/// generator (x -> (12 - x) mod 12). This lets the demo address a set it built
/// itself (an inverted chord) through the same engine that handled the parsed
/// chord, so the Tn-type, prime form, and interval vector on screen are always
/// computed, never reimplemented in JavaScript. `pcs_json` is a JSON array of
/// integers.
#[wasm_bindgen]
pub fn fingerprint_pcs(pcs_json: &str, invert: bool) -> Result<String, JsValue> {
    let mut pcs: Vec<i32> = serde_json::from_str(pcs_json).map_err(js_err)?;
    if invert {
        pcs = pcs.iter().map(|x| (12 - x).rem_euclid(12)).collect();
    }
    pcs.sort_unstable();
    pcs.dedup();
    let out = pcs_fingerprint(&pcs)?;
    serde_json::to_string(&out).map_err(js_err)
}

#[cfg(test)]
mod tests {
    use super::oklch_chip;

    const DEMO_YUL: &str = include_str!("../../../demo/app/fixtures/demo.yul.json");

    #[test]
    fn node_digests_match_fold_shape() {
        let out = super::node_digests_impl(DEMO_YUL, "shape").expect("node_digests");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let nodes = v["nodes"].as_array().expect("nodes array");
        assert!(!nodes.is_empty(), "fold needs at least one node");
        assert!(nodes.len() <= 16, "fold unit should stay small, got {}", nodes.len());
        // Exactly one root (parent: null), the others all parented in the skeleton.
        let roots = nodes.iter().filter(|n| n["parent"].is_null()).count();
        assert_eq!(roots, 1, "a tree has exactly one root");
        // Every field the consumer reads is present and well-typed.
        for n in nodes {
            assert!(n["id"].is_string());
            assert!(n["kind"].is_string());
            assert!(n["order"].is_u64());
            for facet in ["local", "tree"] {
                let map = n[facet].as_object().expect("dimension map");
                for dim in ["structure", "names", "constants", "types"] {
                    let hex = map[dim].as_str().expect("hex");
                    assert_eq!(hex.len(), 64, "{dim} digest is 32-byte hex");
                }
            }
        }
        assert!(v["components"].is_array());
        let graph = v["graph"].as_object().expect("graph digests");
        assert_eq!(graph["structure"].as_str().unwrap().len(), 64);
    }

    #[test]
    fn node_digests_is_deterministic_and_shape_anonymizes() {
        // Same input, same bytes (the digests are a pure function of the graph).
        let a = super::node_digests_impl(DEMO_YUL, "shape").unwrap();
        let b = super::node_digests_impl(DEMO_YUL, "shape").unwrap();
        assert_eq!(a, b, "node_digests is deterministic");
        // shape (anonymous) and identity differ: names/paths enter only in identity.
        let id = super::node_digests_impl(DEMO_YUL, "identity").unwrap();
        assert_ne!(a, id, "shape view should not equal identity view");
    }

    #[test]
    fn chip_color_stable_distinct_and_formatted() {
        let a = oklch_chip("c80e6c0c0bab");
        assert_eq!(a, oklch_chip("c80e6c0c0bab"), "same digest, same color");
        assert_ne!(a, oklch_chip("394376dc71f0"), "different digest, different color");
        assert!(a.starts_with("oklch(") && a.ends_with(')'), "got {a}");
    }
}
