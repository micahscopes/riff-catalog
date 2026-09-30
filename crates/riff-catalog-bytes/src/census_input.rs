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

/// The `evm_run` patterns of a census, in the census's order. A pattern
/// naming a region the census does not list is an error.
pub fn census_run_classes(census: &ArtifactCensus) -> Result<Vec<CensusRunClass>> {
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
        .map(|p| {
            Ok(CensusRunClass {
                digest: p.digest.clone(),
                covered_bytes: p.covered_bytes as u64,
                ranges: p
                    .regions
                    .iter()
                    .map(|id| {
                        by_id.get(id.as_str()).copied().with_context(|| {
                            format!("run class {} names unknown region `{id}`", p.digest)
                        })
                    })
                    .collect::<Result<_>>()?,
            })
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
    let classes = census_run_classes(&census).with_context(|| name.to_string())?;
    check_runs(&classes, artifact).with_context(|| name.to_string())?;
    Ok(classes)
}

/// Every run copy must be whole instructions of `artifact`, and a class's
/// covered bytes must be the sum of its copies.
fn check_runs(classes: &[CensusRunClass], artifact: &[u8]) -> Result<()> {
    let insts = riff_catalog_evm::decode::decode(artifact);
    let starts: std::collections::BTreeSet<u32> = insts.iter().map(|i| i.pc).collect();
    let ends: std::collections::BTreeSet<u32> = insts.iter().map(|i| i.pc + i.len).collect();
    for class in classes {
        let mut covered = 0u64;
        for &(start, end) in &class.ranges {
            ensure!(
                start < end && end as usize <= artifact.len(),
                "run class {} has a copy at {start}..{end}, not inside the {}-byte artifact",
                class.digest,
                artifact.len()
            );
            ensure!(
                starts.contains(&start) && ends.contains(&end),
                "run class {} has a copy at {start}..{end}, which is not whole instructions",
                class.digest
            );
            covered += u64::from(end - start);
        }
        ensure!(
            covered == class.covered_bytes,
            "run class {} says it covers {} bytes, but its copies cover {covered}",
            class.digest,
            class.covered_bytes
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_naming_an_unknown_region_is_an_error() {
        // Two copies of a 12-byte body in two functions: one run class.
        let body = [
            0x60u8, 0x01, 0x80, 0x01, 0x90, 0x50, 0x60, 0x07, 0x02, 0x80, 0x01, 0x50,
        ];
        let mut code = body.to_vec();
        code.push(0x00);
        code.extend(body);
        code.push(0x00);
        let manifest: riff_catalog_bloat::RegionManifest =
            serde_json::from_value(serde_json::json!({
                "schema": "riffcat-regions/2",
                "artifact_blake3": blake3::hash(&code).to_hex().to_string(),
                "adapter": "t/1",
                "regions": [
                    {"id": "f", "kind": "function", "name": "f", "start": 0, "end": 13},
                    {"id": "g", "kind": "function", "name": "g", "start": 13, "end": 26}
                ],
                "evm_runs": {"min_run_bytes": 8}
            }))
            .unwrap();
        let mut census = riff_catalog_bloat::census_regions(&code, manifest).unwrap();
        let json = serde_json::to_vec(&census).unwrap();
        assert_eq!(parse_census_runs(&json, &code, "c").unwrap().len(), 1);
        let run = census
            .patterns
            .iter_mut()
            .find(|p| p.region_kind == "evm_run")
            .unwrap();
        run.regions[0] = "evm-run:999".into();
        let json = serde_json::to_vec(&census).unwrap();
        let err = format!("{:#}", parse_census_runs(&json, &code, "c").unwrap_err());
        assert!(err.contains("evm-run:999"), "{err}");
    }

    #[test]
    fn run_copies_must_be_instructions_of_this_artifact() {
        let body = [
            0x60u8, 0x01, 0x80, 0x01, 0x90, 0x50, 0x60, 0x07, 0x02, 0x80, 0x01, 0x50,
        ];
        let mut code = body.to_vec();
        code.push(0x00);
        code.extend(body);
        code.push(0x00);
        let manifest: riff_catalog_bloat::RegionManifest =
            serde_json::from_value(serde_json::json!({
                "schema": "riffcat-regions/2",
                "artifact_blake3": blake3::hash(&code).to_hex().to_string(),
                "adapter": "t/1",
                "regions": [
                    {"id": "f", "kind": "function", "name": "f", "start": 0, "end": 13},
                    {"id": "g", "kind": "function", "name": "g", "start": 13, "end": 26}
                ],
                "evm_runs": {"min_run_bytes": 8}
            }))
            .unwrap();
        let census = riff_catalog_bloat::census_regions(&code, manifest).unwrap();
        let edited = |edit: &dyn Fn(&mut riff_catalog_bloat::ArtifactCensus)| {
            let mut c = census.clone();
            edit(&mut c);
            parse_census_runs(&serde_json::to_vec(&c).unwrap(), &code, "c")
        };
        edited(&|_| {}).unwrap();
        let run = |c: &mut riff_catalog_bloat::ArtifactCensus| -> usize {
            c.regions
                .iter()
                .position(|r| r.region.kind == "evm_run")
                .unwrap()
        };
        let cases: Vec<(&str, Box<dyn Fn(&mut riff_catalog_bloat::ArtifactCensus)>)> = vec![
            (
                "past",
                Box::new(|c| {
                    let i = run(c);
                    c.regions[i].region.end = 99;
                }),
            ),
            (
                "reversed",
                Box::new(|c| {
                    let i = run(c);
                    let r = &mut c.regions[i].region;
                    std::mem::swap(&mut r.start, &mut r.end);
                }),
            ),
            (
                "off an instruction",
                Box::new(|c| {
                    let i = run(c);
                    c.regions[i].region.start += 1;
                    c.regions[i].region.end += 1;
                }),
            ),
            (
                "covered bytes",
                Box::new(|c| {
                    let p = c
                        .patterns
                        .iter_mut()
                        .find(|p| p.region_kind == "evm_run")
                        .unwrap();
                    p.covered_bytes += 1;
                }),
            ),
        ];
        for (name, edit) in cases {
            assert!(edited(&*edit).is_err(), "{name} accepted");
        }
    }
}
