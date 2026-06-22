//! Real solc output, baked into the wasm so the storybook is fully
//! self-contained: it runs offline, from `file://`, with no fetch and no
//! server. Each fixture is genuine compiler output for a real contract; the
//! storybook fingerprints it live in the browser via the engine. solc is not
//! shipped here, only its output, which is all the fingerprinter consumes.

use wasm_bindgen::prelude::*;

/// One baked fixture: `(id, label, level, json)`. `level` is `"yul"` (solc's
/// `irAst`) or `"source"` (solc's source `ast`) and selects which engine entry
/// point the UI feeds it to.
const FIXTURES: &[(&str, &str, &str, &str)] = &[
    (
        "demo",
        "Demo: small token, 0.8.x via-IR Yul",
        "yul",
        include_str!("../fixtures/demo.yul.json"),
    ),
    // Library cross-reference: three unrelated wrapper contracts, each vendoring
    // one library's full-precision mul·div. Compiled identically (via-IR, no
    // optimizer). The shared chunk is one fingerprint within a library and a
    // different one across libraries: the supply chain, in the shape.
    (
        "oz-muldiv",
        "OpenZeppelin · Math.mulDiv",
        "yul",
        include_str!("../fixtures/oz-muldiv.yul.json"),
    ),
    (
        "solady-muldiv",
        "Solady · FixedPointMathLib.fullMulDiv",
        "yul",
        include_str!("../fixtures/solady-muldiv.yul.json"),
    ),
    (
        "solmate-muldiv",
        "Solmate · FixedPointMathLib.mulDivDown",
        "yul",
        include_str!("../fixtures/solmate-muldiv.yul.json"),
    ),
];

/// JSON array of `{id, label, level}` for every baked fixture, for the picker.
#[wasm_bindgen]
pub fn fixture_index() -> String {
    let items: Vec<_> = FIXTURES
        .iter()
        .map(|(id, label, level, _)| {
            serde_json::json!({ "id": id, "label": label, "level": level })
        })
        .collect();
    serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
}

/// The raw solc JSON (irAst or source AST) for one fixture id, or `null`.
#[wasm_bindgen]
pub fn fixture(id: &str) -> Option<String> {
    FIXTURES
        .iter()
        .find(|(fid, ..)| *fid == id)
        .map(|(.., json)| (*json).to_string())
}

/// A fixture used only to warm the engine on startup (see `main`).
pub fn warmup() -> &'static str {
    FIXTURES[0].3
}
