//! Reading artifact files and the reports other commands wrote.

use std::path::Path;

use anyhow::{Context, Result, ensure};
use riff_catalog_bloat::{REGIONS_SCHEMA_V1, REGIONS_SCHEMA_V2, RegionManifest};
use serde::de::DeserializeOwned;

/// Read a file, naming it in the error.
pub fn read_file(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("read {}", path.display()))
}

/// Read a JSON file, naming it in the error.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(&read_file(path)?).with_context(|| format!("parse {}", path.display()))
}

/// Read an artifact file ([`decode_artifact`]), naming it in errors.
pub fn load_artifact(path: &Path) -> Result<Vec<u8>> {
    decode_artifact(&read_file(path)?).with_context(|| format!("decode {}", path.display()))
}

/// Read a region manifest and check that it describes `artifact`: a
/// supported schema and the artifact's blake3.
pub fn load_regions(path: &Path, artifact: &[u8]) -> Result<RegionManifest> {
    let manifest = load_regions_unbound(path)?;
    let actual = blake3::hash(artifact).to_hex().to_string();
    ensure!(
        manifest.artifact_blake3 == actual,
        "{} is a region manifest for the artifact with blake3 {}, not for this one (blake3 {actual})",
        path.display(),
        manifest.artifact_blake3
    );
    Ok(manifest)
}

/// Read a region manifest when no artifact is at hand (only the schema is
/// checked).
pub fn load_regions_unbound(path: &Path) -> Result<RegionManifest> {
    let manifest: RegionManifest = read_json(path)?;
    ensure!(
        manifest.schema == REGIONS_SCHEMA_V1 || manifest.schema == REGIONS_SCHEMA_V2,
        "{} is `{}`, expected `{REGIONS_SCHEMA_V1}` or `{REGIONS_SCHEMA_V2}`",
        path.display(),
        manifest.schema
    );
    Ok(manifest)
}

/// Where the instructions of `artifact` end. A requested end may not pass
/// the artifact. Without one: the start of the manifest's `data` region that
/// runs to the end of the artifact, else the start of a Solidity CBOR
/// metadata trailer, else the whole artifact.
pub fn code_end(
    artifact: &[u8],
    requested: Option<usize>,
    manifest: Option<&RegionManifest>,
) -> Result<usize> {
    if let Some(end) = requested {
        ensure!(
            end <= artifact.len(),
            "--code-end {end} is past the end of the {}-byte artifact",
            artifact.len()
        );
        return Ok(end);
    }
    let data = manifest.and_then(|m| {
        m.regions
            .iter()
            .filter(|r| r.kind == "data" && r.end == artifact.len())
            .map(|r| r.start)
            .min()
    });
    Ok(data.unwrap_or_else(|| riff_catalog_evm::split_metadata(artifact).0))
}

/// Read Fe attribution details for `contract` and check them against the
/// instructions `code` decodes to: the rows must be exactly those
/// instructions, from pc 0 to the end of `code`. Without a trace there are
/// no opcodes to compare; boundaries are what the details can show.
pub fn load_details(
    path: &Path,
    contract: &str,
    code: &[u8],
) -> Result<Vec<riff_catalog_ingest_trace::bytes::DetailsRow>> {
    let text = std::str::from_utf8(&read_file(path)?)
        .with_context(|| format!("{} is not UTF-8", path.display()))?
        .to_string();
    let rows = riff_catalog_ingest_trace::bytes::read_runtime_details(&text, contract)
        .with_context(|| format!("read {}", path.display()))?;
    let insts = riff_catalog_evm::decode::decode(code);
    ensure!(
        rows.len() == insts.len(),
        "{} has {} rows for contract `{contract}`, but the code decodes to {} instructions ({} bytes)",
        path.display(),
        rows.len(),
        insts.len(),
        code.len()
    );
    for (row, inst) in rows.iter().zip(&insts) {
        ensure!(
            row.pc_start == inst.pc && row.pc_end == inst.pc + inst.len,
            "{}: row {}..{} is not an instruction of this artifact (the instruction at pc {} is {}..{})",
            path.display(),
            row.pc_start,
            row.pc_end,
            inst.pc,
            inst.pc,
            inst.pc + inst.len
        );
    }
    Ok(rows)
}

/// Refuse a report whose `schema` is not the one this reader understands.
pub fn check_schema(found: &str, expected: &str, path: &Path) -> Result<()> {
    ensure!(
        found == expected,
        "{} is `{found}`, expected `{expected}`",
        path.display()
    );
    Ok(())
}

/// Decode an artifact file: raw bytes, or Fe's hex text form (optional `0x`,
/// surrounding whitespace allowed).
pub fn decode_artifact(raw: &[u8]) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(raw).ok().map(str::trim);
    if let Some(text) = text {
        let hex = text.strip_prefix("0x").unwrap_or(text);
        if !hex.is_empty() && hex.len() % 2 == 0 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).context("hex artifact"))
                .collect();
        }
    }
    Ok(raw.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifacts_decode_from_hex_or_raw() {
        assert_eq!(decode_artifact(b"0x6080\n").unwrap(), vec![0x60, 0x80]);
        assert_eq!(decode_artifact(b"6080").unwrap(), vec![0x60, 0x80]);
        assert_eq!(decode_artifact(&[0x60, 0x80]).unwrap(), vec![0x60, 0x80]);
    }
}
