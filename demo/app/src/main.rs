//! The "it runs in the browser" proof. On load, fingerprints a real contract's
//! Yul (an embedded irAst fixture) entirely client-side via the riff-catalog-wasm
//! crate, and renders the result. No backend, no server: riffcat compiled to wasm.
//!
//! In the full demo, the fixture is replaced by solc output computed live in a
//! Web Worker (soljson). This proves the riffcat half of that pipeline.

const FIXTURE: &str = include_str!("../fixture_yul.json");

fn main() {
    let body = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.body())
        .expect("a document body");

    let html = match riff_catalog_wasm::fingerprint_yul(FIXTURE, "shape") {
        Ok(json) => {
            let units: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
            let n = units.as_array().map(Vec::len).unwrap_or(0);
            let rows = units
                .as_array()
                .into_iter()
                .flatten()
                .map(|u| {
                    let name = u.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                    let unit = u.get("unit").and_then(|v| v.as_str()).unwrap_or("?");
                    let st = u
                        .pointer("/digests/structure")
                        .and_then(|v| v.as_str())
                        .map(|s| &s[..s.len().min(16)])
                        .unwrap_or("");
                    format!("<tr><td>{unit}</td><td>{name}</td><td class=fp>{st}</td></tr>")
                })
                .collect::<String>();
            format!(
                "<h1>riffcat, in the browser</h1>\
                 <p>Fingerprinted the <code>Demo</code> contract's Yul client-side \
                 (wasm, no backend): <b>{n}</b> units, names-blind structure facet.</p>\
                 <table><tr><th>unit</th><th>name</th><th>structure</th></tr>{rows}</table>"
            )
        }
        Err(e) => format!("<pre>error: {e:?}</pre>"),
    };
    body.set_inner_html(&html);
}
