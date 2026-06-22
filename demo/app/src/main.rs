//! The storybook's wasm entry point.
//!
//! The UI is vanilla web components (see `index.html`) that own the page and
//! drive the fingerprinting engine from JavaScript through
//! `window.wasmBindings`. This crate's job is to (a) expose the baked fixtures
//! (`fixtures`) and (b) make the engine's exports land in `wasmBindings`.
//!
//! `main` deliberately does not touch the DOM. It only warms the engine once,
//! which both references the `riff-catalog-wasm` exports (so wasm-bindgen keeps
//! them in `wasmBindings`) and pays the one-time codegen/allocation cost up
//! front, so the first interactive fingerprint in the UI is instant.

mod fixtures;

fn main() {
    let _ = riff_catalog_wasm::fingerprint_yul(fixtures::warmup(), "shape");
}
