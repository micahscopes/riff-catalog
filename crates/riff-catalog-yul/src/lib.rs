//! riff-catalog-yul: one Yul AST, two front doors, one lowering.
//!
//! The AST types in [`ast`] deserialize **directly** from solc's `irAst` /
//! `irOptimizedAst` JSON, and the text [`parser`] produces the same types
//! from Yul source (solc's `ir` output, hand-written `.yul`, fe-emitted Yul).
//! There is deliberately no trait between the two paths: derived `PartialEq`
//! on the shared type IS conformance level 0 — the two representations of
//! the same program cannot drift apart silently (invariant I10/I1 of
//! PLAN.md; the canonical-encoding-drift hazard from the interop plan).
//!
//! Lowerings:
//! - [`lower`] (level "yul-ast/1"): the syntax tree into a riff-catalog
//!   graph — rich Names/Constants/Types dimensions.
//! - [`ssa`] (level "yul-ssa-cfg/1"): solc's experimental `yulCFGJson`
//!   (SSA basic blocks, names erased by construction) — the cyclic-CFG
//!   showcase for the WL machinery.

pub mod ast;
pub mod builtins;
pub mod canon;
pub mod error;
pub mod lower;
pub mod parser;

mod lexer;

pub use ast::Object;
pub use error::{YulLowerError, YulParseError};
pub use parser::parse_object;

/// Deserialize solc's `irAst`/`irOptimizedAst` JSON into the shared AST.
pub fn from_solc_value(value: &serde_json::Value) -> Result<Object, serde_json::Error> {
    serde_json::from_value(value.clone())
}
