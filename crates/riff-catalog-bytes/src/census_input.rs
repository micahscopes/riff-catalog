//! EVM run classes read back from a census, for commands that use them as
//! instruction selections.

use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use riff_catalog_bloat::{ArtifactCensus, CENSUS_SCHEMA_V1, CENSUS_SCHEMA_V2, CensusFile};

/// One `evm_run` pattern of a census: its digest, covered bytes and the byte
/// ranges of its occurrences, in the pattern's order.
#[derive(Clone, Debug)]
pub struct CensusRunClass {
    pub digest: String,
    pub covered_bytes: u64,
    pub ranges: Vec<(u32, u32)>,
}

/// The `evm_run` patterns of a census, in the census's order.
pub fn census_run_classes(census: &ArtifactCensus) -> Vec<CensusRunClass> {
    let by_id: BTreeMap<&str, (u32, u32)> = census
        .regions
        .iter()
        .map(|r| {
            (
                r.region.id.as_str(),
                (r.region.start as u32, r.region.end as u32),
            )
        })
        .collect();
    census
        .patterns
        .iter()
        .filter(|p| p.region_kind == "evm_run")
        .map(|p| CensusRunClass {
            digest: p.digest.clone(),
            covered_bytes: p.covered_bytes as u64,
            ranges: p
                .regions
                .iter()
                .filter_map(|id| by_id.get(id.as_str()).copied())
                .collect(),
        })
        .collect()
}

/// Read the EVM run classes of a census of `artifact`: a census as `census
/// --json` prints it, or a census file as `census --output` saves it. The
/// census must be of this artifact (its blake3) and made from a manifest
/// that asked for EVM runs (schema version 2); it may have found none.
/// `name` names the input in errors.
pub fn parse_census_runs(json: &[u8], artifact: &[u8], name: &str) -> Result<Vec<CensusRunClass>> {
    let value: serde_json::Value =
        serde_json::from_slice(json).with_context(|| format!("parse {name}"))?;
    let census: ArtifactCensus = if value["schema"] == "riffcat-census-file/1" {
        serde_json::from_value::<CensusFile>(value)
            .with_context(|| format!("{name}: not a census file"))?
            .census
    } else {
        serde_json::from_value(value).with_context(|| {
            format!("{name}: expected a census (`census --json` or `census --output`)")
        })?
    };
    ensure!(
        census.schema != CENSUS_SCHEMA_V1,
        "{name} has no EVM runs: its region manifest did not ask for them (evm_runs)"
    );
    ensure!(
        census.schema == CENSUS_SCHEMA_V2,
        "{name} is `{}`, expected `{CENSUS_SCHEMA_V2}`",
        census.schema
    );
    let actual = blake3::hash(artifact).to_hex().to_string();
    ensure!(
        census.artifact_blake3 == actual,
        "{name} is a census of the artifact with blake3 {}, not of this one (blake3 {actual})",
        census.artifact_blake3
    );
    Ok(census_run_classes(&census))
}
