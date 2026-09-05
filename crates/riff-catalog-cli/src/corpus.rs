//! The JSONL corpus: one file per ingested artifact plus claims/attestations
//! files. Every line is one self-contained record.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use riff_catalog_claims::{Attestation, Claim};
use riff_catalog_core::{Digest, Dimension, Graph, GraphKey, PolicyId};
use serde::{Deserialize, Serialize};

/// Cheaply decide whether a raw JSONL line is a `Graph` record without parsing
/// it. `Record` is internally tagged (`#[serde(tag = "record")]`) and written
/// compactly, so the tag is the first field on every line. We scan only a short
/// prefix, keeping this O(1) regardless of the graph payload's size (a single
/// graph line can be hundreds of MB). The tag is ASCII, so a byte-level search
/// is robust to any UTF-8 content further along the line.
fn line_is_graph_record(line: &str) -> bool {
    const PREFIX: usize = 64;
    const TAG: &[u8] = b"\"record\":\"graph\"";
    let head = &line.as_bytes()[..line.len().min(PREFIX)];
    head.windows(TAG.len()).any(|window| window == TAG)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "snake_case")]
pub enum Record {
    Artifact {
        artifact_id: String,
        owner: String,
        origin: String,
        pipeline: String,
        optimize: bool,
        /// The compiler version this artifact was produced with (e.g.
        /// "0.8.24+commit.e11b9ed9"). Provenance only — NEVER folded into a
        /// shape fingerprint, which stays compiler-version-independent so the
        /// same source rhymes across versions. Lets a query ask "same shape"
        /// and "same shape AND same compiler" as two separate questions.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        compiler: Option<String>,
        label: Option<String>,
    },
    Graph {
        artifact_id: String,
        unit: String,
        level: String,
        name: String,
        graph_key: GraphKey,
        graph: Graph,
    },
    Digest {
        artifact_id: String,
        owner: String,
        unit: String,
        level: String,
        name: String,
        mode: String,
        policy_id: PolicyId,
        digests: BTreeMap<Dimension, Digest>,
        node_count: usize,
    },
    Observation {
        artifact_id: String,
        owner: String,
        unit: String,
        level: String,
        name: String,
        schema: String,
        stage_kind: String,
        stage: String,
        ordinal: u64,
        function_graph_id: u64,
        duration_us: u64,
        metrics: BTreeMap<String, u64>,
        identity_address: String,
        shape_address: String,
    },
    Claim {
        claim: Claim,
        left_display: String,
        right_display: String,
    },
    Attestation {
        attestation: Attestation,
        subject_display: String,
    },
}

/// A digest row as used by every query command.
#[derive(Clone, Debug)]
#[allow(dead_code)] // unit/level/mode are part of the row schema even where unread
pub struct DigestRow {
    pub artifact_id: String,
    pub owner: String,
    pub unit: String,
    pub level: String,
    pub name: String,
    pub mode: String,
    pub policy_id: PolicyId,
    pub digests: BTreeMap<Dimension, Digest>,
}

/// A persisted graph plus the corpus coordinates that identify it.
#[derive(Clone, Debug)]
pub struct GraphRow {
    pub artifact_id: String,
    pub unit: String,
    pub name: String,
    pub graph_key: GraphKey,
    pub graph: Graph,
}

pub struct Corpus {
    pub dir: PathBuf,
}

impl Corpus {
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating corpus dir {}", dir.display()))?;
        Ok(Self {
            dir: dir.to_path_buf(),
        })
    }

    /// Append records (claims/attestations: genuinely accumulating logs).
    pub fn append(&self, file_stem: &str, records: &[Record]) -> Result<()> {
        self.write(file_stem, records, true)
    }

    /// Replace an artifact's records wholesale: re-ingesting the same
    /// artifact is idempotent instead of doubling every digest row and
    /// skewing bucket/overlap statistics (external review pass 2, P2).
    pub fn replace(&self, file_stem: &str, records: &[Record]) -> Result<()> {
        self.write(file_stem, records, false)
    }

    fn write(&self, file_stem: &str, records: &[Record], append: bool) -> Result<()> {
        let path = self.dir.join(format!("{file_stem}.jsonl"));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(append)
            .write(true)
            .truncate(!append)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        let mut file = BufWriter::new(file);
        for record in records {
            serde_json::to_writer(&mut file, record)?;
            file.write_all(b"\n")?;
        }
        file.flush()?;
        Ok(())
    }

    /// Load every record except `Graph`. Digest/claim/attestation queries never
    /// read the graph payloads, which dominate the corpus on disk (hundreds of
    /// MB), so skipping them before JSON parsing turns a multi-second full-corpus
    /// load into a cheap one. (Graph records are still persisted by `ingest`;
    /// add a graph-aware loader if a graph-level query ever needs them.)
    pub fn load_non_graph_records(&self) -> Result<Vec<Record>> {
        let mut records = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Ok(records);
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "jsonl") {
                continue;
            }
            let content = std::fs::read_to_string(&path)?;
            for (line_no, line) in content.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                if line_is_graph_record(line) {
                    continue;
                }
                let record: Record = serde_json::from_str(line)
                    .with_context(|| format!("parsing {}:{}", path.display(), line_no + 1))?;
                records.push(record);
            }
        }
        Ok(records)
    }

    pub fn digest_rows(&self, unit: &str, mode: &str) -> Result<Vec<DigestRow>> {
        self.digest_rows_filtered(Some(unit), Some(mode))
    }

    /// Stream graph records matching one lowering level and selector. Graph
    /// records can be very large, so this reads one JSONL record at a time.
    pub fn graph_rows(
        &self,
        level: &str,
        selector: &str,
        unit: Option<&str>,
        name: Option<&str>,
    ) -> Result<Vec<GraphRow>> {
        let mut rows = Vec::new();
        let level_marker = format!("\"level\":{}", serde_json::to_string(level)?);
        let unit_marker = unit
            .map(serde_json::to_string)
            .transpose()?
            .map(|unit| format!("\"unit\":{unit}"));
        let name_marker = name
            .map(serde_json::to_string)
            .transpose()?
            .map(|name| format!("\"name\":{name}"));
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Ok(rows);
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_none_or(|extension| extension != "jsonl")
            {
                continue;
            }
            let file = std::fs::File::open(&path)
                .with_context(|| format!("opening {}", path.display()))?;
            for (line_no, line) in BufReader::new(file).lines().enumerate() {
                let line =
                    line.with_context(|| format!("reading {}:{}", path.display(), line_no + 1))?;
                if line.trim().is_empty()
                    || !line_is_graph_record(&line)
                    || !line.contains(&level_marker)
                    || unit_marker
                        .as_ref()
                        .is_some_and(|marker| !line.contains(marker))
                    || name_marker
                        .as_ref()
                        .is_some_and(|marker| !line.contains(marker))
                {
                    continue;
                }
                let record: Record = serde_json::from_str(&line)
                    .with_context(|| format!("parsing {}:{}", path.display(), line_no + 1))?;
                let Record::Graph {
                    artifact_id,
                    unit: row_unit,
                    level: row_level,
                    name: row_name,
                    graph_key,
                    graph,
                } = record
                else {
                    continue;
                };
                if row_level != level
                    || unit.is_some_and(|unit| row_unit != unit)
                    || name.is_some_and(|name| row_name != name)
                {
                    continue;
                }
                let row = GraphRow {
                    artifact_id,
                    unit: row_unit,
                    name: row_name,
                    graph_key,
                    graph,
                };
                if matches_graph_selector(&row, selector) {
                    rows.push(row);
                }
            }
        }
        rows.sort_by(|left, right| {
            left.artifact_id
                .cmp(&right.artifact_id)
                .then_with(|| left.unit.cmp(&right.unit))
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.graph_key.cmp(&right.graph_key))
        });
        Ok(rows)
    }

    /// Digest rows with optional unit/mode filters (`None` = all). The
    /// unfiltered form feeds `root`, which commits to the whole corpus.
    pub fn digest_rows_filtered(
        &self,
        unit: Option<&str>,
        mode: Option<&str>,
    ) -> Result<Vec<DigestRow>> {
        Ok(self
            .load_non_graph_records()?
            .into_iter()
            .filter_map(|record| match record {
                Record::Digest {
                    artifact_id,
                    owner,
                    unit: row_unit,
                    level,
                    name,
                    mode: row_mode,
                    policy_id,
                    digests,
                    ..
                } if unit.is_none_or(|unit| row_unit == unit)
                    && mode.is_none_or(|mode| row_mode == mode) =>
                {
                    Some(DigestRow {
                        artifact_id,
                        owner,
                        unit: row_unit,
                        level,
                        name,
                        mode: row_mode,
                        policy_id,
                        digests,
                    })
                }
                _ => None,
            })
            .collect())
    }

    pub fn claims(&self) -> Result<Vec<Claim>> {
        self.load_non_graph_records()?
            .into_iter()
            .filter_map(|record| match record {
                Record::Claim { claim, .. } => Some(claim),
                _ => None,
            })
            .map(|claim| {
                // A reader that doesn't know a claim's schema must refuse it,
                // not strip the fields it doesn't understand: a conditional
                // claim read by an assumptions-blind binary would silently
                // merge unconditionally.
                if claim.schema_version > riff_catalog_claims::CLAIMS_SCHEMA_VERSION {
                    anyhow::bail!(
                        "claim {} has schema version {} — newer than this binary \
                         understands ({}); refusing to interpret it",
                        claim.claim_id().display_short(),
                        claim.schema_version,
                        riff_catalog_claims::CLAIMS_SCHEMA_VERSION
                    );
                }
                Ok(claim)
            })
            .collect()
    }

    pub fn attestations(&self) -> Result<Vec<Attestation>> {
        self.load_non_graph_records()?
            .into_iter()
            .filter_map(|record| match record {
                Record::Attestation { attestation, .. } => Some(attestation),
                _ => None,
            })
            .map(|attestation| {
                if attestation.schema_version > riff_catalog_claims::CLAIMS_SCHEMA_VERSION {
                    anyhow::bail!(
                        "attestation {} has schema version {} — newer than this binary \
                         understands ({}); refusing to interpret it",
                        attestation.attestation_id().display_short(),
                        attestation.schema_version,
                        riff_catalog_claims::CLAIMS_SCHEMA_VERSION
                    );
                }
                Ok(attestation)
            })
            .collect()
    }
}

/// Selector: case-sensitive substring over owner, name, and artifact id.
/// The combined form `owner-part::name-part` requires both to match; a
/// name part starting with `=` is an exact name match.
pub fn matches_selector(row: &DigestRow, selector: &str) -> bool {
    if let Some((owner_part, name_part)) = selector.split_once("::") {
        let name_matches = match name_part.strip_prefix('=') {
            Some(exact) => row.name == exact,
            None => row.name.contains(name_part),
        };
        return row.owner.contains(owner_part) && name_matches;
    }
    row.owner.contains(selector)
        || row.name.contains(selector)
        || row.artifact_id.contains(selector)
}

fn matches_graph_selector(row: &GraphRow, selector: &str) -> bool {
    let owner = row.graph_key.owner.owner();
    if let Some((owner_part, name_part)) = selector.split_once("::") {
        let name_matches = match name_part.strip_prefix('=') {
            Some(exact) => row.name == exact,
            None => row.name.contains(name_part),
        };
        return owner.contains(owner_part) && name_matches;
    }
    owner.contains(selector) || row.name.contains(selector) || row.artifact_id.contains(selector)
}
