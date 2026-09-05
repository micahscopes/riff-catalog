//! A small, compiler-independent ledger for code-growth observations.
//!
//! The schema deliberately separates compiler events, graph-derived totals,
//! artifact measurements, and compatibility traces. Missing evidence stays
//! absent. It is never silently converted to zero.

mod fe;
mod manifest;
mod model;
mod report;
mod validate;

pub use fe::{FeImport, import_fe_trace};
pub use manifest::{artifact_digest, capture_id, load_capture, save_capture, verify_artifacts};
pub use model::*;
pub use report::{CompareReport, Report, compare, render_compare_table, render_table, report};
pub use validate::{reachable_union, validate};
