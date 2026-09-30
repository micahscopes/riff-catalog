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

/// Split a `name=value` command-line argument.
pub fn named<'a>(spec: &'a str, what: &str) -> Result<(&'a str, &'a str)> {
    spec.split_once('=')
        .with_context(|| format!("{what}: expected name=value, got `{spec}`"))
}

/// A pc set file: a JSON array of pcs, or an object whose `entries` are
/// objects with a `pc` (for example Sonatina memory-plan tags). Every entry
/// must be a pc that fits in 32 bits.
pub fn read_pc_set(name: &str, json: &[u8]) -> Result<Selection> {
    let value: serde_json::Value = serde_json::from_slice(json)?;
    let items = value
        .as_array()
        .or_else(|| value["entries"].as_array())
        .context("expected an array of pcs or an object with entries")?;
    let mut pcs = BTreeSet::new();
    for (k, e) in items.iter().enumerate() {
        let pc = e
            .as_u64()
            .or_else(|| e["pc"].as_u64())
            .with_context(|| format!("entry {k} ({e}) is not a pc"))?;
        let pc = u32::try_from(pc).with_context(|| format!("entry {k}: pc {pc} is too large"))?;
        pcs.insert(pc);
    }
    Ok(Selection {
        name: name.to_string(),
        pcs,
    })
}

/// Refuse a selection holding a pc that is not the start of an instruction
/// of `code` (inside a PUSH immediate, or past the end): it was made for
/// other code.
pub fn check_instruction_starts(selection: &Selection, code: &[u8]) -> Result<()> {
    let starts: BTreeSet<u32> = decode(code).iter().map(|i| i.pc).collect();
    if let Some(pc) = selection.pcs.iter().find(|pc| !starts.contains(pc)) {
        anyhow::bail!(
            "`{}` holds pc {pc}, which is not the start of an instruction of the {}-byte code",
            selection.name,
            code.len()
        );
    }
    Ok(())
}

/// Split `a|b` needles, refusing an empty one (it would match every name).
pub fn needles(arg: &str) -> Result<Vec<&str>> {
    let needles: Vec<&str> = arg.split('|').collect();
    anyhow::ensure!(
        needles.iter().all(|n| !n.is_empty()),
        "`{arg}` has an empty name, which would match everything"
    );
    Ok(needles)
}

/// pcs of the full PUSH1..PUSH4 instructions whose value is `target` (for a
/// function entry: its call sites).
pub fn push_selection(name: &str, code: &[u8], target: u32) -> Selection {
    Selection {
        name: name.into(),
        pcs: decode(code)
            .iter()
            .filter(|i| riff_catalog_evm::decode::push_value(code, i) == Some(target))
            .map(|i| i.pc)
            .collect(),
    }
}

/// Non-overlapping byte ranges matching `pattern` (hex, `??` any byte),
/// scanning left to right.
pub fn pattern_matches(code: &[u8], pattern: &str) -> Result<Vec<(u32, u32)>> {
    let hex: String = pattern.chars().filter(|c| !c.is_whitespace()).collect();
    anyhow::ensure!(
        hex.len().is_multiple_of(2) && !hex.is_empty(),
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
