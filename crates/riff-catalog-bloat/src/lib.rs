//! A small, compiler-independent ledger for code-growth observations.
//!
//! The schema deliberately separates compiler events, graph-derived totals,
//! artifact measurements, and compatibility traces. Missing evidence stays
//! absent. It is never silently converted to zero.

mod artifact;
mod byte_causes;
mod census;
mod census_store;
mod compare_functions;
mod evm_dataflow;
mod fe;
mod fe_events;
mod fe_stages;
mod fe_trace;
mod manifest;
mod model;
mod regions;
mod report;
mod selection;
mod solc_functions;
mod sonatina_functions;
mod validate;

pub use artifact::decode_artifact;
pub use byte_causes::{
    BYTE_CAUSES_SCHEMA, ByteCauses, CauseDelta, CauseTally, apportion_excess, classify_bytes,
    compare_causes,
};
pub use census::{
    ArtifactCensus, CensusCaptureContext, CensusRegion, EvmRunOptions, EvmRunPorts, EvmRunSummary,
    PatternGroup, RegionManifest, RegionSpec, census_capture, census_file, census_regions,
    census_wgsl, evm_run_summary, render_census,
};
pub use census_store::{
    CensusComparison, CensusFile, CensusSource, compare_censuses, render_census_comparison,
    replay_census, save_census,
};
pub use compare_functions::{
    FunctionComparison, FunctionPair, compare_functions, render_function_comparison, residual_row,
};
pub use evm_dataflow::{
    BlockClass, BlockFacetCensus, BlockRecord, CrossFacet, DataflowBlocks, DataflowReport,
    EVM_DATAFLOW_BLOCKS_SCHEMA, EVM_DATAFLOW_REPORT_SCHEMA, FunctionScheduling,
    MEMORY_OFFSETS_BLIND_VIEW, PORT_ORDER_BLIND_VIEW, SchedulingBytes, compare_blocks,
    dataflow_report, evm_dataflow_blocks, render_dataflow_report,
};
pub use fe::{FeImport, import_fe_trace};
pub use fe_events::{FeEventsImport, import_fe_events};
pub use fe_stages::{
    BodyExpansion, ChainClass, ConstructExpansion, FE_STAGES_SCHEMA, FeStagesReport,
    MEMORY_OPCODES, STAGE_CHAIN_LEVEL, StageInputs, StageReport, memory_bucket, render_fe_stages,
};
pub use fe_trace::{
    BodyInfo, FE_TRACE_REGIONS_ADAPTER, FeTraceBytesReport, Row, RunInfo, RunOccurrenceInfo,
    body_name, emitted_function_manifest, fe_trace_bytes, render_fe_trace_bytes,
};
pub use manifest::{artifact_digest, capture_id, load_capture, save_capture, verify_artifacts};
pub use model::*;
pub use regions::{FunctionRegion, FunctionRegions, OUTSIDE_FUNCTIONS};
pub use report::{CompareReport, Report, compare, render_compare_table, render_table, report};
pub use selection::{
    Selection, opcode_selection, pattern_matches, pattern_selection, range_selection,
};
pub use solc_functions::{SOLC_REGIONS_ADAPTER, SolcFunctions, solc_functions};
pub use sonatina_functions::{
    FunctionClass, FunctionFacetCensus, function_graphs, sonatina_function_facets,
};
pub use validate::{reachable_union, validate};
