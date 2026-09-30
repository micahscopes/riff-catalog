//! Reading artifact files and the reports other commands wrote.

use std::path::Path;

use anyhow::{Context, Result, ensure};
use serde::de::DeserializeOwned;

/// Read a file, naming it in the error.
pub fn read_file(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("read {}", path.display()))
}

/// Read a JSON file, naming it in the error.
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(&read_file(path)?).with_context(|| format!("parse {}", path.display()))
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
