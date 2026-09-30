//! EVM run classes read back from a census report, for commands that use
//! them as instruction selections.

/// One `evm_run` pattern of a census: its digest, covered bytes and the byte
/// ranges of its occurrences, in the pattern's order.
#[derive(Clone, Debug)]
pub struct CensusRunClass {
    pub digest: String,
    pub covered_bytes: Option<u64>,
    pub ranges: Vec<(u32, u32)>,
}

/// The `evm_run` patterns of a census JSON value, in the census's order.
pub fn census_run_classes(value: &serde_json::Value) -> Vec<CensusRunClass> {
    let by_id: std::collections::BTreeMap<String, (u32, u32)> = value["regions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let r = &r["region"];
            Some((
                r["id"].as_str()?.to_string(),
                (r["start"].as_u64()? as u32, r["end"].as_u64()? as u32),
            ))
        })
        .collect();
    value["patterns"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["region_kind"] == "evm_run")
        .map(|p| CensusRunClass {
            digest: p["digest"].as_str().unwrap_or("").to_string(),
            covered_bytes: p["covered_bytes"].as_u64(),
            ranges: p["regions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|id| by_id.get(id.as_str()?).copied())
                .collect(),
        })
        .collect()
}
