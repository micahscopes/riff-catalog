//! Byte census of one Fe EVM contract from Fe's own trace and attribution.
//!
//! Inputs are what Fe already emits: the trace bundle (`fe dev trace emit`)
//! and the attribution details (`fe dev debug emit --format ethdebug
//! --attribution-details`), plus the real runtime artifact. The trace must
//! describe exactly that artifact: length, code hash and every immediate are
//! checked before anything is counted.
//!
//! The report has two independent parts:
//!
//! - where the bytes go: every byte is counted once by emitted function, by
//!   Fe's attribution classification, and by primary source body, and the
//!   totals add up to the artifact length. "Origin anywhere" totals are also
//!   given; those overlap by construction.
//! - repeated regions: the artifact census with EVM runs (see
//!   `riff_catalog_evm::runs`), scoped to the emitted functions, with each
//!   occurrence joined back to its emitted function and dominant source body.

use std::collections::{BTreeMap, BTreeSet};

use crate::regions::FunctionRegions;
use anyhow::{Context, Result, bail, ensure};
use riff_catalog_ingest_trace::bytes::{
    ByteLedger, LedgerError, LedgerInstruction, LedgerSelector, Tally, dominates,
    immediate_dominators, source_body, tally_by, tally_origin_anywhere,
};
use serde::{Deserialize, Serialize};

use crate::{
    ArtifactCensus, CallGraphCompleteness, DirectCall, EvmRunOptions, Function, RegionManifest,
    RegionSpec, Stage, census_regions, reachable_union,
};

pub const FE_TRACE_REGIONS_ADAPTER: &str = "fe-trace-emitted-functions/1";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Row {
    pub key: String,
    pub bytes: u64,
    pub instructions: u64,
}

fn rows<K>(map: BTreeMap<K, Tally>, label: impl Fn(&K) -> String) -> Vec<Row> {
    let mut out: Vec<Row> = map
        .iter()
        .map(|(k, t)| Row {
            key: label(k),
            bytes: t.bytes,
            instructions: t.instructions,
        })
        .collect();
    out.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.key.cmp(&b.key)));
    out
}

/// A source body as a reader sees it: a readable name and where it is.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BodyInfo {
    pub body: String,
    pub name: String,
    pub file: Option<String>,
    pub first_line: Option<u32>,
    pub last_line: Option<u32>,
    /// Text of the first line, when the file is a readable local `file://` URI.
    pub first_line_text: Option<String>,
}

/// Readable name for a HIR body id such as
/// `func$Core$core$lib$abi$fn$store_word$` or
/// `contract_recv$Local$seaport$seaport$contract$Seaport$0$3`. Display only:
/// the raw id stays the key everywhere.
pub fn body_name(body: &str) -> String {
    let parts: Vec<&str> = body.split('$').collect();
    if parts.first().is_some_and(|p| p.starts_with("contract_"))
        && let Some(at) = parts.iter().rposition(|p| *p == "contract")
    {
        let rest = &parts[at + 1..];
        let what = parts[0].trim_start_matches("contract_");
        return match (what, rest) {
            ("recv", [contract, recv, arm, ..]) => format!("{contract} recv {recv} arm {arm}"),
            (_, [contract, ..]) => format!("{contract} {what}"),
            _ => body.to_string(),
        };
    }
    let Some(fn_at) = parts.iter().position(|p| *p == "fn") else {
        return body.to_string();
    };
    if parts.first() != Some(&"func") || fn_at + 1 >= parts.len() || fn_at < 2 {
        return body.to_string();
    }
    let path: Vec<&str> = parts[2..fn_at]
        .iter()
        .copied()
        .filter(|p| *p != "lib")
        .collect();
    let name = parts[fn_at + 1];
    let rest = &parts[fn_at + 2..];
    let self_ty = || -> String {
        let tail = rest
            .iter()
            .position(|p| *p == "assoc")
            .map_or(rest, |a| &rest[a + 1..]);
        let named = tail
            .iter()
            .position(|p| matches!(*p, "struct" | "enum" | "prim"))
            .and_then(|i| tail.get(i + 1));
        match named {
            Some(n) => n.to_string(),
            None if tail.contains(&"param") => "T".into(),
            None => "?".into(),
        }
    };
    match rest.first() {
        Some(&"impl_trait") => {
            let tr = rest
                .iter()
                .position(|p| *p == "trait")
                .and_then(|i| rest.get(i + 1))
                .unwrap_or(&"?");
            format!("{}::{name} (impl {tr} for {})", path.join("::"), self_ty())
        }
        Some(&"impl") => format!("{}::{}::{name}", path.join("::"), self_ty()),
        _ => format!("{}::{name}", path.join("::")),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunOccurrenceInfo {
    pub start: u32,
    pub end: u32,
    pub emitted_function: Option<String>,
    /// Source body with the most primary-attributed bytes in this occurrence.
    pub top_body: Option<String>,
    pub top_body_bytes: u64,
    /// Bytes in this occurrence with no single primary source.
    pub no_primary_bytes: u64,
    /// Recv arm owning the most bytes of this occurrence, if any arm owns
    /// some, and how many bytes it owns.
    pub arm: Option<String>,
    pub arm_bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunInfo {
    pub digest: String,
    pub copies: usize,
    pub bytes_per_copy: u64,
    pub instructions_per_copy: usize,
    pub covered_bytes: u64,
    /// Bytes beyond the first copy: a rough upper bound on what sharing the
    /// region could save before call/return and parameter costs.
    pub extra_bytes: u64,
    pub ports: usize,
    pub varying_ports: usize,
    /// Constant ports (only when constants are compared as ports).
    pub constant_ports: usize,
    pub varying_constant_ports: usize,
    pub occurrences: Vec<RunOccurrenceInfo>,
    /// Instructions (by position in the run) whose primary source is present
    /// and identical in every copy: the same Fe source emitted more than once.
    pub same_source_instructions: usize,
    /// Mnemonics of the first copy, with immediates.
    pub listing: Vec<String>,
}

/// Emitted functions reachable from one recv arm through inferred calls.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArmReach {
    pub arm: String,
    /// Functions reached from this arm only (not from other arms or from
    /// the dispatcher's shared code), with their bytes.
    pub exclusive_functions: Vec<(String, u64)>,
    pub exclusive_bytes: u64,
    /// All functions reached from this arm, shared ones included.
    pub reached: Vec<String>,
    pub reached_functions: usize,
    pub reached_bytes: u64,
}

/// One recv arm's share of the code.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArmInfo {
    /// HIR body id of the arm (`contract_recv$...$<Contract>$<recv>$<arm>`).
    pub arm: String,
    /// Emitted function whose post-optimization CFG holds the arm's inlined
    /// region, with that region's entry block, if one was found.
    pub inlined_into: Option<String>,
    pub region_entry: Option<String>,
    pub region_blocks: usize,
    /// Bytes in blocks dominated by the arm's region entry.
    pub inlined_bytes: u64,
    /// Bytes of the arm's own emitted function when it was not inlined.
    pub own_function_bytes: u64,
    /// Why no inlined region was assigned, when one was looked for.
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FeTraceBytesReport {
    pub schema: String,
    pub contract: String,
    pub artifact_bytes: u64,
    pub artifact_blake3: String,
    pub trace_code_hash_checked: bool,
    /// Bytes covered by instructions in the trace.
    pub instruction_bytes: u64,
    /// Bytes after the code that the code hash proves belong to the traced
    /// code object but that the trace does not describe as instructions.
    pub trailing_data_bytes: u64,
    pub by_classification: Vec<Row>,
    /// By position: the emitted function whose region (first to last linked
    /// byte) holds the byte. Adds up.
    pub by_emitted_function: Vec<Row>,
    /// Only bytes the trace links to a function (`bytecode.pc -> evm.vcode.inst`).
    pub by_emitted_function_linked: Vec<Row>,
    /// Unlinked bytes counted in a function because they form its entry (see
    /// [`emitted_function_manifest`]).
    pub entry_bytes_by_position: u64,
    pub by_primary_body: Vec<Row>,
    pub by_primary_file: Vec<Row>,
    pub no_primary_by_emitted_function: Vec<Row>,
    pub origin_anywhere_by_body: Vec<Row>,
    pub bodies: BTreeMap<String, BodyInfo>,
    /// Bytes per recv arm plus one row for everything no arm owns; adds up.
    pub by_recv_arm: Vec<Row>,
    /// Like `by_recv_arm`, plus each emitted function reached from one arm
    /// only; functions reached from several arms or from the shared dispatch
    /// code get rows of their own. Adds up.
    pub by_recv_arm_with_helpers: Vec<Row>,
    pub arms: Vec<ArmInfo>,
    pub arm_reach: Vec<ArmReach>,
    pub runs_min_bytes: u32,
    pub runs_policy: Option<String>,
    pub runs_selected_bytes: u64,
    pub runs: Vec<RunInfo>,
}

/// The emitted-function regions of the artifact, as a digest-bound manifest.
/// A function's region spans its first to last linked byte; bytes inside it
/// that the trace does not link to any function are counted in it by
/// position. An unlinked gap directly before a function is that function's
/// entry, and joins its region, when it starts with a JUMPDEST and every other
/// JUMPDEST in it is reached only from the gap or the function (the backend
/// emits the entry JUMPDEST and argument set-up without a vcode link; a
/// shared tail, reached from several functions, is not an entry).
/// Other bytes between functions become `unattributed` regions. If two
/// functions' spans interleave, each maximal same-function range is its own
/// region and no entry is inferred.
pub fn emitted_function_manifest(
    ledger: &ByteLedger,
    artifact: &[u8],
    evm_runs: Option<EvmRunOptions>,
) -> RegionManifest {
    let mut spans: BTreeMap<&str, (u32, u32)> = BTreeMap::new();
    for inst in &ledger.instructions {
        if let Some(f) = inst.emitted_function.as_deref() {
            let e = spans.entry(f).or_insert((inst.pc_start, inst.pc_end));
            e.0 = e.0.min(inst.pc_start);
            e.1 = e.1.max(inst.pc_end);
        }
    }
    let mut sorted: Vec<(&str, u32, u32)> = spans.iter().map(|(f, (s, e))| (*f, *s, *e)).collect();
    sorted.sort_by_key(|(_, s, _)| *s);
    let interleaved = sorted.windows(2).any(|w| w[1].1 < w[0].2);
    let named: Vec<(Option<&str>, u32, u32)> = if interleaved {
        ledger.function_ranges()
    } else {
        let code = &artifact[..(ledger.code_len as usize).min(artifact.len())];
        let insts = riff_catalog_evm::decode::decode(code);
        let jumpdests: BTreeSet<u32> = riff_catalog_evm::decode::jumpdests(code)
            .into_iter()
            .collect();
        // Label pushes: target -> pushing pcs.
        let mut pushed_from: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for (i, label) in insts
            .iter()
            .zip(riff_catalog_evm::decode::jump_labels(code, &insts))
        {
            if let Some(v) = label {
                pushed_from.entry(v).or_default().push(i.pc);
            }
        }
        let mut out = Vec::new();
        let mut cursor = 0u32;
        for (f, s, e) in sorted {
            let mut start = s;
            if s > cursor {
                // The gap is `f`'s entry when it starts with a JUMPDEST and
                // every other JUMPDEST in it is only reached from the gap or
                // from `f` itself (a shared tail is reached from elsewhere).
                let local = jumpdests.contains(&cursor)
                    && jumpdests.range(cursor + 1..s).all(|j| {
                        pushed_from
                            .get(j)
                            .into_iter()
                            .flatten()
                            .all(|p| (cursor..e).contains(p))
                    });
                if local {
                    start = cursor;
                } else {
                    out.push((None, cursor, s));
                }
            }
            out.push((Some(f), start, e));
            cursor = e;
        }
        if cursor < ledger.code_len {
            out.push((None, cursor, ledger.code_len));
        }
        out
    };
    let data = (artifact.len() as u32 > ledger.code_len).then(|| RegionSpec {
        id: format!("data:{}", ledger.code_len),
        kind: "data".into(),
        name: TRAILING_DATA.into(),
        start: ledger.code_len as usize,
        end: artifact.len(),
    });
    let regions = named
        .into_iter()
        .map(|(f, s, e)| RegionSpec {
            id: format!(
                "{}:{s}",
                if f.is_some() {
                    "function"
                } else {
                    "unattributed"
                }
            ),
            kind: if f.is_some() {
                "function"
            } else {
                "unattributed"
            }
            .into(),
            name: f.unwrap_or("(no emitted function in trace)").to_string(),
            start: s as usize,
            end: e as usize,
        })
        .chain(data)
        .collect();
    RegionManifest {
        schema: "riffcat-regions/1".into(),
        artifact_blake3: blake3::hash(artifact).to_hex().to_string(),
        adapter: FE_TRACE_REGIONS_ADAPTER.into(),
        regions,
        evm_runs,
    }
}

fn primary_body(inst: &LedgerInstruction) -> Option<&str> {
    inst.primary_source.as_deref().and_then(source_body)
}

/// Build the ledger, check it against the artifact, and census it.
pub fn fe_trace_bytes(
    trace: impl std::io::BufRead,
    attribution_details_json: &str,
    contract: &str,
    artifact: &[u8],
    run_options: &EvmRunOptions,
) -> Result<(
    ByteLedger,
    RegionManifest,
    ArtifactCensus,
    FeTraceBytesReport,
)> {
    let ledger = ByteLedger::read(
        trace,
        attribution_details_json,
        &LedgerSelector {
            contract: contract.to_string(),
        },
    )
    .map_err(|e: LedgerError| anyhow::anyhow!("{e}"))?;
    let check = ledger
        .verify_artifact(artifact)
        .map_err(|e| anyhow::anyhow!("trace does not describe this artifact: {e}"))?;
    let total: u64 = ledger
        .instructions
        .iter()
        .map(LedgerInstruction::bytes)
        .sum();
    ensure!(
        total + u64::from(check.trailing_bytes) == artifact.len() as u64,
        "instruction bytes {total} plus trailing data {} != artifact {}",
        check.trailing_bytes,
        artifact.len()
    );
    // Trailing data is a row of its own in every table that adds up.
    let with_data = |mut rows: Vec<Row>| {
        if check.trailing_bytes > 0 {
            rows.push(Row {
                key: TRAILING_DATA.into(),
                bytes: u64::from(check.trailing_bytes),
                instructions: 0,
            });
            rows.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.key.cmp(&b.key)));
        }
        rows
    };

    let manifest = emitted_function_manifest(&ledger, artifact, Some(run_options.clone()));
    let census = census_regions(artifact, manifest.clone())?;

    let by_classification = rows(
        tally_by(&ledger, |i| {
            format!(
                "{} / {} / {}",
                i.classification,
                i.confidence,
                i.classification_reason.as_deref().unwrap_or("-")
            )
        }),
        |k| k.clone(),
    );
    let fn_label = |k: &Option<String>| {
        k.clone()
            .unwrap_or_else(|| "(no emitted function in trace)".into())
    };
    let by_emitted_function_linked =
        rows(tally_by(&ledger, |i| i.emitted_function.clone()), fn_label);
    // By position: the function region (from the manifest) holding each pc.
    let functions = FunctionRegions::from_manifest(&manifest);
    let region_name = |pc: u32| -> String {
        functions
            .name_at(pc)
            .unwrap_or(BETWEEN_FUNCTIONS)
            .to_string()
    };
    let by_emitted_function = rows(tally_by(&ledger, |i| region_name(i.pc_start)), |k| {
        k.clone()
    });
    let mut first_linked: BTreeMap<&str, u32> = BTreeMap::new();
    for inst in &ledger.instructions {
        if let Some(f) = inst.emitted_function.as_deref() {
            first_linked.entry(f).or_insert(inst.pc_start);
        }
    }
    let entry_bytes_by_position: u64 = functions
        .iter()
        .filter_map(|r| {
            first_linked
                .get(r.name.as_str())
                .map(|f| u64::from(*f) - u64::from(r.start))
        })
        .sum();
    let no_primary =
        |i: &LedgerInstruction| format!("(no single primary source: {})", i.classification);
    let by_primary_body = rows(
        tally_by(&ledger, |i| {
            primary_body(i)
                .map(str::to_string)
                .unwrap_or_else(|| no_primary(i))
        }),
        |k| k.clone(),
    );
    let by_primary_file = rows(
        tally_by(&ledger, |i| {
            i.primary_source
                .as_ref()
                .and_then(|o| ledger.source_spans.get(o))
                .map(|s| {
                    ledger
                        .source_files
                        .get(&s.file)
                        .cloned()
                        .unwrap_or_else(|| s.file.clone())
                })
                .unwrap_or_else(|| no_primary(i))
        }),
        |k| k.clone(),
    );
    let no_primary_by_emitted_function = rows(
        tally_by(&ledger, |i| {
            i.primary_source.is_none().then(|| region_name(i.pc_start))
        }),
        |k| match k {
            None => "(has a primary source)".into(),
            Some(f) => f.clone(),
        },
    )
    .into_iter()
    .filter(|r| r.key != "(has a primary source)")
    .collect();
    let origin_anywhere_by_body = rows(tally_origin_anywhere(&ledger), |k| k.clone());

    // Body locations from every span the ledger carries.
    let mut bodies: BTreeMap<String, BodyInfo> = BTreeMap::new();
    for (origin, span) in &ledger.source_spans {
        let Some(body) = source_body(origin) else {
            continue;
        };
        let file = ledger
            .source_files
            .get(&span.file)
            .cloned()
            .unwrap_or_else(|| span.file.clone());
        let info = bodies.entry(body.to_string()).or_insert_with(|| BodyInfo {
            body: body.to_string(),
            name: body_name(body),
            file: Some(file.clone()),
            first_line: Some(span.start_line),
            last_line: Some(span.end_line),
            first_line_text: None,
        });
        if info.file.as_deref() == Some(file.as_str()) {
            info.first_line = info.first_line.map(|l| l.min(span.start_line));
            info.last_line = info.last_line.map(|l| l.max(span.end_line));
        }
    }
    for inst in &ledger.instructions {
        for origin in inst.all_origins.iter().chain(inst.primary_source.iter()) {
            if let Some(body) = source_body(origin) {
                bodies.entry(body.to_string()).or_insert_with(|| BodyInfo {
                    body: body.to_string(),
                    name: body_name(body),
                    file: None,
                    first_line: None,
                    last_line: None,
                    first_line_text: None,
                });
            }
        }
    }

    for info in bodies.values_mut() {
        info.first_line_text = info
            .file
            .as_deref()
            .zip(info.first_line)
            .and_then(|(f, l)| source_line(f, l));
    }

    let (arm_of, arms) = recv_arms(&ledger, &functions);
    let by_recv_arm = rows(
        tally_by(&ledger, |i| {
            arm_of
                .get(&i.pc_start)
                .cloned()
                .unwrap_or_else(|| NO_ARM.to_string())
        }),
        |k| k.clone(),
    );
    let (arm_reach, by_recv_arm_with_helpers) = arm_reach(&ledger, &functions, artifact, &arm_of)?;

    // Runs joined back to the ledger.
    let starts: Vec<u32> = ledger.instructions.iter().map(|i| i.pc_start).collect();
    let slice = |s: u32, e: u32| {
        let a = starts.partition_point(|p| *p < s);
        let b = starts.partition_point(|p| *p < e);
        &ledger.instructions[a..b]
    };
    let region_by_id: BTreeMap<&str, &crate::CensusRegion> = census
        .regions
        .iter()
        .map(|r| (r.region.id.as_str(), r))
        .collect();
    let mut runs = Vec::new();
    let mut runs_selected_bytes = 0u64;
    for pattern in census
        .patterns
        .iter()
        .filter(|p| p.region_kind == "evm_run")
    {
        let mut occurrences = Vec::new();
        for id in &pattern.regions {
            let r = region_by_id[id.as_str()];
            let (s, e) = (r.region.start as u32, r.region.end as u32);
            let insts = slice(s, e);
            let functions: BTreeSet<Option<&str>> = insts
                .iter()
                .map(|i| i.emitted_function.as_deref())
                .filter(|f| f.is_some())
                .collect();
            let mut body_bytes: BTreeMap<&str, u64> = BTreeMap::new();
            let mut no_primary_bytes = 0;
            for i in insts {
                match primary_body(i) {
                    Some(b) => *body_bytes.entry(b).or_default() += i.bytes(),
                    None => no_primary_bytes += i.bytes(),
                }
            }
            let top = body_bytes
                .iter()
                .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)));
            let mut by_arm: BTreeMap<&str, u64> = BTreeMap::new();
            for i in insts {
                if let Some(a) = arm_of.get(&i.pc_start) {
                    *by_arm.entry(a).or_default() += i.bytes();
                }
            }
            let top_arm = by_arm
                .into_iter()
                .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)));
            occurrences.push(RunOccurrenceInfo {
                start: s,
                end: e,
                emitted_function: (functions.len() == 1)
                    .then(|| functions.into_iter().next().flatten().map(str::to_string))
                    .flatten(),
                top_body: top.map(|(b, _)| b.to_string()),
                top_body_bytes: top.map_or(0, |(_, n)| *n),
                no_primary_bytes,
                arm: top_arm.map(|(a, _)| a.to_string()),
                arm_bytes: top_arm.map_or(0, |(_, b)| b),
            });
        }
        let copies: Vec<&[LedgerInstruction]> =
            occurrences.iter().map(|o| slice(o.start, o.end)).collect();
        let same_source_instructions = (0..copies[0].len())
            .filter(|&k| {
                let first = copies[0][k].primary_source.as_ref();
                first.is_some()
                    && copies
                        .iter()
                        .all(|c| c.get(k).and_then(|i| i.primary_source.as_ref()) == first)
            })
            .count();
        let first = &occurrences[0];
        let listing = slice(first.start, first.end)
            .iter()
            .map(|i| match &i.immediate {
                Some(imm) => format!("{} {imm}", i.mnemonic),
                None => i.mnemonic.clone(),
            })
            .collect();
        let bytes_per_copy = u64::from(first.end - first.start);
        runs_selected_bytes += pattern.covered_bytes as u64;
        let ports = pattern
            .evm_ports
            .clone()
            .context("EVM run pattern without port summary")?;
        runs.push(RunInfo {
            digest: pattern.digest.clone(),
            copies: occurrences.len(),
            bytes_per_copy,
            instructions_per_copy: pattern.statements_per_occurrence.unwrap_or_default(),
            covered_bytes: pattern.covered_bytes as u64,
            extra_bytes: bytes_per_copy * (occurrences.len() as u64 - 1),
            ports: ports.ports,
            varying_ports: ports.varying_ports,
            constant_ports: ports.constant_ports,
            varying_constant_ports: ports.varying_constant_ports,
            occurrences,
            same_source_instructions,
            listing,
        });
    }

    let report = FeTraceBytesReport {
        schema: "riffcat-fe-trace-bytes/1".into(),
        contract: contract.to_string(),
        artifact_bytes: artifact.len() as u64,
        artifact_blake3: census.artifact_blake3.clone(),
        trace_code_hash_checked: check.code_hash_checked,
        instruction_bytes: u64::from(check.instruction_bytes),
        trailing_data_bytes: u64::from(check.trailing_bytes),
        by_classification: with_data(by_classification),
        by_emitted_function: with_data(by_emitted_function),
        by_emitted_function_linked,
        entry_bytes_by_position,
        by_primary_body: with_data(by_primary_body),
        by_primary_file: with_data(by_primary_file),
        no_primary_by_emitted_function,
        origin_anywhere_by_body,
        bodies,
        by_recv_arm: with_data(by_recv_arm),
        by_recv_arm_with_helpers: with_data(by_recv_arm_with_helpers),
        arms,
        arm_reach,
        runs_min_bytes: run_options.min_run_bytes,
        runs_policy: census
            .patterns
            .iter()
            .find(|p| p.region_kind == "evm_run")
            .map(|p| p.policy.clone()),
        runs_selected_bytes,
        runs,
    };
    check_totals(&report)?;
    Ok((ledger, manifest, census, report))
}

/// Row label for code between function regions.
pub const BETWEEN_FUNCTIONS: &str = "(between functions: no emitted function in trace)";

/// Row label for bytes after the code that the trace has no instruction for.
pub const TRAILING_DATA: &str = "(data after the code: no instruction in the trace)";

/// Row label for bytes no recv arm owns.
pub const NO_ARM: &str = "(no single recv arm: dispatch, shared code or not assigned)";

fn source_line(file: &str, line: u32) -> Option<String> {
    let path = file.strip_prefix("file://")?;
    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    let at = line.checked_sub(1)? as usize;
    let first = lines.get(at)?.trim();
    // A body often starts at its brace; show the header line above it.
    if first.chars().all(|c| matches!(c, '{' | ')' | ' ')) {
        for back in (at.saturating_sub(16)..at).rev() {
            let l = lines[back].trim();
            if l.contains("::") || l.starts_with("fn ") || l.starts_with("pub fn ") {
                return Some(l.to_string());
            }
        }
    }
    Some(first.to_string())
}

/// Recv-arm bodies (`contract_recv$...`) among an instruction's origins.
fn arms_of(inst: &LedgerInstruction) -> BTreeSet<&str> {
    inst.all_origins
        .iter()
        .chain(inst.primary_source.iter())
        .filter_map(|o| source_body(o))
        .filter(|b| b.starts_with("contract_recv$"))
        .collect()
}

/// Assign bytes to recv arms.
///
/// A post-optimization block is labeled with the recv arms that appear among
/// the origins (Fe's attribution: primary source or any origin) of its
/// instructions. An arm left as its own function (`__<Contract>_recv_<r>_<a>`)
/// owns that function's bytes. An arm inlined into a function that holds
/// several arms (the dispatcher) owns the blocks dominated by its region
/// entry: the nearest common dominator of the blocks labeled with that arm
/// alone. The assignment is kept only if no block labeled with another arm
/// alone falls inside the region; otherwise the arm's inlined bytes stay
/// unassigned, with a note. Blocks reached from several arms (shared tails,
/// the dispatch itself) stay unassigned by construction.
fn recv_arms(
    ledger: &ByteLedger,
    functions: &FunctionRegions,
) -> (BTreeMap<u32, String>, Vec<ArmInfo>) {
    // An arm's own function is its whole region, entry and unlinked bytes
    // included, like every other by-position table.
    let region_of = |pc: u32| functions.name_at(pc);
    let mut arm_of: BTreeMap<u32, String> = BTreeMap::new();
    let mut arms: BTreeMap<String, ArmInfo> = BTreeMap::new();
    let new_arm = |arm: &str| ArmInfo {
        arm: arm.to_string(),
        inlined_into: None,
        region_entry: None,
        region_blocks: 0,
        inlined_bytes: 0,
        own_function_bytes: 0,
        note: None,
    };
    let mut labels: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut all_arms: BTreeSet<&str> = BTreeSet::new();
    for inst in &ledger.instructions {
        let here = arms_of(inst);
        all_arms.extend(here.iter().copied());
        if let Some(block) = inst.postopt_block.as_deref() {
            labels.entry(block).or_default().extend(here);
        }
    }
    // Arms that remained functions: `__<Contract>_recv_<recv>_<arm>`.
    let own_prefix = format!("__{}_recv_", ledger.contract);
    let own_function = |f: &str| -> Option<&str> {
        let (recv, arm) = f.strip_prefix(&own_prefix)?.split_once('_')?;
        let suffix = format!("${}${recv}${arm}", ledger.contract);
        all_arms.iter().copied().find(|a| a.ends_with(&suffix))
    };
    let mut block_arm: BTreeMap<&str, &str> = BTreeMap::new();
    for (function, cfg) in &ledger.cfgs {
        if own_function(function).is_some() {
            continue;
        }
        let local: BTreeMap<&str, &BTreeSet<&str>> = cfg
            .blocks
            .iter()
            .filter_map(|b| labels.get(b.as_str()).map(|l| (b.as_str(), l)))
            .collect();
        let arms_here: BTreeSet<&str> = local.values().flat_map(|l| l.iter().copied()).collect();
        if arms_here.len() < 2 {
            continue; // Not a dispatcher: nothing to separate.
        }
        let idom = immediate_dominators(cfg);
        for arm in arms_here {
            let info = arms.entry(arm.to_string()).or_insert_with(|| new_arm(arm));
            let only: Vec<&str> = local
                .iter()
                .filter(|(b, l)| l.len() == 1 && l.contains(arm) && idom.contains_key(**b))
                .map(|(b, _)| *b)
                .collect();
            let Some(mut entry) = only.first().copied() else {
                info.note = Some(format!("no block in {function} comes only from this arm"));
                continue;
            };
            for b in &only[1..] {
                while !dominates(&idom, entry, b) {
                    entry = idom[entry].as_str();
                }
            }
            let region: Vec<&str> = cfg
                .blocks
                .iter()
                .map(String::as_str)
                .filter(|b| idom.contains_key(*b) && dominates(&idom, entry, b))
                .collect();
            let intruder = region.iter().find(|b| {
                local
                    .get(**b)
                    .is_some_and(|l| l.len() == 1 && !l.contains(arm))
            });
            if let Some(b) = intruder {
                info.note = Some(format!(
                    "region entry {entry} in {function} also dominates block {b}, labeled with another arm only"
                ));
                continue;
            }
            info.inlined_into = Some(function.clone());
            info.region_entry = Some(entry.to_string());
            info.region_blocks = region.len();
            for b in region {
                block_arm.insert(b, arm);
            }
        }
    }
    for inst in &ledger.instructions {
        let inlined = inst
            .postopt_block
            .as_deref()
            .and_then(|b| block_arm.get(b).copied());
        let own = region_of(inst.pc_start).and_then(own_function);
        let Some(arm) = inlined.or(own) else { continue };
        let info = arms.entry(arm.to_string()).or_insert_with(|| new_arm(arm));
        if inlined.is_some() {
            info.inlined_bytes += inst.bytes();
        } else {
            info.own_function_bytes += inst.bytes();
        }
        arm_of.insert(inst.pc_start, arm.to_string());
    }
    (arm_of, arms.into_values().collect())
}

/// Row labels for `by_recv_arm_with_helpers`.
pub const SHARED_BY_ARMS: &str = "(functions reached from several recv arms)";
pub const DISPATCH_ONLY: &str = "(functions reached only from shared dispatch code)";
pub const UNREACHED: &str = "(functions not reached through inferred calls)";

/// Which emitted functions each recv arm reaches, through calls inferred from
/// label pushes whose value is the first byte of a function region (a call
/// pushes its callee's entry). Computed jumps are not seen, so the graph is
/// declared incomplete, and reachability uses the existing capture model's
/// [`reachable_union`] over a stage whose "instructions" are bytes.
fn arm_reach(
    ledger: &ByteLedger,
    functions: &FunctionRegions,
    artifact: &[u8],
    arm_of: &BTreeMap<u32, String>,
) -> Result<(Vec<ArmReach>, Vec<Row>)> {
    let by_start: BTreeMap<u32, &str> = functions
        .iter()
        .map(|r| (r.start, r.name.as_str()))
        .collect();
    let bytes_of: BTreeMap<&str, u64> = functions
        .iter()
        .map(|r| (r.name.as_str(), u64::from(r.end - r.start)))
        .collect();
    let owner_of = |pc: u32| functions.name_at(pc);
    // Functions that are some arm's own (not inlined) body.
    let own_arm: BTreeMap<&str, &str> = ledger
        .instructions
        .iter()
        .filter_map(|i| {
            let f = i.emitted_function.as_deref()?;
            let a = arm_of.get(&i.pc_start)?;
            (f.starts_with(&format!("__{}_recv_", ledger.contract))).then_some((f, a.as_str()))
        })
        .collect();
    let arm_node = |a: &str| format!("arm:{a}");
    let mut calls: BTreeMap<(String, String), u64> = BTreeMap::new();
    let code = &artifact[..ledger.code_len as usize];
    for i in riff_catalog_evm::decode::decode(code) {
        let Some(v) = riff_catalog_evm::decode::push_value(code, &i) else {
            continue;
        };
        let Some(callee) = by_start.get(&v) else {
            continue;
        };
        let Some(caller_fn) = owner_of(i.pc) else {
            continue;
        };
        if caller_fn == *callee {
            continue; // a jump back to its own entry, not a call
        }
        let caller = match arm_of.get(&i.pc) {
            Some(a) if !own_arm.contains_key(caller_fn) => arm_node(a),
            _ => caller_fn.to_string(),
        };
        // An arm's own function is entered from the dispatch on that arm's
        // behalf: the edge belongs to the arm, not to the shared dispatch.
        let caller = match own_arm.get(callee) {
            Some(a) if !caller.starts_with("arm:") => arm_node(a),
            _ => caller,
        };
        *calls.entry((caller, callee.to_string())).or_default() += 1;
    }
    let arms: BTreeSet<&str> = arm_of.values().map(String::as_str).collect();
    let mut stage_functions: Vec<Function> = functions
        .iter()
        .map(|r| Function {
            id: r.name.clone(),
            display_name: r.name.clone(),
            instructions: u64::from(r.end - r.start),
        })
        .collect();
    for a in &arms {
        stage_functions.push(Function {
            id: arm_node(a),
            display_name: arm_node(a),
            instructions: 0,
        });
    }
    let stage = Stage {
        id: "emitted-functions".into(),
        kind: "evm-bytecode".into(),
        predecessors: Vec::new(),
        functions: stage_functions,
        selected_entries: Vec::new(),
        direct_calls: calls
            .into_iter()
            .map(|((caller, callee), callsites)| DirectCall {
                caller,
                callee,
                callsites,
            })
            .collect(),
        call_graph: CallGraphCompleteness::Incomplete {
            reason:
                "calls inferred from label pushes to function entries; computed jumps are not seen"
                    .into(),
        },
        unknown_indirect_calls: Vec::new(),
        measurements: Vec::new(),
    };
    let reach = |entry: String| -> Result<BTreeSet<String>> {
        Ok(reachable_union(&stage, &[entry])?
            .functions
            .into_iter()
            .filter(|f| !f.starts_with("arm:"))
            .collect())
    };
    let mut per_arm: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for a in &arms {
        per_arm.insert(a, reach(arm_node(a))?);
    }
    // Dispatchers: the function at pc 0 (the runtime entry) and every
    // function holding an inlined arm region. Their bytes that no arm owns
    // are shared dispatch code, and what they call is reached on everyone's
    // behalf.
    let dispatchers: BTreeSet<&str> = owner_of(0)
        .into_iter()
        .chain(
            ledger
                .instructions
                .iter()
                .filter(|i| arm_of.contains_key(&i.pc_start))
                .filter_map(|i| owner_of(i.pc_start))
                .filter(|f| !own_arm.contains_key(f)),
        )
        .collect();
    let mut dispatch: BTreeSet<String> = BTreeSet::new();
    for d in &dispatchers {
        dispatch.extend(reach(d.to_string())?.into_iter().filter(|f| f != d));
    }
    let mut owner: BTreeMap<String, String> = BTreeMap::new();
    for f in bytes_of.keys() {
        let reached_by: Vec<&str> = per_arm
            .iter()
            .filter(|(_, r)| r.contains(*f))
            .map(|(a, _)| *a)
            .collect();
        let label = if dispatch.contains(*f) && !reached_by.is_empty() || reached_by.len() > 1 {
            SHARED_BY_ARMS.to_string()
        } else if let [a] = reached_by.as_slice() {
            a.to_string()
        } else if dispatch.contains(*f) {
            DISPATCH_ONLY.to_string()
        } else {
            UNREACHED.to_string()
        };
        owner.insert(f.to_string(), label);
    }
    let out: Vec<ArmReach> = per_arm
        .iter()
        .map(|(a, r)| {
            let exclusive: Vec<(String, u64)> = r
                .iter()
                .filter(|f| owner.get(*f).map(String::as_str) == Some(*a))
                .map(|f| (f.clone(), bytes_of[f.as_str()]))
                .collect();
            ArmReach {
                arm: a.to_string(),
                exclusive_bytes: exclusive.iter().map(|(_, b)| b).sum(),
                exclusive_functions: exclusive,
                reached: r.iter().cloned().collect(),
                reached_functions: r.len(),
                reached_bytes: r.iter().map(|f| bytes_of[f.as_str()]).sum(),
            }
        })
        .collect();
    // Every code byte: the arm owning it directly; else dispatch code when
    // it sits in a dispatcher or between functions; else its function's owner.
    let rows_out = rows(
        tally_by(ledger, |i| match arm_of.get(&i.pc_start) {
            Some(a) => a.clone(),
            None => match owner_of(i.pc_start) {
                Some(f) if !dispatchers.contains(f) => owner[f].clone(),
                _ => NO_ARM.to_string(),
            },
        }),
        |k| k.clone(),
    );
    Ok((out, rows_out))
}

fn check_totals(report: &FeTraceBytesReport) -> Result<()> {
    for (name, table) in [
        ("classification", &report.by_classification),
        ("emitted function", &report.by_emitted_function),
        ("primary body", &report.by_primary_body),
        ("primary file", &report.by_primary_file),
        ("recv arm", &report.by_recv_arm),
        ("recv arm with helpers", &report.by_recv_arm_with_helpers),
    ] {
        let sum: u64 = table.iter().map(|r| r.bytes).sum();
        if sum != report.artifact_bytes {
            bail!(
                "{name} table sums to {sum}, artifact is {}",
                report.artifact_bytes
            );
        }
    }
    Ok(())
}

pub fn render_fe_trace_bytes(report: &FeTraceBytesReport, top: usize) -> String {
    let pct = |b: u64| 100.0 * b as f64 / report.artifact_bytes.max(1) as f64;
    let body_label = |key: &str| match report.bodies.get(key) {
        Some(info) => {
            let mut label = match (&info.file, info.first_line) {
                (Some(f), Some(l)) => format!(
                    "{}  [{}:{}-{}]",
                    info.name,
                    f.rsplit('/').next().unwrap_or(f),
                    l,
                    info.last_line.unwrap_or(l)
                ),
                _ => info.name.clone(),
            };
            if info.body.starts_with("contract_recv$")
                && let Some(text) = &info.first_line_text
            {
                let short: String = text.chars().take(60).collect();
                label.push_str(&format!("  {short}"));
            }
            label
        }
        None => key.to_string(),
    };
    let mut out = format!(
        "contract {}: {} runtime bytes (blake3 {}), trace verified against the artifact{}\n{} instruction bytes, {} data bytes after the code\n",
        report.contract,
        report.artifact_bytes,
        &report.artifact_blake3[..16],
        if report.trace_code_hash_checked {
            ", code hash included"
        } else {
            ""
        },
        report.instruction_bytes,
        report.trailing_data_bytes,
    );
    let table =
        |out: &mut String, title: &str, rows: &[Row], n: usize, label: &dyn Fn(&str) -> String| {
            out.push_str(&format!("\n{title}\n"));
            for r in rows.iter().take(n) {
                out.push_str(&format!(
                    "{:>7} {:>5.1}% {:>6} instr  {}\n",
                    r.bytes,
                    pct(r.bytes),
                    r.instructions,
                    label(&r.key)
                ));
            }
            if rows.len() > n {
                let rest: u64 = rows[n..].iter().map(|r| r.bytes).sum();
                out.push_str(&format!(
                    "{rest:>7} {:>5.1}%        {} more rows\n",
                    pct(rest),
                    rows.len() - n
                ));
            }
        };
    let plain = |k: &str| k.to_string();
    table(
        &mut out,
        "Bytes by Fe attribution (classification / confidence / reason); adds up",
        &report.by_classification,
        usize::MAX,
        &plain,
    );
    table(
        &mut out,
        "Bytes by emitted function (final code layout, by position); adds up",
        &report.by_emitted_function,
        top,
        &plain,
    );
    let linked: u64 = report
        .by_emitted_function_linked
        .iter()
        .filter(|r| !r.key.starts_with('('))
        .map(|r| r.bytes)
        .sum();
    out.push_str(&format!(
        "  of which the trace links {linked} bytes ({:.1}%) to their function directly; {} bytes are function entries (an unlinked gap before the function, starting with a JUMPDEST, not reached from other functions), the rest sit inside a function's region by position\n",
        pct(linked),
        report.entry_bytes_by_position
    ));
    table(
        &mut out,
        "Bytes by recv arm (inlined region by dominance, or the arm's own function); adds up",
        &report.by_recv_arm,
        top,
        &body_label,
    );
    table(
        &mut out,
        "Bytes by recv arm, adding functions only that arm reaches (calls inferred from label pushes); adds up",
        &report.by_recv_arm_with_helpers,
        top,
        &body_label,
    );
    for arm in &report.arms {
        if let Some(note) = &arm.note {
            out.push_str(&format!("  note {}: {note}\n", body_label(&arm.arm)));
        }
    }
    table(
        &mut out,
        "Bytes by primary source body (each byte once); adds up",
        &report.by_primary_body,
        top,
        &body_label,
    );
    table(
        &mut out,
        "Bytes by primary source file; adds up",
        &report.by_primary_file,
        top,
        &plain,
    );
    table(
        &mut out,
        "Bytes with no single primary source, by emitted function",
        &report.no_primary_by_emitted_function,
        top,
        &plain,
    );
    table(
        &mut out,
        "Bytes with this body anywhere among their origins (overlapping, do not add)",
        &report.origin_anywhere_by_body,
        top,
        &body_label,
    );
    out.push_str(&format!(
        "\nRepeated EVM runs (>= {} bytes, non-overlapping selection): {} bytes in {} classes ({:.1}%)\n",
        report.runs_min_bytes,
        report.runs_selected_bytes,
        report.runs.len(),
        pct(report.runs_selected_bytes)
    ));
    if let Some(policy) = &report.runs_policy {
        out.push_str(&format!("policy {policy}\n"));
    }
    out.push_str(
        "covered  copies  bytes/copy  extra  label ports(varying)  constant ports(varying)  same-source instr  digest\n",
    );
    for run in report.runs.iter().take(top) {
        let mut wheres: BTreeMap<String, usize> = BTreeMap::new();
        for o in &run.occurrences {
            let f = o.emitted_function.as_deref().unwrap_or("?");
            let b = o
                .top_body
                .as_deref()
                .map(body_label)
                .unwrap_or_else(|| "no primary source".into());
            let arm = o
                .arm
                .as_deref()
                .map(|a| {
                    format!(
                        "{} ({}/{} bytes)",
                        body_label(a),
                        o.arm_bytes,
                        o.end - o.start
                    )
                })
                .unwrap_or_else(|| "-".into());
            *wheres.entry(format!("{f} / arm {arm} / {b}")).or_default() += 1;
        }
        out.push_str(&format!(
            "{:>7} {:>7} {:>11} {:>6} {:>12}({})  {:>15}({})  {:>7}/{:<7}  {}\n",
            run.covered_bytes,
            run.copies,
            run.bytes_per_copy,
            run.extra_bytes,
            run.ports,
            run.varying_ports,
            run.constant_ports,
            run.varying_constant_ports,
            run.same_source_instructions,
            run.instructions_per_copy,
            &run.digest[..12]
        ));
        for (w, n) in wheres.iter().take(6) {
            out.push_str(&format!("          {n} x {w}\n"));
        }
    }
    out.push_str("\nnote: extra bytes are a rough upper bound on what sharing could save, before call, return and parameter costs. A repeated run is structural correspondence under the stated key, not proof that the copies behave alike or that sharing them is safe.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_names_are_readable_and_total() {
        assert_eq!(
            body_name("contract_recv$StandAlone$standalone$fib_demo$fib_demo$contract$Fib$0$0"),
            "Fib recv 0 arm 0"
        );
        assert_eq!(
            body_name("func$Core$core$lib$abi$fn$store_word$"),
            "core::abi::store_word"
        );
        assert_eq!(
            body_name(
                "func$Std$std$lib$abi$sol$fn$decode_u32_word$impl_trait$Core$core$lib$abi$trait$Abi$args$adt$Std$std$lib$abi$sol$struct$Sol$assoc$$adt$Std$std$lib$abi$sol$struct$Sol"
            ),
            "std::abi::sol::decode_u32_word (impl Abi for Sol)"
        );
        assert_eq!(
            body_name(
                "func$Core$core$lib$num$fn$downcast$impl_trait$Core$core$lib$num$trait$IntDowncast$args$param$Core$core$lib$num$impl trait$impl_trait$0$param$Core$core$lib$num$impl trait$impl_trait$1$assoc$$param$Core$core$lib$num$impl trait$impl_trait$0"
            ),
            "core::num::downcast (impl IntDowncast for T)"
        );
        assert_eq!(body_name("something$else"), "something$else");
        assert_eq!(body_name("func$fn"), "func$fn");
    }

    fn inst(pc_start: u32, pc_end: u32, f: Option<&str>) -> LedgerInstruction {
        LedgerInstruction {
            pc_start,
            pc_end,
            mnemonic: String::new(),
            immediate: None,
            emitted_function: f.map(str::to_string),
            postopt_block: None,
            classification: "unmapped".into(),
            classification_reason: None,
            confidence: "unmapped".into(),
            primary_source: None,
            all_origins: Vec::new(),
        }
    }

    fn ledger(instructions: Vec<LedgerInstruction>) -> ByteLedger {
        ByteLedger {
            contract: "C".into(),
            code_object: "code".into(),
            code_hash: None,
            code_len: instructions.last().unwrap().pc_end,
            instructions,
            source_spans: BTreeMap::new(),
            source_files: BTreeMap::new(),
            cfgs: BTreeMap::new(),
        }
    }

    #[test]
    fn an_unlinked_entry_joins_its_function_but_a_shared_tail_does_not() {
        // root: PUSH2 0x0005, JUMP, STOP | gap: JUMPDEST | f: PUSH0, STOP
        let code = [0x61, 0x00, 0x05, 0x56, 0x00, 0x5b, 0x5f, 0x00];
        let l = ledger(vec![
            inst(0, 3, Some("root")),
            inst(3, 4, Some("root")),
            inst(4, 5, Some("root")),
            inst(5, 6, None),
            inst(6, 7, Some("f")),
            inst(7, 8, Some("f")),
        ]);
        let m = emitted_function_manifest(&l, &code, None);
        let spans: Vec<_> = m
            .regions
            .iter()
            .map(|r| (r.kind.as_str(), r.name.as_str(), r.start, r.end))
            .collect();
        assert_eq!(
            spans,
            vec![("function", "root", 0, 5), ("function", "f", 5, 8)]
        );

        // gap: JUMPDEST, JUMPDEST where root also pushes the second one: a
        // shared target, so the gap stays between functions.
        let code = [0x61, 0x00, 0x06, 0x56, 0x00, 0x5b, 0x5b, 0x5f, 0x00];
        let l = ledger(vec![
            inst(0, 3, Some("root")),
            inst(3, 4, Some("root")),
            inst(4, 5, Some("root")),
            inst(5, 6, None),
            inst(6, 7, None),
            inst(7, 8, Some("f")),
            inst(8, 9, Some("f")),
        ]);
        let m = emitted_function_manifest(&l, &code, None);
        let kinds: Vec<_> = m
            .regions
            .iter()
            .map(|r| (r.kind.as_str(), r.start, r.end))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("function", 0, 5),
                ("unattributed", 5, 7),
                ("function", 7, 9)
            ]
        );
    }
}
