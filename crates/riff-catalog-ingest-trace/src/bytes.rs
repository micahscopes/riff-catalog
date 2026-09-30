//! Byte ledger: every emitted byte of one EVM code object, joined to the
//! compiler's own attribution.
//!
//! Two Fe outputs go in, and nothing is re-derived that Fe already decided:
//!
//! - the trace bundle from `fe dev trace emit` supplies the instruction
//!   extents (`instruction_extent`), opcodes (`opcode`), the code objects
//!   (`code_object`), source spans (`source_span`, `source_file`), and the
//!   backend lineage `bytecode.pc -> evm.vcode.inst -> sonatina.evm.prepared.inst
//!   -> sonatina.postopt.inst` with the post-optimization blocks and CFG
//!   (`instruction_block`, `block`, `cfg_edge`, `function`);
//! - the attribution details from `fe dev debug emit --format ethdebug
//!   --attribution-details` supply, per instruction, Fe's own attribution
//!   decision under its `PrimarySourceV1` policy: the classification, the
//!   single primary source when there is one, and every origin reachable from
//!   the instruction. This reader does not re-walk the origin graph to second
//!   guess that policy.
//!
//! The emitted function of a pc is the Sonatina function whose `vcode`
//! instruction emitted it (the function in final code layout, so a helper
//! inlined into a dispatcher counts under the dispatcher here). Where the
//! trace has no such edge, the emitted function is `None`, never guessed.
//!
//! Aggregation helpers keep two views apart: bytes by primary source (each
//! byte counted once) and bytes by "origin anywhere" (a byte counts toward
//! every source body among its origins, so those totals overlap and must not
//! be added).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::BufRead;

use serde::{Deserialize, Serialize};

/// Attribution details schema this reader understands.
pub const ATTRIBUTION_DETAILS_SCHEMA: &str = "fe-ethdebug-attribution-details-v1";
/// Trace bundle schema versions whose fact kinds this reader understands.
/// Version 2 added `attribution_gap`; the kinds read here are unchanged.
pub const SUPPORTED_LEDGER_TRACE_SCHEMAS: [u64; 2] = [1, 2];

const SEP: char = '\u{1f}';

#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    #[error("trace line {line}: {message}")]
    Trace { line: usize, message: String },
    #[error("attribution details: {0}")]
    Details(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Inconsistent(String),
}

/// Canonical text form of an origin key, the same `kind\x1fowner\x1flocal`
/// spelling Fe uses in its attribution details.
pub fn key_text(kind: &str, owner: &str, local: &str) -> String {
    format!("{kind}{SEP}{owner}{SEP}{local}")
}

/// Split a canonical key into (kind, owner, local).
pub fn key_parts(key: &str) -> Option<(&str, &str, &str)> {
    let mut parts = key.splitn(3, SEP);
    Some((parts.next()?, parts.next()?, parts.next()?))
}

#[derive(Deserialize)]
pub(crate) struct WireKey {
    kind: String,
    owner_key: String,
    local_key: String,
}

impl WireKey {
    pub(crate) fn text(&self) -> String {
        key_text(&self.kind, &self.owner_key, &self.local_key)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LedgerSpan {
    pub file: String,
    pub start_byte: u32,
    pub end_byte: u32,
    pub start_line: u32,
    pub end_line: u32,
}

/// One emitted instruction.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LedgerInstruction {
    pub pc_start: u32,
    pub pc_end: u32,
    pub mnemonic: String,
    pub immediate: Option<String>,
    /// Sonatina function whose vcode emitted this pc, when the trace says so.
    pub emitted_function: Option<String>,
    /// Post-optimization Sonatina block reached through the backend lineage.
    pub postopt_block: Option<String>,
    pub classification: String,
    pub classification_reason: Option<String>,
    pub confidence: String,
    pub primary_source: Option<String>,
    pub all_origins: Vec<String>,
}

impl LedgerInstruction {
    pub fn bytes(&self) -> u64 {
        u64::from(self.pc_end - self.pc_start)
    }
}

/// Outcome of [`ByteLedger::verify_artifact`].
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactCheck {
    /// Bytes covered by instruction extents (`0..instruction_bytes`).
    pub instruction_bytes: u32,
    /// Bytes after the last instruction, proven part of the code object by
    /// the code hash but not described as instructions by the trace.
    pub trailing_bytes: u32,
    pub code_hash_checked: bool,
}

/// Post-optimization CFG of one emitted function, for region analyses.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FunctionCfg {
    /// Blocks in ordinal order; the first is the entry.
    pub blocks: Vec<String>,
    pub edges: Vec<(String, String)>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ByteLedger {
    pub contract: String,
    pub code_object: String,
    pub code_hash: Option<String>,
    /// End of the last instruction extent in this code object.
    pub code_len: u32,
    pub instructions: Vec<LedgerInstruction>,
    /// Spans of every origin referenced by an instruction, keyed by origin.
    pub source_spans: BTreeMap<String, LedgerSpan>,
    /// Source file key to display path (the file URI).
    pub source_files: BTreeMap<String, String>,
    /// Emitted function name to its post-optimization CFG.
    pub cfgs: BTreeMap<String, FunctionCfg>,
}

#[derive(Deserialize)]
struct DetailsFile {
    schema_version: String,
    instruction_origin_index: Vec<DetailsRow>,
}

/// One instruction of Fe's attribution details file.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DetailsRow {
    pub instruction_key: String,
    pub code_object: Option<String>,
    pub pc_start: u32,
    pub pc_end: u32,
    pub primary_source: Option<String>,
    pub all_origins: Vec<String>,
    pub classification: String,
    pub classification_reason: Option<String>,
    pub confidence: String,
}

impl DetailsRow {
    /// No source link at all: neither one exact source nor generated code
    /// tied to the source it was made for.
    pub fn has_no_source(&self) -> bool {
        !(self.classification == "source_mapped"
            || self.classification_reason.as_deref() == Some("SyntheticFor"))
    }
}

/// Read Fe's attribution details and keep the rows of `contract`'s runtime
/// code object (its key ends in `runtime` and names `:contract:<contract>:`).
pub fn read_runtime_details(
    attribution_details_json: &str,
    contract: &str,
) -> Result<Vec<DetailsRow>, LedgerError> {
    let details: DetailsFile = serde_json::from_str(attribution_details_json)
        .map_err(|e| LedgerError::Details(e.to_string()))?;
    if details.schema_version != ATTRIBUTION_DETAILS_SCHEMA {
        return Err(LedgerError::Details(format!(
            "unsupported schema {} (expected {ATTRIBUTION_DETAILS_SCHEMA})",
            details.schema_version
        )));
    }
    let marker = format!(":contract:{contract}:");
    let mut rows: Vec<DetailsRow> = details
        .instruction_origin_index
        .into_iter()
        .filter(|r| {
            r.code_object
                .as_deref()
                .is_some_and(|c| c.ends_with("runtime") && c.contains(&marker))
        })
        .collect();
    rows.sort_by_key(|r| r.pc_start);
    Ok(rows)
}

/// Which code object to read: the runtime section of this contract.
#[derive(Clone, Debug)]
pub struct LedgerSelector {
    pub contract: String,
}

#[derive(Default)]
struct Collected {
    code_objects: Vec<(String, String, Option<String>, Option<String>)>,
    extents: HashMap<String, (String, u32, u32)>,
    opcodes: HashMap<String, (String, Option<String>)>,
    edges: HashMap<String, Vec<String>>,
    postopt_block_of: HashMap<String, String>,
    postopt_blocks: Vec<(String, String, u32)>,
    postopt_cfg: Vec<(String, String, String)>,
    function_names: HashMap<String, String>,
    spans: HashMap<String, LedgerSpan>,
    files: BTreeMap<String, String>,
}

fn field<'a>(
    value: &'a serde_json::Value,
    name: &str,
    line: usize,
) -> Result<&'a serde_json::Value, LedgerError> {
    value.get(name).ok_or_else(|| LedgerError::Trace {
        line,
        message: format!("missing field `{name}`"),
    })
}

fn key_of(value: &serde_json::Value, name: &str, line: usize) -> Result<String, LedgerError> {
    let raw = field(value, name, line)?;
    let key: WireKey = serde_json::from_value(raw.clone()).map_err(|e| LedgerError::Trace {
        line,
        message: format!("field `{name}`: {e}"),
    })?;
    Ok(key.text())
}

fn u32_of(value: &serde_json::Value, name: &str, line: usize) -> Result<u32, LedgerError> {
    field(value, name, line)?
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| LedgerError::Trace {
            line,
            message: format!("field `{name}` is not a u32"),
        })
}

/// Fact kinds this reader uses. Every other kind is skipped without parsing.
const WANTED: [&str; 10] = [
    "code_object",
    "instruction_extent",
    "opcode",
    "origin_edge",
    "instruction_block",
    "block",
    "cfg_edge",
    "function",
    "source_span",
    "source_file",
];

pub(crate) fn fact_type(line: &str) -> Option<&str> {
    let at = line.find("\"type\":")? + "\"type\":".len();
    let rest = line[at..].trim_start().strip_prefix('"')?;
    Some(&rest[..rest.find('"')?])
}

fn collect(trace: impl BufRead) -> Result<Collected, LedgerError> {
    let mut c = Collected::default();
    for (index, raw) in trace.lines().enumerate() {
        let line = index + 1;
        let raw = raw?;
        let text = raw.trim();
        if text.is_empty() {
            continue;
        }
        if text.starts_with("{\"record\":\"metadata\"") || text.contains("\"record\": \"metadata\"")
        {
            let value: serde_json::Value =
                serde_json::from_str(text).map_err(|e| LedgerError::Trace {
                    line,
                    message: e.to_string(),
                })?;
            if let Some(found) = value.get("schema_version").and_then(|v| v.as_u64())
                && !SUPPORTED_LEDGER_TRACE_SCHEMAS.contains(&found)
            {
                return Err(LedgerError::Trace {
                    line,
                    message: format!(
                        "unsupported trace schema version {found} (supported: {SUPPORTED_LEDGER_TRACE_SCHEMAS:?})"
                    ),
                });
            }
            continue;
        }
        let Some(kind) = fact_type(text) else {
            continue;
        };
        if !WANTED.contains(&kind) {
            continue;
        }
        // Origin edges are the bulk of a large trace; keep only backend lineage.
        if kind == "origin_edge"
            && !(text.contains("\"from\":{\"kind\":\"bytecode.pc\"")
                || text.contains("\"from\":{\"kind\":\"evm.vcode.inst\"")
                || text.contains("\"from\":{\"kind\":\"sonatina.evm.prepared.inst\""))
        {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| LedgerError::Trace {
            line,
            message: e.to_string(),
        })?;
        match kind {
            "code_object" => {
                let code_object = key_of(&v, "code_object", line)?;
                let object_kind = field(&v, "kind", line)?
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let owner = v
                    .get("owner_function_or_contract")
                    .filter(|o| !o.is_null())
                    .map(|_| key_of(&v, "owner_function_or_contract", line))
                    .transpose()?;
                let hash = v
                    .get("code_hash")
                    .and_then(|h| h.as_str())
                    .map(str::to_string);
                c.code_objects.push((code_object, object_kind, owner, hash));
            }
            "instruction_extent" => {
                let instruction = key_of(&v, "instruction", line)?;
                let code_object = key_of(&v, "code_object", line)?;
                let range = field(&v, "pc_range", line)?;
                c.extents.insert(
                    instruction,
                    (
                        code_object,
                        u32_of(range, "start", line)?,
                        u32_of(range, "end", line)?,
                    ),
                );
            }
            "opcode" => {
                let pc = key_of(&v, "pc", line)?;
                let opcode = field(&v, "opcode", line)?
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                let immediate = v
                    .get("immediate")
                    .and_then(|i| i.as_str())
                    .map(str::to_string);
                c.opcodes.insert(pc, (opcode, immediate));
            }
            "origin_edge" => {
                let from = key_of(&v, "from", line)?;
                let to = key_of(&v, "to", line)?;
                c.edges.entry(from).or_default().push(to);
            }
            "instruction_block" => {
                if field(&v, "phase", line)?.as_str() == Some("sonatina_post_opt") {
                    c.postopt_block_of
                        .insert(key_of(&v, "instruction", line)?, key_of(&v, "block", line)?);
                }
            }
            "block" => {
                if field(&v, "phase", line)?.as_str() == Some("sonatina_post_opt") {
                    c.postopt_blocks.push((
                        key_of(&v, "function", line)?,
                        key_of(&v, "block", line)?,
                        u32_of(&v, "ordinal", line)?,
                    ));
                }
            }
            "cfg_edge" => {
                let function = key_of(&v, "function", line)?;
                if function.starts_with("sonatina.postopt.function") {
                    c.postopt_cfg.push((
                        function,
                        key_of(&v, "from_block", line)?,
                        key_of(&v, "to_block", line)?,
                    ));
                }
            }
            "function" => {
                let function = key_of(&v, "function", line)?;
                if function.starts_with("sonatina.postopt.function") {
                    let name = field(&v, "name", line)?
                        .as_str()
                        .unwrap_or_default()
                        .to_string();
                    c.function_names.insert(function, name);
                }
            }
            "source_span" => {
                let origin = key_of(&v, "origin", line)?;
                c.spans.insert(
                    origin,
                    LedgerSpan {
                        file: key_of(&v, "file", line)?,
                        start_byte: u32_of(&v, "start_byte", line)?,
                        end_byte: u32_of(&v, "end_byte", line)?,
                        start_line: u32_of(&v, "start_line", line)?,
                        end_line: u32_of(&v, "end_line", line)?,
                    },
                );
            }
            "source_file" => {
                let file = key_of(&v, "file_key", line)?;
                let uri = field(&v, "uri", line)?
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                c.files.insert(file, uri);
            }
            _ => {}
        }
    }
    Ok(c)
}

/// Sonatina function key (`sonatina.postopt.function`) for a lineage node
/// whose local key starts with `function:FuncRef(n):`.
fn postopt_function_of(node: &str) -> Option<String> {
    let (_, owner, local) = key_parts(node)?;
    let end = local.find("):")? + 1;
    Some(key_text("sonatina.postopt.function", owner, &local[..end]))
}

impl ByteLedger {
    /// Build the ledger for `selector.contract`'s runtime code object.
    pub fn read(
        trace: impl BufRead,
        attribution_details_json: &str,
        selector: &LedgerSelector,
    ) -> Result<Self, LedgerError> {
        let c = collect(trace)?;
        let details: DetailsFile = serde_json::from_str(attribution_details_json)
            .map_err(|e| LedgerError::Details(e.to_string()))?;
        if details.schema_version != ATTRIBUTION_DETAILS_SCHEMA {
            return Err(LedgerError::Details(format!(
                "unsupported schema {} (expected {ATTRIBUTION_DETAILS_SCHEMA})",
                details.schema_version
            )));
        }
        let matches: Vec<_> = c
            .code_objects
            .iter()
            .filter(|(_, kind, owner, _)| {
                kind == "evm_runtime_bytecode"
                    && owner
                        .as_deref()
                        .and_then(key_parts)
                        .is_some_and(|(_, _, local)| local == selector.contract)
            })
            .collect();
        let [(code_object, _, _, code_hash)] = matches.as_slice() else {
            return Err(LedgerError::Inconsistent(format!(
                "expected exactly one runtime code object for contract `{}`, found {}",
                selector.contract,
                matches.len()
            )));
        };

        let lineage_block = |pc: &str| -> (Option<String>, Option<String>) {
            // bytecode.pc -> evm.vcode.inst -> prepared -> postopt inst -> block
            let vcode: Vec<&String> = c
                .edges
                .get(pc)
                .into_iter()
                .flatten()
                .filter(|to| to.starts_with("evm.vcode.inst"))
                .collect();
            let function = vcode
                .iter()
                .filter_map(|v| postopt_function_of(v))
                .collect::<BTreeSet<_>>();
            let function = (function.len() == 1)
                .then(|| function.into_iter().next())
                .flatten()
                .map(|f| c.function_names.get(&f).cloned().unwrap_or(f));
            let mut blocks = BTreeSet::new();
            for v in &vcode {
                for prepared in c.edges.get(*v).into_iter().flatten() {
                    for postopt in c.edges.get(prepared).into_iter().flatten() {
                        if let Some(block) = c.postopt_block_of.get(postopt) {
                            blocks.insert(block.clone());
                        }
                    }
                }
            }
            let block = (blocks.len() == 1)
                .then(|| blocks.into_iter().next())
                .flatten();
            (function, block)
        };

        let mut instructions = Vec::new();
        let mut referenced = BTreeSet::new();
        for row in details.instruction_origin_index {
            if row.code_object.as_deref() != Some(code_object.as_str()) {
                continue;
            }
            let (mnemonic, immediate) = c
                .opcodes
                .get(&row.instruction_key)
                .cloned()
                .unwrap_or_else(|| (String::new(), None));
            if let Some((object, start, end)) = c.extents.get(&row.instruction_key)
                && (object != code_object || *start != row.pc_start || *end != row.pc_end)
            {
                return Err(LedgerError::Inconsistent(format!(
                    "attribution details and trace disagree on the extent of {}",
                    row.instruction_key
                )));
            }
            let (emitted_function, postopt_block) = lineage_block(&row.instruction_key);
            referenced.extend(row.primary_source.iter().cloned());
            referenced.extend(row.all_origins.iter().cloned());
            instructions.push(LedgerInstruction {
                pc_start: row.pc_start,
                pc_end: row.pc_end,
                mnemonic,
                immediate,
                emitted_function,
                postopt_block,
                classification: row.classification,
                classification_reason: row.classification_reason,
                confidence: row.confidence,
                primary_source: row.primary_source,
                all_origins: row.all_origins,
            });
        }
        instructions.sort_by_key(|i| i.pc_start);
        for pair in instructions.windows(2) {
            if pair[1].pc_start < pair[0].pc_end {
                return Err(LedgerError::Inconsistent(format!(
                    "instruction extents overlap at pc {}",
                    pair[1].pc_start
                )));
            }
        }
        let code_len = instructions.last().map_or(0, |i| i.pc_end);

        let source_spans: BTreeMap<String, LedgerSpan> = referenced
            .into_iter()
            .filter_map(|o| c.spans.get(&o).map(|s| (o, s.clone())))
            .collect();

        // Post-optimization CFGs, named by emitted function.
        let mut cfgs: BTreeMap<String, FunctionCfg> = BTreeMap::new();
        let name_of = |f: &String| {
            c.function_names
                .get(f)
                .cloned()
                .unwrap_or_else(|| f.clone())
        };
        let mut ordered: Vec<_> = c.postopt_blocks.iter().collect();
        ordered.sort_by(|a, b| (&a.0, a.2).cmp(&(&b.0, b.2)));
        for (function, block, _) in ordered {
            cfgs.entry(name_of(function))
                .or_default()
                .blocks
                .push(block.clone());
        }
        for (function, from, to) in &c.postopt_cfg {
            cfgs.entry(name_of(function))
                .or_default()
                .edges
                .push((from.clone(), to.clone()));
        }

        Ok(Self {
            contract: selector.contract.clone(),
            code_object: code_object.clone(),
            code_hash: code_hash.clone(),
            code_len,
            instructions,
            source_spans,
            source_files: c.files,
            cfgs,
        })
    }

    /// Check the ledger against the real artifact bytes: the trace's blake3
    /// code hash (when present) must be the hash of the whole artifact, the
    /// instruction extents must tile `0..code_len` without gaps, and every
    /// PUSH immediate must equal the artifact's bytes.
    ///
    /// Bytes after the last instruction are accepted only when the code hash
    /// proves they belong to the traced code object: they are data the
    /// backend placed after the code (constant regions), which the trace does
    /// not describe as instructions. They are reported, never attributed.
    pub fn verify_artifact(&self, code: &[u8]) -> Result<ArtifactCheck, LedgerError> {
        if code.len() < self.code_len as usize {
            return Err(LedgerError::Inconsistent(format!(
                "artifact is {} bytes, shorter than the {} instruction bytes in the trace",
                code.len(),
                self.code_len
            )));
        }
        let code_hash_checked = match &self.code_hash {
            Some(hash) => {
                let actual = format!("blake3:{}", blake3_hex(code));
                if &actual != hash {
                    return Err(LedgerError::Inconsistent(format!(
                        "artifact hash {actual} differs from trace code hash {hash}"
                    )));
                }
                true
            }
            None if code.len() != self.code_len as usize => {
                return Err(LedgerError::Inconsistent(format!(
                    "artifact is {} bytes, trace covers {} bytes, and the trace has no code hash to show the rest belongs to it",
                    code.len(),
                    self.code_len
                )));
            }
            None => false,
        };
        let mut expected = 0u32;
        for inst in &self.instructions {
            if inst.pc_start != expected {
                return Err(LedgerError::Inconsistent(format!(
                    "bytes {expected}..{} have no instruction extent",
                    inst.pc_start
                )));
            }
            if let Some(imm) = &inst.immediate {
                let hex: String = code[inst.pc_start as usize + 1..inst.pc_end as usize]
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                if imm.trim_start_matches("0x") != hex {
                    return Err(LedgerError::Inconsistent(format!(
                        "immediate at pc {} is {imm} in the trace, 0x{hex} in the artifact",
                        inst.pc_start
                    )));
                }
            }
            expected = inst.pc_end;
        }
        Ok(ArtifactCheck {
            instruction_bytes: self.code_len,
            trailing_bytes: code.len() as u32 - self.code_len,
            code_hash_checked,
        })
    }

    /// Maximal pc ranges of consecutive instructions with the same emitted
    /// function (`None` ranges included), in pc order.
    pub fn function_ranges(&self) -> Vec<(Option<&str>, u32, u32)> {
        let mut out: Vec<(Option<&str>, u32, u32)> = Vec::new();
        for inst in &self.instructions {
            let name = inst.emitted_function.as_deref();
            match out.last_mut() {
                Some(last) if last.0 == name && last.2 == inst.pc_start => last.2 = inst.pc_end,
                _ => out.push((name, inst.pc_start, inst.pc_end)),
            }
        }
        out
    }
}

/// Immediate dominators of a post-optimization CFG, entry = `blocks[0]`
/// (Cooper, Harvey and Kennedy's iterative algorithm over reverse postorder).
/// The entry maps to itself; blocks unreachable from the entry are absent.
pub fn immediate_dominators(cfg: &FunctionCfg) -> HashMap<String, String> {
    let Some(entry) = cfg.blocks.first() else {
        return HashMap::new();
    };
    let mut succ: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut pred: HashMap<&str, Vec<&str>> = HashMap::new();
    for (from, to) in &cfg.edges {
        succ.entry(from).or_default().push(to);
        pred.entry(to).or_default().push(from);
    }
    // Iterative DFS postorder from the entry.
    let mut order: Vec<&str> = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut stack: Vec<(&str, usize)> = vec![(entry.as_str(), 0)];
    seen.insert(entry);
    while let Some((node, next)) = stack.pop() {
        let children = succ.get(node).map(Vec::as_slice).unwrap_or(&[]);
        if next < children.len() {
            stack.push((node, next + 1));
            let child = children[next];
            if seen.insert(child) {
                stack.push((child, 0));
            }
        } else {
            order.push(node);
        }
    }
    let rpo: Vec<&str> = order.iter().rev().copied().collect();
    let index: HashMap<&str, usize> = rpo.iter().enumerate().map(|(i, b)| (*b, i)).collect();
    let mut idom: Vec<Option<usize>> = vec![None; rpo.len()];
    idom[0] = Some(0);
    let intersect = |idom: &[Option<usize>], mut a: usize, mut b: usize| {
        while a != b {
            while a > b {
                a = idom[a].expect("processed");
            }
            while b > a {
                b = idom[b].expect("processed");
            }
        }
        a
    };
    let mut changed = true;
    while changed {
        changed = false;
        for (i, block) in rpo.iter().enumerate().skip(1) {
            let mut new: Option<usize> = None;
            for p in pred.get(block).into_iter().flatten() {
                let Some(&pi) = index.get(p) else { continue };
                if idom[pi].is_none() {
                    continue;
                }
                new = Some(match new {
                    None => pi,
                    Some(n) => intersect(&idom, pi, n),
                });
            }
            if new.is_some() && idom[i] != new {
                idom[i] = new;
                changed = true;
            }
        }
    }
    rpo.iter()
        .enumerate()
        .filter_map(|(i, b)| idom[i].map(|d| (b.to_string(), rpo[d].to_string())))
        .collect()
}

/// Whether `a` dominates `b` under `idom` (every block dominates itself).
pub fn dominates(idom: &HashMap<String, String>, a: &str, b: &str) -> bool {
    let mut cur = b;
    loop {
        if cur == a {
            return true;
        }
        match idom.get(cur) {
            Some(next) if next != cur => cur = next,
            _ => return false,
        }
    }
}

/// The HIR body that owns an origin (`hir-body:<id>` owner keys), if any.
pub fn source_body(origin: &str) -> Option<&str> {
    let (_, owner, _) = key_parts(origin)?;
    owner.strip_prefix("hir-body:")
}

fn blake3_hex(bytes: &[u8]) -> String {
    // Kept local so this crate stays free of hashing-engine dependencies; the
    // blake3 crate itself is small and only used for this check.
    blake3::hash(bytes).to_hex().to_string()
}

/// Byte totals over a key, with instruction counts.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tally {
    pub bytes: u64,
    pub instructions: u64,
}

impl Tally {
    /// Count one instruction of `bytes` bytes.
    pub fn add_bytes(&mut self, bytes: u64) {
        self.bytes += bytes;
        self.instructions += 1;
    }

    fn add(&mut self, inst: &LedgerInstruction) {
        self.add_bytes(inst.bytes());
    }
}

/// Group bytes by a key computed per instruction. Each byte lands in exactly
/// one group, so the totals add up to the ledger's code length.
pub fn tally_by<K: Ord>(
    ledger: &ByteLedger,
    mut key: impl FnMut(&LedgerInstruction) -> K,
) -> BTreeMap<K, Tally> {
    let mut out: BTreeMap<K, Tally> = BTreeMap::new();
    for inst in &ledger.instructions {
        out.entry(key(inst)).or_default().add(inst);
    }
    out
}

/// "Bytes with this source body anywhere among their origins". A byte counts
/// once per distinct body in its origin set, so these totals overlap.
pub fn tally_origin_anywhere(ledger: &ByteLedger) -> BTreeMap<String, Tally> {
    let mut out: BTreeMap<String, Tally> = BTreeMap::new();
    for inst in &ledger.instructions {
        let bodies: BTreeSet<&str> = inst
            .all_origins
            .iter()
            .chain(inst.primary_source.iter())
            .filter_map(|o| source_body(o))
            .collect();
        for body in bodies {
            out.entry(body.to_string()).or_default().add(inst);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: &str = "package:demo:contract:C:section:runtime";
    const SONA: &str = "package:demo:sonatina";

    fn k(kind: &str, owner: &str, local: &str) -> String {
        format!(r#"{{"kind":"{kind}","owner_key":"{owner}","local_key":"{local}"}}"#)
    }

    fn trace() -> String {
        let pc = |n: u32| k("bytecode.pc", OWNER, &format!("pc:{n}"));
        let object = k("code.object", OWNER, "runtime");
        let vcode = |f: u32, n: u32| {
            k(
                "evm.vcode.inst",
                SONA,
                &format!("function:FuncRef({f}):vcode_inst:VCodeInst({n})"),
            )
        };
        let prepared = |f: u32, n: u32| {
            k(
                "sonatina.evm.prepared.inst",
                SONA,
                &format!("function:FuncRef({f}):inst:InstId({n})"),
            )
        };
        let postopt = |f: u32, n: u32| {
            k(
                "sonatina.postopt.inst",
                SONA,
                &format!("function:FuncRef({f}):inst:InstId({n})"),
            )
        };
        let block = |f: u32, b: u32| {
            k(
                "sonatina.postopt.block",
                SONA,
                &format!("function:FuncRef({f}):block:block{b}"),
            )
        };
        let func = |f: u32| {
            k(
                "sonatina.postopt.function",
                SONA,
                &format!("function:FuncRef({f})"),
            )
        };
        let mut lines = vec![
            r#"{"record":"metadata","schema_version":2,"input_path":"demo"}"#.to_string(),
            format!(
                r#"{{"record":"fact","type":"code_object","code_object":{object},"kind":"evm_runtime_bytecode","owner_function_or_contract":{},"target":"evm/sonatina","code_hash":null}}"#,
                k("bytecode.contract", "package:demo", "C")
            ),
            format!(
                r#"{{"record":"fact","type":"function","function":{},"name":"root","source_origin":null,"code_object":null}}"#,
                func(0)
            ),
            format!(
                r#"{{"record":"fact","type":"function","function":{},"name":"helper","source_origin":null,"code_object":null}}"#,
                func(1)
            ),
            format!(
                r#"{{"record":"fact","type":"block","block":{},"function":{},"phase":"sonatina_post_opt","ordinal":0,"name":null}}"#,
                block(0, 0),
                func(0)
            ),
            format!(
                r#"{{"record":"fact","type":"cfg_edge","function":{},"from_block":{},"to_block":{},"kind":"jump","condition_origin":null}}"#,
                func(0),
                block(0, 0),
                block(0, 1)
            ),
            r#"{"record":"fact","type":"storage","anything":1}"#.to_string(),
        ];
        // pc 0: PUSH1 0x80 (root, block0), pc 2: MSTORE (helper), pc 3: STOP (no lineage)
        let extents = [
            (0u32, 2u32, "PUSH1", Some("0x80")),
            (2, 3, "MSTORE", None),
            (3, 4, "STOP", None),
        ];
        for (start, end, op, imm) in extents {
            lines.push(format!(r#"{{"record":"fact","type":"instruction_extent","instruction":{},"code_object":{object},"pc_range":{{"start":{start},"end":{end}}},"byte_len":{}}}"#, pc(start), end - start));
            let imm = imm.map_or("null".to_string(), |i| format!("\"{i}\""));
            lines.push(format!(r#"{{"record":"fact","type":"opcode","pc":{},"opcode":"{op}","immediate":{imm},"category":"other"}}"#, pc(start)));
        }
        for (pcn, f, n) in [(0u32, 0u32, 7u32), (2, 1, 3)] {
            lines.push(format!(r#"{{"record":"fact","type":"origin_edge","from":{},"to":{},"label":"emitted_from","introduced_by":"bytecode_emission"}}"#, pc(pcn), vcode(f, n)));
            lines.push(format!(r#"{{"record":"fact","type":"origin_edge","from":{},"to":{},"label":"lowered_from","introduced_by":"backend"}}"#, vcode(f, n), prepared(f, n)));
            lines.push(format!(r#"{{"record":"fact","type":"origin_edge","from":{},"to":{},"label":"lowered_from","introduced_by":"backend"}}"#, prepared(f, n), postopt(f, n)));
            lines.push(format!(r#"{{"record":"fact","type":"instruction_block","instruction":{},"block":{},"phase":"sonatina_post_opt"}}"#, postopt(f, n), block(f, 0)));
            lines.push(format!(r#"{{"record":"fact","type":"origin_edge","from":{},"to":{},"label":"lowered_from","introduced_by":"sonatina_post_opt"}}"#, postopt(f, n), k("runtime.stmt", &format!("runtime-instance:semantic:fn{f}"), "block:0:stmt:0")));
        }
        lines.push(format!(r#"{{"record":"fact","type":"source_span","origin":{},"file":{},"start_byte":0,"end_byte":4,"start_line":1,"start_column":1,"end_line":1,"end_column":5}}"#, k("hir.expr", "hir-body:body_a", "0"), k("source.file", "file:///demo.fe", "file:0")));
        lines.join("\n")
    }

    fn details() -> String {
        let pc = |n: u32| {
            key_text("bytecode.pc", OWNER, &format!("pc:{n}")).replace('\u{1f}', "\\u001f")
        };
        let object = key_text("code.object", OWNER, "runtime").replace('\u{1f}', "\\u001f");
        let expr = key_text("hir.expr", "hir-body:body_a", "0").replace('\u{1f}', "\\u001f");
        let other = key_text("hir.expr", "hir-body:body_b", "3").replace('\u{1f}', "\\u001f");
        format!(
            r#"{{"schema_version":"fe-ethdebug-attribution-details-v1","trace_hash":"x","ethdebug_schema_version":"y","ethdebug_artifact_hash":"z","instruction_origin_index":[
            {{"instruction_key":"{}","code_object":"{object}","pc_start":0,"pc_end":2,"primary_source":"{expr}","all_origins":["{expr}"],"classification":"source_mapped","classification_reason":null,"confidence":"high"}},
            {{"instruction_key":"{}","code_object":"{object}","pc_start":2,"pc_end":3,"primary_source":null,"all_origins":["{expr}","{other}"],"classification":"ambiguous","classification_reason":null,"confidence":"ambiguous"}},
            {{"instruction_key":"{}","code_object":"{object}","pc_start":3,"pc_end":4,"primary_source":null,"all_origins":[],"classification":"unmapped","classification_reason":"synthetic","confidence":"unmapped"}}
            ]}}"#,
            pc(0),
            pc(2),
            pc(3)
        )
    }

    fn ledger() -> ByteLedger {
        ByteLedger::read(
            trace().as_bytes(),
            &details(),
            &LedgerSelector {
                contract: "C".into(),
            },
        )
        .unwrap()
    }

    #[test]
    fn reads_every_byte_with_function_and_attribution() {
        let l = ledger();
        assert_eq!(l.code_len, 4);
        assert_eq!(l.instructions.len(), 3);
        assert_eq!(l.instructions[0].emitted_function.as_deref(), Some("root"));
        assert_eq!(
            l.instructions[1].emitted_function.as_deref(),
            Some("helper")
        );
        assert_eq!(
            l.instructions[2].emitted_function, None,
            "no lineage is not guessed"
        );
        assert!(
            l.instructions[0]
                .postopt_block
                .as_deref()
                .unwrap()
                .contains("block0")
        );
        assert_eq!(l.instructions[0].immediate.as_deref(), Some("0x80"));
        assert_eq!(l.source_spans.len(), 1, "only spans the trace carries");
        let cfg = &l.cfgs["root"];
        assert_eq!(cfg.blocks.len(), 1);
        assert_eq!(cfg.edges.len(), 1);
    }

    #[test]
    fn primary_totals_add_up_and_anywhere_totals_overlap() {
        let l = ledger();
        let by_primary = tally_by(&l, |i| {
            i.primary_source
                .as_deref()
                .and_then(source_body)
                .map(str::to_string)
        });
        let total: u64 = by_primary.values().map(|t| t.bytes).sum();
        assert_eq!(total, 4);
        assert_eq!(by_primary[&Some("body_a".to_string())].bytes, 2);
        let anywhere = tally_origin_anywhere(&l);
        assert_eq!(anywhere["body_a"].bytes, 3);
        assert_eq!(anywhere["body_b"].bytes, 1);
    }

    #[test]
    fn verifies_the_artifact_bytes() {
        let l = ledger();
        l.verify_artifact(&[0x60, 0x80, 0x52, 0x00]).unwrap();
        assert!(
            l.verify_artifact(&[0x60, 0x81, 0x52, 0x00]).is_err(),
            "immediate mismatch"
        );
        assert!(
            l.verify_artifact(&[0x60, 0x80, 0x52]).is_err(),
            "length mismatch"
        );
    }

    #[test]
    fn function_ranges_are_maximal_runs() {
        let l = ledger();
        let ranges = l.function_ranges();
        assert_eq!(
            ranges,
            vec![(Some("root"), 0, 2), (Some("helper"), 2, 3), (None, 3, 4)]
        );
    }

    #[test]
    fn rejects_unknown_schemas_and_missing_contracts() {
        let bad = trace().replace("\"schema_version\":2", "\"schema_version\":9");
        assert!(
            ByteLedger::read(
                bad.as_bytes(),
                &details(),
                &LedgerSelector {
                    contract: "C".into()
                }
            )
            .is_err()
        );
        assert!(
            ByteLedger::read(
                trace().as_bytes(),
                &details(),
                &LedgerSelector {
                    contract: "D".into()
                }
            )
            .is_err()
        );
        let details = details().replace("details-v1", "details-v9");
        assert!(
            ByteLedger::read(
                trace().as_bytes(),
                &details,
                &LedgerSelector {
                    contract: "C".into()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn dominators_of_a_diamond_with_a_loop() {
        let b = |n: &str| n.to_string();
        let cfg = FunctionCfg {
            blocks: vec![b("e"), b("l"), b("r"), b("j"), b("x"), b("dead")],
            edges: vec![
                (b("e"), b("l")),
                (b("e"), b("r")),
                (b("l"), b("j")),
                (b("r"), b("j")),
                (b("j"), b("x")),
                (b("x"), b("j")),
            ],
        };
        let idom = immediate_dominators(&cfg);
        assert_eq!(idom["e"], "e");
        assert_eq!(idom["l"], "e");
        assert_eq!(idom["r"], "e");
        assert_eq!(idom["j"], "e", "join of the diamond");
        assert_eq!(idom["x"], "j");
        assert!(!idom.contains_key("dead"));
        assert!(dominates(&idom, "j", "x"));
        assert!(!dominates(&idom, "l", "j"));
        assert!(dominates(&idom, "e", "x"));
    }
}
