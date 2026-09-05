//! A small, compiler-independent ledger for code-growth observations.
//!
//! The schema deliberately separates compiler events, graph-derived totals,
//! artifact measurements, and compatibility traces. Missing evidence stays
//! absent. It is never silently converted to zero.

mod census;
mod census_store;
mod fe;
mod fe_events;
mod manifest;
mod model;
mod report;
mod validate;

pub use census::{
    ArtifactCensus, CensusCaptureContext, CensusRegion, PatternGroup, RegionManifest, RegionSpec,
    census_capture, census_file, census_regions, census_wgsl, render_census,
};
pub use census_store::{
    CensusComparison, CensusFile, CensusSource, compare_censuses, render_census_comparison,
    replay_census, save_census,
};
pub use fe::{FeImport, import_fe_trace};
pub use fe_events::{FeEventsImport, import_fe_events};
pub use manifest::{artifact_digest, capture_id, load_capture, save_capture, verify_artifacts};
pub use model::*;
pub use report::{CompareReport, Report, compare, render_compare_table, render_table, report};
pub use validate::{reachable_union, validate};
