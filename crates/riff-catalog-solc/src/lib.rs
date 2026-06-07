//! riff-catalog-solc: a thin, typed driver for `solc --standard-json`.
//!
//! Deliberately has no dependency on riff-catalog-core: this crate only gets
//! artifacts *out* of solc (AST JSON, Yul IR text/AST, the experimental SSA
//! CFG via `yulCFGJson`, bytecode); lowering them into graphs is the
//! lowering crates' job. That keeps the process driver reusable by the
//! sourcify fetcher and the conformance harness alike.

mod error;
mod input;
mod output;
mod resolve;
mod runner;

pub use error::{SolcDiagnostic, SolcError};
pub use input::{CompileOptions, Pipeline, solidity_input, with_output_selection, yul_input};
pub use output::SolcOutput;
pub use resolve::{InstalledSolc, SolcResolver};
pub use runner::{CachedSolc, SolcRunner};
