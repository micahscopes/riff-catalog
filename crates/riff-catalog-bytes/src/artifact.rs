//! Reading artifact files.

use anyhow::{Context, Result};

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
