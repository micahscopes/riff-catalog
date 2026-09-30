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

use riff_catalog_evm::decode::{MEMORY_OPCODES, decode};
use serde::{Deserialize, Serialize};

use anyhow::{Context, Result, bail, ensure};
use riff_catalog_ingest_trace::bytes::DetailsRow;
pub use riff_catalog_ingest_trace::bytes::Tally as CauseTally;

use crate::regions::FunctionRegions;
use crate::selection::{
    Selection, check_instruction_starts, needles, pattern_selection, range_selection, read_pc_set,
};
use crate::sonatina_functions::{SONATINA_FUNCTIONS_SCHEMA, SonatinaFunctions};

pub const BYTE_CAUSES_SCHEMA: &str = "riffcat-evm-byte-causes/2";
pub const BYTE_CAUSES_COMPARE_SCHEMA: &str = "riffcat-evm-byte-causes-compare/1";

/// The `evm-byte-causes-compare` report.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CauseComparison {
    pub schema: String,
    pub left_artifact_blake3: String,
    pub right_artifact_blake3: String,
    pub rows: Vec<CauseDelta>,
}

pub const ROLE_STACK: &str = "role: stack shuffle (DUP, SWAP, POP)";
pub const ROLE_CONTROL: &str = "role: control flow (JUMP, JUMPI, JUMPDEST, label pushes)";
pub const ROLE_MEMORY: &str = "role: memory access (MLOAD, MSTORE, MSTORE8, MCOPY, MSIZE)";
pub const ROLE_MEMORY_ADDRESS: &str = "role: memory address constants";
pub const ROLE_CONSTANT: &str = "role: other constant pushes";
pub const ROLE_ARITHMETIC: &str = "role: arithmetic, comparison, bitwise";
pub const ROLE_EFFECT: &str = "role: calls, environment, storage, logs, hashing, halts";
pub const ROLE_DATA: &str = "data after the code (metadata, constants)";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ByteCauses {
    pub schema: String,
    pub artifact_blake3: String,
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
        op if MEMORY_OPCODES.contains(&op) => ROLE_MEMORY,
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
/// Causes and details must have distinct names, no cause may be named like
/// a role bucket, and every pc they hold must start an instruction before
/// `code_end`.
pub fn classify_bytes(
    code: &[u8],
    code_end: usize,
    causes: &[Selection],
    labels: &BTreeSet<u32>,
    memory_address: &BTreeSet<u32>,
    functions: &FunctionRegions,
    details: &[Selection],
) -> Result<ByteCauses> {
    ensure!(
        code_end <= code.len(),
        "code end {code_end} is past the {}-byte artifact",
        code.len()
    );
    for list in [causes, details] {
        let mut seen = BTreeSet::new();
        for s in list {
            ensure!(
                seen.insert(&s.name),
                "two causes or details are named `{}`",
                s.name
            );
            check_instruction_starts(s, &code[..code_end])?;
        }
    }
    let roles = [
        ROLE_STACK,
        ROLE_CONTROL,
        ROLE_MEMORY,
        ROLE_MEMORY_ADDRESS,
        ROLE_CONSTANT,
        ROLE_ARITHMETIC,
        ROLE_EFFECT,
        ROLE_DATA,
    ];
    if let Some(c) = causes.iter().find(|c| roles.contains(&c.name.as_str())) {
        bail!("cause `{}` has the name of a role bucket", c.name);
    }
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
        buckets
            .entry(bucket.clone())
            .or_default()
            .add_bytes(u64::from(inst.len));
        *regions
            .entry(functions.label_at(inst.pc))
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
    Ok(ByteCauses {
        schema: BYTE_CAUSES_SCHEMA.into(),
        artifact_blake3: blake3::hash(code).to_hex().to_string(),
        artifact_bytes: code.len() as u64,
        code_end: code_end as u64,
        order,
        buckets,
        by_region,
        detail,
    })
}

/// What a `--cause` or `--detail` argument can refer to.
pub struct CauseInputs<'a> {
    /// The whole artifact, which census inputs must describe.
    pub artifact: &'a [u8],
    /// The instructions (the artifact up to its code end).
    pub code: &'a [u8],
    pub functions: &'a FunctionRegions,
    /// Fe attribution rows, for `bodies:`.
    pub rows: Option<&'a [DetailsRow]>,
}

/// Build one named cause from `kind:NAME=arg`: `pcs:NAME=path.json`,
/// `pattern:NAME=hex`, `repeats:NAME=census.json`,
/// `duplicates:NAME=sonatina-functions.json#facet-index`, `regions:NAME=a|b`
/// or `bodies:NAME=a|b`. `read` loads a file named in the argument.
pub fn cause_selection(
    spec: &str,
    inputs: &CauseInputs,
    read: &dyn Fn(&str) -> Result<Vec<u8>>,
) -> Result<Selection> {
    let code = inputs.code;
    let functions = inputs.functions;
    let (kind, rest) = spec.split_once(':').context("--cause kind:NAME=arg")?;
    let (name, arg) = rest.split_once('=').context("--cause kind:NAME=arg")?;
    let pcs: BTreeSet<u32> = match kind {
        "pcs" => {
            let set = read_pc_set(name, &read(arg)?).with_context(|| format!("pc set {arg}"))?;
            check_instruction_starts(&set, code).with_context(|| format!("pc set {arg}"))?;
            set.pcs
        }
        "pattern" => pattern_selection(name, code, arg)?.pcs,
        "repeats" => {
            let classes = crate::census_input::parse_census_runs(
                &read(arg)?,
                inputs.artifact,
                &format!("repeats: {arg}"),
            )?;
            let mut ranges = Vec::new();
            for class in classes {
                let mut occ = class.ranges;
                occ.sort();
                ranges.extend(occ.into_iter().skip(1));
            }
            range_selection(name, code, &ranges).pcs
        }
        "duplicates" => {
            let (path, index) = arg.split_once('#').unwrap_or((arg, "0"));
            let census: SonatinaFunctions = serde_json::from_slice(&read(path)?)
                .with_context(|| format!("duplicates: parse {path}"))?;
            ensure!(
                census.schema == SONATINA_FUNCTIONS_SCHEMA,
                "duplicates: {path} is `{}`, expected `{SONATINA_FUNCTIONS_SCHEMA}`",
                census.schema
            );
            let facet = census
                .facets
                .get(index.parse::<usize>()?)
                .with_context(|| {
                    format!(
                        "duplicates: {path} has {} facets, no facet {index}",
                        census.facets.len()
                    )
                })?;
            let mut ranges = Vec::new();
            for class in &facet.classes {
                let largest = class
                    .functions
                    .iter()
                    .filter_map(|(n, b)| b.map(|b| (b, n.clone())))
                    .max();
                for (n, b) in &class.functions {
                    if b.is_some() && largest.as_ref().map(|l| &l.1) != Some(n) {
                        ranges.extend(
                            functions
                                .iter()
                                .filter(|f| &f.name == n)
                                .map(|f| (f.start, f.end)),
                        );
                    }
                }
            }
            range_selection(name, code, &ranges).pcs
        }
        "regions" => {
            let needles = needles(arg)?;
            let ranges: Vec<(u32, u32)> = functions
                .iter()
                .filter(|f| needles.iter().any(|n| f.name.contains(n)))
                .map(|f| (f.start, f.end))
                .collect();
            range_selection(name, code, &ranges).pcs
        }
        "bodies" => {
            let rows = inputs.rows.context("bodies: needs --attribution")?;
            let needles = needles(arg)?;
            let hit = |key: &str| {
                riff_catalog_ingest_trace::bytes::source_body(key)
                    .is_some_and(|b| needles.iter().any(|n| b.contains(n)))
            };
            rows.iter()
                .filter(|r| match &r.primary_source {
                    Some(p) => hit(p),
                    None => {
                        r.classification_reason.as_deref() == Some("SyntheticFor")
                            && r.all_origins.iter().any(|o| hit(o))
                    }
                })
                .map(|r| r.pc_start)
                .collect()
        }
        other => bail!("unknown cause kind `{other}`"),
    };
    Ok(Selection {
        name: name.to_string(),
        pcs,
    })
}

/// The excess ledger of `code` (instructions end at `code_end`): jump
/// labels and memory-address constants found by the EVM crate's rules,
/// then [`classify_bytes`], checked to cover every byte.
pub fn byte_cause_ledger(
    code: &[u8],
    code_end: usize,
    causes: &[Selection],
    functions: &FunctionRegions,
    details: &[Selection],
) -> Result<ByteCauses> {
    let insts = decode(&code[..code_end]);
    let labels = riff_catalog_evm::runs::with_labels(&code[..code_end], &insts)
        .into_iter()
        .filter(|(_, l)| l.is_some())
        .map(|(i, _)| i.pc)
        .collect();
    let memory_address = riff_catalog_evm::dataflow::lift_code(&code[..code_end])?
        .into_iter()
        .flat_map(|b| b.memory_offset_pushes)
        .collect();
    let ledger = classify_bytes(
        code,
        code_end,
        causes,
        &labels,
        &memory_address,
        functions,
        details,
    )?;
    let total: u64 = ledger.buckets.values().map(|t| t.bytes).sum();
    ensure!(
        total == ledger.artifact_bytes,
        "buckets cover {total} of {} bytes",
        ledger.artifact_bytes
    );
    Ok(ledger)
}

impl ByteCauses {
    /// Refuse a ledger that does not put every byte of its artifact in
    /// exactly one bucket: buckets must add up to the artifact, the data
    /// bucket must be the bytes after the code end, and every bucket must
    /// be listed once in `order`.
    pub fn check(&self) -> Result<()> {
        let total: u64 = self.buckets.values().map(|t| t.bytes).sum();
        ensure!(
            total == self.artifact_bytes,
            "its buckets add up to {total} bytes, but its artifact is {} bytes ({:+} bytes)",
            self.artifact_bytes,
            total as i64 - self.artifact_bytes as i64
        );
        ensure!(
            self.code_end <= self.artifact_bytes,
            "its code end {} is past its {}-byte artifact",
            self.code_end,
            self.artifact_bytes
        );
        let data = self.buckets.get(ROLE_DATA).map_or(0, |t| t.bytes);
        ensure!(
            data == self.artifact_bytes - self.code_end,
            "its data bucket holds {data} bytes, but {} bytes follow its code end",
            self.artifact_bytes - self.code_end
        );
        let mut listed = BTreeSet::new();
        for name in &self.order {
            ensure!(listed.insert(name), "it lists bucket `{name}` twice");
        }
        if let Some(name) = self.buckets.keys().find(|b| !listed.contains(b)) {
            bail!("its bucket `{name}` is not listed in its order");
        }
        Ok(())
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
            &FunctionRegions::new(vec![crate::regions::FunctionRegion {
                name: "f".into(),
                start: 0,
                end: 9,
            }]),
            &[],
        )
        .unwrap();
        let b = |k: &str| c.buckets.get(k).map_or(0, |t| t.bytes);
        assert_eq!(b("clamp"), 1);
        assert_eq!(b(ROLE_MEMORY_ADDRESS), 2);
        assert_eq!(b(ROLE_STACK), 1);
        assert_eq!(b(ROLE_CONTROL), 4);
        assert_eq!(b(ROLE_EFFECT), 1);
        assert_eq!(b(ROLE_DATA), 2);
        let total: u64 = c.buckets.values().map(|t| t.bytes).sum();
        assert_eq!(total, code.len() as u64);
        let empty = classify_bytes(
            &code[..1],
            1,
            &[],
            &labels,
            &mem,
            &FunctionRegions::default(),
            &[],
        )
        .unwrap();
        let d = compare_causes(&c, &empty);
        assert_eq!(
            d.iter().map(|x| x.excess).sum::<i64>(),
            code.len() as i64 - 1
        );
    }
}
