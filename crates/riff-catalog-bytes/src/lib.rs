//! Where the bytes of an EVM runtime come from: analyses that join an
//! artifact to its compiler's own records (Fe trace bundles and attribution
//! details, solc source maps, Sonatina IR) and to its lifted dataflow, and
//! the excess ledger that compares two builds cause by cause.
//!
//! Every analysis works on the artifact bytes and a `riffcat-regions`
//! manifest from `riff-catalog-bloat`, whose census it reuses for repeated
//! EVM runs. The `riffcat-bytes` binary exposes each analysis as a command.

mod artifact;
mod byte_causes;
mod census_input;
mod compare_functions;
mod evm_dataflow;
mod fe_stages;
mod fe_trace;
mod regions;
mod selection;
mod solc_functions;
mod sonatina_functions;

pub use artifact::{
    check_schema, code_end, decode_artifact, load_artifact, load_regions, load_regions_unbound,
    read_file, read_json,
};
pub use byte_causes::{
    BYTE_CAUSES_COMPARE_SCHEMA, BYTE_CAUSES_SCHEMA, ByteCauses, CauseComparison, CauseDelta,
    CauseInputs, CauseTally, apportion_excess, byte_cause_ledger, cause_selection, classify_bytes,
    compare_causes,
};
pub use census_input::{CensusRunClass, census_run_classes, parse_census_runs};
pub use compare_functions::{
    FUNCTION_COMPARISON_SCHEMA, FunctionComparison, FunctionComparisonReport, FunctionPair,
    compare_functions, render_function_comparison, residual_row,
};
pub use evm_dataflow::{
    BlockClass, BlockFacetCensus, BlockRecord, CrossFacet, DataflowBlocks, DataflowComparison,
    DataflowReport, EVM_DATAFLOW_BLOCKS_SCHEMA, EVM_DATAFLOW_COMPARE_SCHEMA,
    EVM_DATAFLOW_REPORT_SCHEMA, FunctionScheduling, INPUT_ORDER_BLIND_VIEW,
    MEMORY_OFFSETS_AND_INPUT_ORDER_BLIND_VIEW, MEMORY_OFFSETS_BLIND_VIEW, SchedulingBytes,
    compare_blocks, dataflow_report, evm_dataflow_blocks, render_dataflow_report,
};
pub use fe_stages::{
    BodyExpansion, ChainClass, ConstructExpansion, FE_STAGES_SCHEMA, FeStagesReport,
    STAGE_CHAIN_LEVEL, StageInputs, StageReport, StageRequest, memory_bucket, render_fe_stages,
};
pub use fe_trace::{
    ArmInfo, ArmReach, BodyInfo, FE_TRACE_REGIONS_ADAPTER, FeTraceBytesReport, Row, RunInfo,
    RunOccurrenceInfo, body_name, emitted_function_manifest, fe_trace_bytes, read_checked_ledger,
    render_fe_trace_bytes,
};
pub use regions::{FunctionRegion, FunctionRegions, OUTSIDE_FUNCTIONS};
pub use selection::{
    Selection, named, opcode_selection, pattern_matches, pattern_selection, push_selection,
    range_selection, read_pc_set,
};
pub use solc_functions::{
    SOLC_FUNCTIONS_SCHEMA, SOLC_REGIONS_ADAPTER, SolcFunctions, solc_functions,
};
pub use sonatina_functions::{
    FunctionClass, FunctionFacetCensus, SONATINA_FUNCTIONS_SCHEMA, SonatinaFunctions,
    sonatina_function_facets,
};
