//! riff-catalog-solidity: level "sol-ast/1".
//!
//! The Solidity AST is ~150 node types and version-volatile (e.g.
//! `isSimpleCounterLoop` appeared mid-0.8.x), so this crate walks the raw
//! JSON generically, driven by a declarative PROFILE: per `nodeType`, which
//! fields are ordered children, which feed Names/Structure/Constants/Types,
//! and which spawn new graph units. Unknown *fields* are simply never read;
//! unknown *node types* are an error in strict mode and a tagged fallback
//! otherwise. Volatile producer artifacts — numeric `id`s, `src` offsets —
//! never enter keys or payloads (invariant I15); `referencedDeclaration`
//! becomes a Reference edge to the path-keyed declaration node when it
//! resolves within the same graph.

mod error;
mod profile;
mod walker;

pub use error::SolLowerError;
pub use profile::spec_for;
pub use walker::{LoweredSol, LoweredUnit, WalkOptions, lower_source_unit};

/// The versioned level string for this lowering (invariant I10).
pub const SOL_AST_LEVEL: &str = "sol-ast/1";
