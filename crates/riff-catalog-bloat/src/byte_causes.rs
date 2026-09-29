//! An excess ledger: every byte of an EVM runtime in exactly one cause
//! bucket, so two builds of the same contract can be compared cause by
//! cause and the differences add up to the size difference.
//!
//! Named causes are pc sets tried in the given order (the first that holds
//! an instruction wins): spill traffic, the free-pointer clamp, duplicated
//! functions, repeated code beyond its first copy, ABI code, and so on. An
//! instruction no named cause claims falls into a role bucket by opcode.
//! Role buckets describe what the leftover bytes do; they are not causes.

use std::collections::{BTreeMap, BTreeSet};

use riff_catalog_evm::runs::decode;
use serde::{Deserialize, Serialize};

use crate::fe_stages::Selection;

pub const BYTE_CAUSES_SCHEMA: &str = "riffcat-evm-byte-causes/1";

pub const ROLE_STACK: &str = "role: stack shuffle (DUP, SWAP, POP)";
pub const ROLE_CONTROL: &str = "role: control flow (JUMP, JUMPI, JUMPDEST, label pushes)";
pub const ROLE_MEMORY: &str = "role: memory access (MLOAD, MSTORE, MSTORE8, MCOPY, MSIZE)";
pub const ROLE_MEMORY_ADDRESS: &str = "role: memory address constants";
pub const ROLE_CONSTANT: &str = "role: other constant pushes";
pub const ROLE_ARITHMETIC: &str = "role: arithmetic, comparison, bitwise";
pub const ROLE_EFFECT: &str = "role: calls, environment, storage, logs, hashing, halts";
pub const ROLE_DATA: &str = "data after the code (metadata, constants)";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CauseTally {
    pub bytes: u64,
    pub instructions: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ByteCauses {
    pub schema: String,
    pub artifact_bytes: u64,
    pub code_end: u64,
    /// Named causes in priority order, then role buckets.
    pub order: Vec<String>,
    pub buckets: BTreeMap<String, CauseTally>,
    /// Per function region: bucket to bytes.
    pub by_region: Vec<(String, u64, BTreeMap<String, u64>)>,
    /// Bucket to detail label to bytes, when detail sets were given (the
    /// first detail set holding an instruction labels it, else `(none)`).
    #[serde(default)]
    pub detail: BTreeMap<String, BTreeMap<String, u64>>,
}

fn role(
    opcode: u8,
    pc: u32,
    labels: &BTreeSet<u32>,
    memory_address: &BTreeSet<u32>,
) -> &'static str {
    match opcode {
        0x50 | 0x80..=0x9f => ROLE_STACK,
        0x56 | 0x57 | 0x5b => ROLE_CONTROL,
        0x51 | 0x52 | 0x53 | 0x59 | 0x5e => ROLE_MEMORY,
        0x5f..=0x7f if labels.contains(&pc) => ROLE_CONTROL,
        0x5f..=0x7f if memory_address.contains(&pc) => ROLE_MEMORY_ADDRESS,
        0x5f..=0x7f => ROLE_CONSTANT,
        0x01..=0x0b | 0x10..=0x1d => ROLE_ARITHMETIC,
        _ => ROLE_EFFECT,
    }
}

/// Classify every byte of `code` (instructions end at `code_end`). `labels`
/// are PUSH pcs whose value is a jump label; `memory_address` PUSH pcs
/// whose value is a memory or calldata address; `functions` name regions.
pub fn classify_bytes(
    code: &[u8],
    code_end: usize,
    causes: &[Selection],
    labels: &BTreeSet<u32>,
    memory_address: &BTreeSet<u32>,
    functions: &[(String, u32, u32)],
    details: &[Selection],
) -> ByteCauses {
    let mut order: Vec<String> = causes.iter().map(|c| c.name.clone()).collect();
    for r in [
        ROLE_STACK,
        ROLE_CONTROL,
        ROLE_MEMORY,
        ROLE_MEMORY_ADDRESS,
        ROLE_CONSTANT,
        ROLE_ARITHMETIC,
        ROLE_EFFECT,
        ROLE_DATA,
    ] {
        order.push(r.to_string());
    }
    let mut functions = functions.to_vec();
    functions.sort_by_key(|f| f.1);
    let region_of = |pc: u32| -> String {
        let i = functions.partition_point(|f| f.1 <= pc);
        match i.checked_sub(1).map(|i| &functions[i]) {
            Some((name, start, end)) if *start <= pc && pc < *end => name.clone(),
            _ => "(outside functions)".into(),
        }
    };
    let mut buckets: BTreeMap<String, CauseTally> = BTreeMap::new();
    let mut regions: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut detail: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    for inst in decode(&code[..code_end]) {
        let bucket = causes
            .iter()
            .find(|c| c.pcs.contains(&inst.pc))
            .map_or_else(
                || role(inst.opcode, inst.pc, labels, memory_address).to_string(),
                |c| c.name.clone(),
            );
        if !details.is_empty() {
            let label = details
                .iter()
                .find(|d| d.pcs.contains(&inst.pc))
                .map_or("(none)", |d| d.name.as_str());
            *detail
                .entry(bucket.clone())
                .or_default()
                .entry(label.to_string())
                .or_default() += u64::from(inst.len);
        }
        let t = buckets.entry(bucket.clone()).or_default();
        t.bytes += u64::from(inst.len);
        t.instructions += 1;
        *regions
            .entry(region_of(inst.pc))
            .or_default()
            .entry(bucket)
            .or_default() += u64::from(inst.len);
    }
    if code_end < code.len() {
        buckets.entry(ROLE_DATA.into()).or_default().bytes += (code.len() - code_end) as u64;
    }
    let mut by_region: Vec<(String, u64, BTreeMap<String, u64>)> = regions
        .into_iter()
        .map(|(n, b)| (n, b.values().sum(), b))
        .collect();
    by_region.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ByteCauses {
        schema: BYTE_CAUSES_SCHEMA.into(),
        artifact_bytes: code.len() as u64,
        code_end: code_end as u64,
        order,
        buckets,
        by_region,
        detail,
    }
}

/// One row of a comparison: a bucket's bytes on both sides.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CauseDelta {
    pub bucket: String,
    pub left: u64,
    pub right: u64,
    pub excess: i64,
}

/// Bucket-by-bucket difference, largest excess first. The excesses add up
/// to `left.artifact_bytes - right.artifact_bytes`.
pub fn compare_causes(left: &ByteCauses, right: &ByteCauses) -> Vec<CauseDelta> {
    let names: BTreeSet<&String> = left.buckets.keys().chain(right.buckets.keys()).collect();
    let mut out: Vec<CauseDelta> = names
        .into_iter()
        .map(|n| {
            let l = left.buckets.get(n).map_or(0, |t| t.bytes);
            let r = right.buckets.get(n).map_or(0, |t| t.bytes);
            CauseDelta {
                bucket: n.clone(),
                left: l,
                right: r,
                excess: l as i64 - r as i64,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.excess
            .cmp(&a.excess)
            .then_with(|| a.bucket.cmp(&b.bucket))
    });
    out
}

/// An estimate, not a measurement: each role bucket's positive excess
/// split over the left side's detail labels in proportion to the left
/// side's bytes in that bucket; named causes keep their own excess. The
/// result adds up to the same total. Labels are summed across buckets.
pub fn apportion_excess(left: &ByteCauses, deltas: &[CauseDelta]) -> Vec<(String, f64)> {
    let mut out: BTreeMap<String, f64> = BTreeMap::new();
    for d in deltas {
        let split = d.bucket.starts_with("role: ") && d.excess > 0;
        match left.detail.get(&d.bucket).filter(|_| split) {
            Some(labels) => {
                let total: u64 = labels.values().sum();
                for (label, bytes) in labels {
                    let name = if label == "(none)" {
                        format!("{} (no mechanism label)", d.bucket)
                    } else {
                        label.clone()
                    };
                    *out.entry(name).or_default() +=
                        d.excess as f64 * *bytes as f64 / total.max(1) as f64;
                }
            }
            None => *out.entry(d.bucket.clone()).or_default() += d.excess as f64,
        }
    }
    let mut v: Vec<(String, f64)> = out.into_iter().collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_byte_lands_in_one_bucket_and_named_causes_win() {
        // PUSH1 0x40 MLOAD DUP1 PUSH1 6 JUMP JUMPDEST STOP, then 2 data bytes.
        let code = [
            0x60, 0x40, 0x51, 0x80, 0x60, 0x07, 0x56, 0x5b, 0x00, 0xaa, 0xbb,
        ];
        let labels: BTreeSet<u32> = [4].into();
        let mem: BTreeSet<u32> = [0].into();
        let cause = Selection {
            name: "clamp".into(),
            pcs: [2].into(),
        };
        let c = classify_bytes(
            &code,
            9,
            &[cause],
            &labels,
            &mem,
            &[("f".into(), 0, 9)],
            &[],
        );
        let b = |k: &str| c.buckets.get(k).map_or(0, |t| t.bytes);
        assert_eq!(b("clamp"), 1);
        assert_eq!(b(ROLE_MEMORY_ADDRESS), 2);
        assert_eq!(b(ROLE_STACK), 1);
        assert_eq!(b(ROLE_CONTROL), 4);
        assert_eq!(b(ROLE_EFFECT), 1);
        assert_eq!(b(ROLE_DATA), 2);
        let total: u64 = c.buckets.values().map(|t| t.bytes).sum();
        assert_eq!(total, code.len() as u64);
        let empty = classify_bytes(&code[..1], 1, &[], &labels, &mem, &[], &[]);
        let d = compare_causes(&c, &empty);
        assert_eq!(
            d.iter().map(|x| x.excess).sum::<i64>(),
            code.len() as i64 - 1
        );
    }
}
