//! The JSONL corpus: one file per admitted batch, one per stored recipe
//! declaration, plus claims/attestations files. Every line is one
//! self-contained record.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use riff_catalog_claims::{Attestation, Claim};
use riff_catalog_core::{Digest, Dimension, Graph, GraphKey, PolicyId, RecipeDecl, Status};
use serde::{Deserialize, Serialize};

use crate::declarations::Declarations;

/// Cheaply decide whether a raw JSONL line is a `Graph` record without parsing
/// it. `Record` is internally tagged (`#[serde(tag = "record")]`) and written
/// compactly, so the tag is the first field on every line. We scan only a short
/// prefix, keeping this O(1) regardless of the graph payload's size (a single
/// graph line can be hundreds of MB). The tag is ASCII, so a byte-level search
/// is robust to any UTF-8 content further along the line.
fn bytes_start_graph_record(bytes: &[u8]) -> bool {
    const PREFIX: usize = 64;
    const TAG: &[u8] = b"\"record\":\"graph\"";
    let head = &bytes[..bytes.len().min(PREFIX)];
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
        status: Status,
        graph_key: GraphKey,
        graph: Graph,
    },
    Digest {
        artifact_id: String,
        owner: String,
        unit: String,
        level: String,
        name: String,
        status: Status,
        mode: String,
        policy_id: PolicyId,
        digests: BTreeMap<Dimension, Digest>,
        /// The bare shape: nodes and edges with no facts.
        shape: Digest,
        /// Pairs of dimensions whose stored digests cannot simply be combined
        /// for this unit (`GraphHashes::mixed_pairs`).
        mixed_pairs: u32,
        /// Joint digests for the dimension sets `mixed_pairs` flags, keyed by
        /// the dimension names joined with `+`. Empty for nearly every unit.
        joint: BTreeMap<String, Digest>,
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
    /// The declaration the corpus's units of one recipe were admitted under.
    Declaration { declaration: RecipeDecl },
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
    pub status: Status,
    pub mode: String,
    pub policy_id: PolicyId,
    pub digests: BTreeMap<Dimension, Digest>,
    pub mixed_pairs: u32,
    pub joint: BTreeMap<String, Digest>,
}

/// A persisted graph plus the corpus coordinates that identify it.
#[derive(Clone, Debug)]
pub struct GraphRow {
    pub artifact_id: String,
    pub unit: String,
    pub name: String,
    pub status: Status,
    pub graph_key: GraphKey,
    pub graph: Graph,
}

pub struct Corpus {
    pub dir: PathBuf,
    declarations: Declarations,
}

/// The file a recipe's declaration is stored in.
fn declaration_stem(recipe: &str) -> String {
    format!("declaration-{}", file_safe(recipe))
}

/// A file-name-safe rendering of a label, made unique by a short hash of the
/// label itself so that two labels never share a file.
pub fn file_safe(label: &str) -> String {
    use sha2::{Digest as _, Sha256};
    let readable: String =
        label.chars().take(80).map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect();
    format!("{readable}-{}", &hex::encode(Sha256::digest(label.as_bytes()))[..10])
}

impl Corpus {
    /// Open a corpus directory, creating it when absent, and read the
    /// declarations stored in it.
    pub fn open(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("creating corpus dir {}", dir.display()))?;
        let mut corpus = Self { dir: dir.to_path_buf(), declarations: Declarations::default() };
        for record in corpus.load_non_graph_records()? {
            if let Record::Declaration { declaration } = record {
                corpus.declarations.declare(declaration)?;
            }
        }
        Ok(corpus)
    }

    /// The declarations this corpus admits units under.
    pub fn declarations(&self) -> &Declarations {
        &self.declarations
    }

    /// Declare a recipe. A declaration that differs from the one the corpus
    /// already holds units under is refused: those units were checked and
    /// hashed under the stored one.
    pub fn declare(&mut self, decl: RecipeDecl) -> Result<()> {
        let recipe = decl.recipe.as_str().to_string();
        self.declarations
            .declare(decl)
            .with_context(|| format!("corpus {} was built under another declaration of {recipe}", self.dir.display()))
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
        // Store the declaration of every recipe these records hold units of,
        // before the units themselves.
        let recipes: std::collections::BTreeSet<&str> = records
            .iter()
            .filter_map(|record| match record {
                Record::Graph { level, .. } => Some(level.as_str()),
                _ => None,
            })
            .collect();
        for recipe in recipes {
            let Some(declaration) = self.declarations.get(recipe) else { continue };
            let path = self.dir.join(format!("{}.jsonl", declaration_stem(recipe)));
            if !path.exists() {
                self.write_file(&path, &[Record::Declaration { declaration: declaration.clone() }], false)?;
            }
        }
        self.write_file(&self.dir.join(format!("{file_stem}.jsonl")), records, append)
    }

    /// Write one file. A replacement is written beside the file and renamed
    /// over it, so a failure leaves the earlier contents in place.
    fn write_file(&self, path: &Path, records: &[Record], append: bool) -> Result<()> {
        let target = if append { path.to_path_buf() } else { path.with_extension("jsonl.tmp") };
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(append)
            .write(true)
            .truncate(!append)
            .open(&target)
            .with_context(|| format!("opening {}", target.display()))?;
        let mut writer = BufWriter::new(file);
        for record in records {
            serde_json::to_writer(&mut writer, record)?;
            writer.write_all(b"\n")?;
        }
        writer
            .flush()
            .with_context(|| format!("flushing {}", target.display()))?;
        drop(writer);
        if !append {
            std::fs::rename(&target, path).with_context(|| format!("replacing {}", path.display()))?;
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
            let file = std::fs::File::open(&path)?;
            let mut reader = BufReader::new(file);
            let mut line_no = 0usize;
            loop {
                let prefix = reader.fill_buf()?;
                if prefix.is_empty() {
                    break;
                }
                line_no += 1;
                if bytes_start_graph_record(prefix) {
                    // Graph records can be close to a gigabyte because JSONL
                    // stores one graph on one line. Discard the line directly
                    // from the buffered reader instead of allocating it merely
                    // to discover the tag and skip it.
                    loop {
                        let available = reader.fill_buf()?;
                        if available.is_empty() {
                            break;
                        }
                        match available.iter().position(|byte| *byte == b'\n') {
                            Some(end) => {
                                reader.consume(end + 1);
                                break;
                            }
                            None => {
                                let length = available.len();
                                reader.consume(length);
                            }
                        }
                    }
                    continue;
                }
                let mut line = String::new();
                reader.read_line(&mut line)?;
                if line.trim().is_empty() {
                    continue;
                }
                let record: Record = serde_json::from_str(&line)
                    .with_context(|| format!("parsing {}:{}", path.display(), line_no))?;
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
                    || !bytes_start_graph_record(line.as_bytes())
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
                    status,
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
                    status,
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
                    status,
                    mode: row_mode,
                    policy_id,
                    digests,
                    mixed_pairs,
                    joint,
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
                        status,
                        mode: row_mode,
                        policy_id,
                        digests,
                        mixed_pairs,
                        joint,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(id: &str) -> Record {
        Record::Artifact {
            artifact_id: id.to_owned(),
            owner: format!("owner:{id}"),
            origin: format!("origin:{id}"),
            pipeline: "test".to_owned(),
            optimize: false,
            compiler: None,
            label: None,
        }
    }

    #[test]
    fn buffered_replace_and_append_are_fully_persisted() {
        let directory =
            std::env::temp_dir().join(format!("riffcat-buffered-corpus-{}", std::process::id()));
        if directory.exists() {
            std::fs::remove_dir_all(&directory).unwrap();
        }
        let corpus = Corpus::open(&directory).unwrap();
        corpus.replace("records", &[artifact("first")]).unwrap();
        corpus.append("records", &[artifact("second")]).unwrap();

        let text = std::fs::read_to_string(directory.join("records.jsonl")).unwrap();
        let records = text
            .lines()
            .map(|line| serde_json::from_str::<Record>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 2);
        assert!(matches!(
            &records[0],
            Record::Artifact { artifact_id, .. } if artifact_id == "first"
        ));
        assert!(matches!(
            &records[1],
            Record::Artifact { artifact_id, .. } if artifact_id == "second"
        ));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn non_graph_reader_stream_discards_large_graph_records() {
        let directory =
            std::env::temp_dir().join(format!("riffcat-streamed-corpus-{}", std::process::id()));
        if directory.exists() {
            std::fs::remove_dir_all(&directory).unwrap();
        }
        let corpus = Corpus::open(&directory).unwrap();
        let path = directory.join("records.jsonl");
        let mut writer = BufWriter::new(std::fs::File::create(&path).unwrap());
        writer
            .write_all(b"{\"record\":\"graph\",\"padding\":\"")
            .unwrap();
        writer.write_all(&vec![b'x'; 1024 * 1024]).unwrap();
        writer.write_all(b"\"}\n").unwrap();
        serde_json::to_writer(&mut writer, &artifact("after-graph")).unwrap();
        writer.write_all(b"\n").unwrap();
        writer.flush().unwrap();

        let records = corpus.load_non_graph_records().unwrap();
        assert_eq!(records.len(), 1);
        assert!(matches!(
            &records[0],
            Record::Artifact { artifact_id, .. } if artifact_id == "after-graph"
        ));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
