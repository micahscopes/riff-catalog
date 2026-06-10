//! The JSONL corpus: one file per ingested artifact plus claims/attestations
//! files. Every line is one self-contained record.

use std::collections::BTreeMap;
use std::io::Write;
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
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(append)
            .write(true)
            .truncate(!append)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        for record in records {
            serde_json::to_writer(&mut file, record)?;
            file.write_all(b"\n")?;
        }
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
                } if row_unit == unit && row_mode == mode => Some(DigestRow {
                    artifact_id,
                    owner,
                    unit: row_unit,
                    level,
                    name,
                    mode: row_mode,
                    policy_id,
                    digests,
                }),
                _ => None,
            })
            .collect())
    }

    pub fn claims(&self) -> Result<Vec<Claim>> {
        Ok(self
            .load_non_graph_records()?
            .into_iter()
            .filter_map(|record| match record {
                Record::Claim { claim, .. } => Some(claim),
                _ => None,
            })
            .collect())
    }

    pub fn attestations(&self) -> Result<Vec<Attestation>> {
        Ok(self
            .load_non_graph_records()?
            .into_iter()
            .filter_map(|record| match record {
                Record::Attestation { attestation, .. } => Some(attestation),
                _ => None,
            })
            .collect())
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
