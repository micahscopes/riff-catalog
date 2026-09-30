//! Reading artifact files and the reports other commands wrote.

use std::path::Path;

use anyhow::{Context, Result, ensure};
use riff_catalog_bloat::RegionManifest;
use serde::de::DeserializeOwned;

/// Read a file, naming it in the error.
pub fn read_file(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("read {}", path.display()))
}

/// Read a JSON file, naming it in the error.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(&read_file(path)?).with_context(|| format!("parse {}", path.display()))
}

/// Read an artifact file ([`decode_artifact_as`]), naming it in errors.
pub fn load_artifact(path: &Path, format: ArtifactFormat) -> Result<Vec<u8>> {
    decode_artifact_as(&read_file(path)?, format)
        .with_context(|| format!("decode {}", path.display()))
}

/// Read a region manifest and check that it describes `artifact`: a
/// supported schema and the artifact's blake3.
pub fn load_regions(path: &Path, artifact: &[u8]) -> Result<RegionManifest> {
    let manifest: RegionManifest = read_json(path)?;
    let actual = blake3::hash(artifact).to_hex().to_string();
    ensure!(
        manifest.artifact_blake3 == actual,
        "{} is a region manifest for the artifact with blake3 {}, not for this one (blake3 {actual})",
        path.display(),
        manifest.artifact_blake3
    );
    manifest
        .validate(Some(artifact))
        .with_context(|| format!("region manifest {}", path.display()))?;
    Ok(manifest)
}

/// Read a region manifest when no artifact is at hand: everything but the
/// artifact's hash and length is checked ([`RegionManifest::validate`]).
pub fn load_regions_unbound(path: &Path) -> Result<RegionManifest> {
    let manifest: RegionManifest = read_json(path)?;
    manifest
        .validate(None)
        .with_context(|| format!("region manifest {}", path.display()))?;
    Ok(manifest)
}

/// Where the instructions of `artifact` end. A requested end may not pass
/// the artifact or fall inside an instruction. Without one: the start of the manifest's `data` regions
/// that run, one after another, to the end of the artifact, else the start
/// of a well-formed Solidity CBOR metadata trailer, else the whole artifact.
pub fn code_end(
    artifact: &[u8],
    requested: Option<usize>,
    manifest: Option<&RegionManifest>,
) -> Result<usize> {
    // Whether `end` falls between two whole instructions.
    let boundary = |end: usize| {
        riff_catalog_evm::decode::decode(&artifact[..end])
            .last()
            .is_none_or(|i| i.len as usize == 1 + riff_catalog_evm::decode::push_len(i.opcode))
    };
    if let Some(end) = requested {
        ensure!(
            end <= artifact.len(),
            "--code-end {end} is past the end of the {}-byte artifact",
            artifact.len()
        );
        ensure!(
            boundary(end),
            "--code-end {end} is inside an instruction (a PUSH's immediate runs past it)"
        );
        return Ok(end);
    }
    // The start of the data regions that run, one after another, to the end.
    let data = manifest.and_then(|m| {
        let mut start = None;
        let mut end = artifact.len();
        while let Some(r) = m
            .regions
            .iter()
            .filter(|r| r.kind == "data" && r.end == end && r.start < end)
            .min_by_key(|r| r.start)
        {
            start = Some(r.start);
            end = r.start;
        }
        start
    });
    if let Some(start) = data {
        ensure!(
            boundary(start),
            "the manifest's data starts at {start}, inside an instruction"
        );
        return Ok(start);
    }
    Ok(riff_catalog_evm::solc_metadata_start(artifact)
        .filter(|start| boundary(*start))
        .unwrap_or(artifact.len()))
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

/// How an artifact file is written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum ArtifactFormat {
    /// Hex text when the file is only hex digits and whitespace and says
    /// so with a `0x` prefix or a line break; raw bytes when it is not only
    /// hex digits and whitespace. A file of bare hex digits could be either
    /// and is refused: pass `hex` or `raw`.
    #[default]
    Auto,
    /// Hex text (optional `0x`, whitespace and line breaks ignored).
    Hex,
    /// Raw bytes.
    Raw,
}

/// Decode an artifact file ([`ArtifactFormat::Auto`]).
pub fn decode_artifact(raw: &[u8]) -> Result<Vec<u8>> {
    decode_artifact_as(raw, ArtifactFormat::Auto)
}

/// Decode an artifact file written in `format`. Hex text must have an even
/// number of digits; a file that is only hex digits is read as hex unless
/// `format` is [`ArtifactFormat::Raw`].
pub fn decode_artifact_as(raw: &[u8], format: ArtifactFormat) -> Result<Vec<u8>> {
    let hex_text = || -> Option<String> {
        let text = std::str::from_utf8(raw).ok()?.trim_start();
        let text = text.strip_prefix("0x").unwrap_or(text);
        let digits: String = text.chars().filter(|c| !c.is_ascii_whitespace()).collect();
        (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_hexdigit())).then_some(digits)
    };
    let digits = match format {
        ArtifactFormat::Raw => return Ok(raw.to_vec()),
        ArtifactFormat::Auto => match hex_text() {
            Some(d) => {
                let marked = raw.starts_with(b"0x") || raw.iter().any(|b| b.is_ascii_whitespace());
                ensure!(
                    marked,
                    "the file is only hex digits, which could be hex text or raw bytes; pass --artifact-format hex or raw"
                );
                d
            }
            None => return Ok(raw.to_vec()),
        },
        ArtifactFormat::Hex => hex_text().context("not hex text")?,
    };
    ensure!(
        digits.len() % 2 == 0,
        "hex text has an odd number of digits ({}); pass --artifact-format raw if the file is raw bytes",
        digits.len()
    );
    (0..digits.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).context("hex artifact"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_code_may_not_end_inside_an_instruction() {
        // PUSH2 0x0000, STOP
        let code = [0x61, 0x00, 0x00, 0x00];
        assert!(code_end(&code, Some(2), None).is_err());
        assert!(code_end(&code, Some(1), None).is_err());
        assert_eq!(code_end(&code, Some(3), None).unwrap(), 3);
        assert_eq!(code_end(&code, Some(0), None).unwrap(), 0);
    }

    #[test]
    fn only_a_whole_cbor_trailer_ends_the_code() {
        // A map header 256 bytes before the end and a length that points at
        // it, but the bytes between are not CBOR: all code.
        let mut code = vec![0x5b; 300];
        code[44] = 0xa2;
        code[298] = 0x00;
        code[299] = 0xfe;
        assert_eq!(code_end(&code, None, None).unwrap(), 300);
        // A real trailer: {"solc": h'000821'} and its length.
        let mut code = vec![0x5b; 10];
        let cbor = [0xa1, 0x64, b's', b'o', b'l', b'c', 0x43, 0x00, 0x08, 0x21];
        code.extend(cbor);
        code.extend([0x00, cbor.len() as u8]);
        assert_eq!(code_end(&code, None, None).unwrap(), 10);
    }

    #[test]
    fn artifacts_decode_from_hex_or_raw() {
        let auto = |raw: &[u8]| decode_artifact_as(raw, ArtifactFormat::Auto);
        assert_eq!(auto(b"0x6080\n").unwrap(), vec![0x60, 0x80]);
        assert_eq!(auto(b"6080\n").unwrap(), vec![0x60, 0x80]);
        // Bare hex digits could be either: refused, with the way out named.
        let err = auto(b"6080").unwrap_err().to_string();
        assert!(err.contains("--artifact-format"), "{err}");
        assert_eq!(auto(&[0x60, 0x80]).unwrap(), vec![0x60, 0x80]);
        // Hex wrapped over lines is still hex.
        assert_eq!(
            auto(b"0x6080\n6040\n").unwrap(),
            vec![0x60, 0x80, 0x60, 0x40]
        );
        // An odd number of hex digits is an error, not raw bytes.
        assert!(auto(b"608060405").is_err());
        // Raw bytes that happen to be hex digits: not guessed.
        let raw = [0x36u8, 0x30, 0x36, 0x30];
        assert!(auto(&raw).is_err());
        assert_eq!(
            decode_artifact_as(&raw, ArtifactFormat::Hex).unwrap(),
            vec![0x60, 0x60]
        );
        assert_eq!(
            decode_artifact_as(&raw, ArtifactFormat::Raw).unwrap(),
            raw.to_vec()
        );
        assert!(decode_artifact_as(&[0x60, 0x80], ArtifactFormat::Hex).is_err());
    }
}
