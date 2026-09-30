//! Named sets of emitted instructions (pc starts), and the ways to build
//! them from EVM code: by opcode, by byte pattern, or by byte range.

use std::collections::BTreeSet;

use anyhow::{Context, Result};
use riff_catalog_evm::decode::decode;

/// A named set of emitted instructions (pc starts).
#[derive(Clone, Debug)]
pub struct Selection {
    pub name: String,
    pub pcs: BTreeSet<u32>,
}

/// Non-overlapping byte ranges matching `pattern` (hex, `??` any byte),
/// scanning left to right.
pub fn pattern_matches(code: &[u8], pattern: &str) -> Result<Vec<(u32, u32)>> {
    let hex: String = pattern.chars().filter(|c| !c.is_whitespace()).collect();
    anyhow::ensure!(
        hex.len() % 2 == 0 && !hex.is_empty(),
        "pattern must be whole bytes"
    );
    let bytes: Vec<Option<u8>> = (0..hex.len())
        .step_by(2)
        .map(|i| match &hex[i..i + 2] {
            "??" => Ok(None),
            h => u8::from_str_radix(h, 16).map(Some),
        })
        .collect::<Result<_, _>>()
        .context("pattern hex")?;
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + bytes.len() <= code.len() {
        let hit = bytes
            .iter()
            .enumerate()
            .all(|(k, b)| b.is_none_or(|b| code[at + k] == b));
        if hit {
            out.push((at as u32, (at + bytes.len()) as u32));
            at += bytes.len();
        } else {
            at += 1;
        }
    }
    Ok(out)
}

/// pcs of instructions that start inside a match of `pattern`.
pub fn pattern_selection(name: &str, code: &[u8], pattern: &str) -> Result<Selection> {
    Ok(range_selection(
        name,
        code,
        &pattern_matches(code, pattern)?,
    ))
}

/// pcs of every instruction whose opcode is in `opcodes`.
pub fn opcode_selection(name: &str, code: &[u8], opcodes: &[u8]) -> Selection {
    Selection {
        name: name.into(),
        pcs: decode(code)
            .into_iter()
            .filter(|i| opcodes.contains(&i.opcode))
            .map(|i| i.pc)
            .collect(),
    }
}

/// pcs of every instruction inside the given byte ranges.
pub fn range_selection(name: &str, code: &[u8], ranges: &[(u32, u32)]) -> Selection {
    Selection {
        name: name.into(),
        pcs: decode(code)
            .into_iter()
            .filter(|i| ranges.iter().any(|(s, e)| *s <= i.pc && i.pc < *e))
            .map(|i| i.pc)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_selection_takes_every_instruction_in_a_match() {
        // PUSH1 0x40 MLOAD | PUSH1 0x40 MLOAD with a wildcard on the value.
        let code = [0x60, 0x40, 0x51, 0x00, 0x60, 0x41, 0x51];
        let s = pattern_selection("p", &code, "60 ?? 51").unwrap();
        assert_eq!(s.pcs.into_iter().collect::<Vec<_>>(), vec![0, 2, 4, 6]);
        let m = opcode_selection("m", &code, &riff_catalog_evm::decode::MEMORY_OPCODES);
        assert_eq!(m.pcs.len(), 2);
    }
}
