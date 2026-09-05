//! Replayable census sidecars. Integrity and reproducibility, not authorship.
use crate::{ArtifactCensus, CensusRegion, census_capture, census_file};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const SCHEMA: &str = "riffcat-census-file/1";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CensusSource {
    Artifact {
        path: PathBuf,
        regions: Option<PathBuf>,
    },
    Capture {
        path: PathBuf,
        artifact_id: String,
        regions: Option<PathBuf>,
    },
}

impl CensusSource {
    pub fn run(&self) -> Result<ArtifactCensus> {
        match self {
            Self::Artifact { path, regions } => census_file(path, regions.as_deref()),
            Self::Capture {
                path,
                artifact_id,
                regions,
            } => census_capture(path, artifact_id, regions.as_deref()),
        }
    }

    fn canonicalize(&mut self) -> Result<()> {
        let (path, regions) = match self {
            Self::Artifact { path, regions } | Self::Capture { path, regions, .. } => {
                (path, regions)
            }
        };
        *path = fs::canonicalize(&path)?;
        if let Some(path) = regions {
            *path = fs::canonicalize(&path)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CensusFile {
    pub schema: String,
    pub census_id: String,
    pub source: CensusSource,
    pub census: ArtifactCensus,
}

fn identity(file: &CensusFile) -> Result<String> {
    let mut hash = blake3::Hasher::new_derive_key("riffcat/artifact-census-sidecar/1");
    hash.update(&serde_json::to_vec(&(
        &file.schema,
        &file.source,
        &file.census,
    ))?);
    Ok(hash.finalize().to_hex().to_string())
}

pub fn save_census(path: &Path, mut source: CensusSource) -> Result<CensusFile> {
    source.canonicalize()?;
    let census = source.run()?;
    let mut file = CensusFile {
        schema: SCHEMA.into(),
        census_id: String::new(),
        source,
        census,
    };
    file.census_id = identity(&file)?;
    let bytes = serde_json::to_vec_pretty(&file)?;
    ensure!(
        bytes.len() <= 64 * 1024 * 1024,
        "census sidecar exceeds replayable 64 MiB limit"
    );
    publish(path, &bytes, |file, bytes| file.write_all(bytes))?;
    Ok(file)
}

fn publish(
    path: &Path,
    bytes: &[u8],
    write: impl FnOnce(&mut fs::File, &[u8]) -> std::io::Result<()>,
) -> Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Pending(PathBuf);
    impl Drop for Pending {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let (pending, mut output) = loop {
        let temp = parent.join(format!(
            ".riffcat-census-pending-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
        {
            Ok(file) => break (Pending(temp), file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    };
    write(&mut output, bytes)?;
    output.sync_all()?;
    // A hard link publishes a complete file atomically without replacing a target.
    match fs::hard_link(&pending.0, path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            ensure!(
                read_sidecar(path)? == bytes,
                "refusing to overwrite a different census sidecar"
            );
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn read_sidecar(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 64 * 1024 * 1024,
        "census sidecar exceeds 64 MiB limit"
    );
    Ok(bytes)
}

/// Recompute from the source artifact/capture, not just from self-reported hashes.
pub fn replay_census(path: &Path) -> Result<CensusFile> {
    let file: CensusFile = serde_json::from_slice(&read_sidecar(path)?)?;
    ensure!(file.schema == SCHEMA, "unsupported census sidecar schema");
    ensure!(
        file.census_id == identity(&file)?,
        "census sidecar identity mismatch"
    );
    let recomputed = file.source.run()?;
    ensure!(
        serde_json::to_value(&recomputed)? == serde_json::to_value(&file.census)?,
        "census differs from recomputed source evidence"
    );
    Ok(file)
}

#[derive(Clone, Debug, Serialize)]
pub struct CensusComparison {
    pub schema: String,
    pub left_census: String,
    pub right_census: String,
    pub left_artifact_bytes: usize,
    pub right_artifact_bytes: usize,
    pub artifact_byte_delta: i64,
    pub adapter_aligned: bool,
    pub capture_alignment_equal: Option<bool>,
    pub intervention_equal: Option<bool>,
    pub left_capture_context: Option<crate::CensusCaptureContext>,
    pub right_capture_context: Option<crate::CensusCaptureContext>,
    pub functions: Vec<FunctionDelta>,
    pub patterns: Vec<PatternDelta>,
    pub caveats: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FunctionDelta {
    pub name: String,
    pub left_regions: Vec<String>,
    pub right_regions: Vec<String>,
    pub left_bytes: Option<usize>,
    pub right_bytes: Option<usize>,
    pub byte_delta: Option<i64>,
    pub alignment: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct PatternDelta {
    pub policy: String,
    pub region_kind: String,
    pub digest: String,
    pub left_occurrences: Option<usize>,
    pub right_occurrences: Option<usize>,
    pub left_covered_bytes: Option<usize>,
    pub right_covered_bytes: Option<usize>,
}

/// Inputs must have been replayed if verification is required by the caller.
pub fn compare_censuses(left: &CensusFile, right: &CensusFile) -> CensusComparison {
    fn functions(report: &ArtifactCensus) -> BTreeMap<String, Vec<&CensusRegion>> {
        let mut map: BTreeMap<String, Vec<&CensusRegion>> = BTreeMap::new();
        for region in report
            .regions
            .iter()
            .filter(|r| r.region.kind == "function")
        {
            map.entry(region.region.name.clone())
                .or_default()
                .push(region);
        }
        map
    }
    let lf = functions(&left.census);
    let rf = functions(&right.census);
    let names: BTreeSet<_> = lf.keys().chain(rf.keys()).collect();
    let adapter_aligned = left.census.adapter == right.census.adapter;
    let mut rows = Vec::new();
    for name in names {
        let l = lf.get(name).cloned().unwrap_or_default();
        let r = rf.get(name).cloned().unwrap_or_default();
        let unique = !name.is_empty() && l.len() == 1 && r.len() == 1 && adapter_aligned;
        rows.push(FunctionDelta {
            name: name.clone(),
            left_regions: l.iter().map(|r| r.region.id.clone()).collect(),
            right_regions: r.iter().map(|r| r.region.id.clone()).collect(),
            left_bytes: (l.len() == 1).then(|| l[0].bytes),
            right_bytes: (r.len() == 1).then(|| r[0].bytes),
            byte_delta: unique.then(|| r[0].bytes as i64 - l[0].bytes as i64),
            alignment: if unique {
                "unique-name hint only"
            } else {
                "unpaired: absent, ambiguous or adapter mismatch"
            }
            .into(),
        });
    }
    rows.sort_by(|a, b| {
        b.byte_delta
            .unwrap_or(0)
            .unsigned_abs()
            .cmp(&a.byte_delta.unwrap_or(0).unsigned_abs())
            .then_with(|| a.name.cmp(&b.name))
    });
    fn patterns(
        report: &ArtifactCensus,
    ) -> BTreeMap<(String, String, String), &crate::PatternGroup> {
        report
            .patterns
            .iter()
            .map(|p| {
                (
                    (p.policy.clone(), p.region_kind.clone(), p.digest.clone()),
                    p,
                )
            })
            .collect()
    }
    let lp = patterns(&left.census);
    let rp = patterns(&right.census);
    let keys: BTreeSet<_> = lp.keys().chain(rp.keys()).collect();
    let patterns = keys
        .into_iter()
        .map(|(policy, kind, digest)| {
            let key = (policy.clone(), kind.clone(), digest.clone());
            let l = lp.get(&key);
            let r = rp.get(&key);
            PatternDelta {
                policy: policy.clone(),
                region_kind: kind.clone(),
                digest: digest.clone(),
                left_occurrences: l.map(|p| p.occurrences),
                right_occurrences: r.map(|p| p.occurrences),
                left_covered_bytes: l.map(|p| p.covered_bytes),
                right_covered_bytes: r.map(|p| p.covered_bytes),
            }
        })
        .collect();
    CensusComparison {
        schema: "riffcat-census-comparison/1".into(), left_census: left.census_id.clone(), right_census: right.census_id.clone(),
        left_artifact_bytes: left.census.artifact_bytes, right_artifact_bytes: right.census.artifact_bytes,
        artifact_byte_delta: right.census.artifact_bytes as i64 - left.census.artifact_bytes as i64,
        adapter_aligned,
        capture_alignment_equal: left.census.capture_context.as_ref().zip(right.census.capture_context.as_ref()).map(|(l,r)| l.alignment == r.alignment),
        intervention_equal: left.census.capture_context.as_ref().zip(right.census.capture_context.as_ref()).map(|(l,r)| l.intervention == r.intervention),
        left_capture_context: left.census.capture_context.clone(),
        right_capture_context: right.census.capture_context.clone(),
        functions: rows, patterns,
        caveats: vec![
            "Function names are alignment hints, not cross-stage identity. Renamed and ambiguous functions remain unpaired.".into(),
            "Pattern correspondence uses matching policy, region kind and digest, not semantic equivalence or causal attribution.".into(),
            "Missing repeated groups are unknown, not zero: a pattern may occur once or fall outside adapter coverage.".into(),
            "Pattern policies overlap; do not sum their covered-byte totals. Size changes do not establish correctness or runtime speedup.".into(),
        ],
    }
}

pub fn render_census_comparison(report: &CensusComparison, top: usize) -> String {
    let mut text = format!(
        "bytes: {} -> {} ({:+})\nadapter aligned: {}\ncapture source/compiler/settings aligned: {:?}\n",
        report.left_artifact_bytes,
        report.right_artifact_bytes,
        report.artifact_byte_delta,
        report.adapter_aligned,
        report.capture_alignment_equal
    );
    text.push_str(&format!(
        "intervention equal: {:?}\n",
        report.intervention_equal
    ));
    for (side, context) in [
        ("left", &report.left_capture_context),
        ("right", &report.right_capture_context),
    ] {
        if let Some(context) = context {
            if !matches!(
                context.completion,
                crate::CaptureCompletion::Complete { .. }
            ) {
                text.push_str(&format!(
                    "WARNING: {side} capture is not complete: {:?}\n",
                    context.completion
                ));
            }
        }
    }
    for row in report.functions.iter().take(top) {
        text.push_str(&format!(
            "{}\t{:?} -> {:?}\tdelta {:?}\t{}\n",
            row.name, row.left_bytes, row.right_bytes, row.byte_delta, row.alignment
        ));
    }
    for caveat in &report.caveats {
        text.push_str(&format!("note: {caveat}\n"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_write_does_not_publish_or_leave_a_temporary_file() {
        let temp = scratch("write-failure");
        let target = temp.0.join("output.json");
        let result = publish(&target, b"complete", |file, _| {
            file.write_all(b"partial")?;
            Err(std::io::Error::other("injected write failure"))
        });
        assert!(result.is_err());
        assert!(!target.exists());
        assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 0);
        publish(&target, b"complete", |f, b| f.write_all(b)).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"complete");
    }

    #[test]
    fn relocation_requires_new_sidecar_but_keeps_artifact_address() {
        let temp = scratch("relocate");
        let before = temp.0.join("before.wgsl");
        let after = temp.0.join("after.wgsl");
        fs::write(&before, "fn f() {}").unwrap();
        let sidecar = temp.0.join("before.json");
        let old = save_census(&sidecar, source(&before)).unwrap();
        fs::rename(&before, &after).unwrap();
        assert!(replay_census(&sidecar).is_err());
        let new = save_census(&temp.0.join("after.json"), source(&after)).unwrap();
        assert_ne!(old.census_id, new.census_id);
        assert_eq!(old.census.artifact_blake3, new.census.artifact_blake3);
        assert!(
            compare_censuses(&old, &new)
                .capture_alignment_equal
                .is_none()
        );
    }
    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn scratch(name: &str) -> Scratch {
        let path = PathBuf::from("/workspace/scratch")
            .join(format!("riffcat-census-{}-{name}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Scratch(path)
    }
    fn source(path: &Path) -> CensusSource {
        CensusSource::Artifact {
            path: path.into(),
            regions: None,
        }
    }

    #[test]
    fn sidecars_recompute_refuse_clobber_and_reject_mutated_evidence() {
        let temp = scratch("replay");
        let artifact = temp.0.join("test.wgsl");
        let sidecar = temp.0.join("census.json");
        fs::write(&artifact, "fn f() { s.a = x.b; call(); s.a = y.b; }").unwrap();
        let saved = save_census(&sidecar, source(&artifact)).unwrap();
        assert_eq!(
            saved.census_id,
            save_census(&sidecar, source(&artifact)).unwrap().census_id
        );
        assert_eq!(saved.census_id, replay_census(&sidecar).unwrap().census_id);
        let before = fs::read(&sidecar).unwrap();
        fs::write(&artifact, "fn f() { s.a = x.c; call(); s.a = y.c; }").unwrap();
        assert!(replay_census(&sidecar).is_err());
        assert!(save_census(&sidecar, source(&artifact)).is_err());
        assert_eq!(fs::read(&sidecar).unwrap(), before);
        let mut altered: serde_json::Value = serde_json::from_slice(&before).unwrap();
        altered["census"]["outside_region_bytes"] = 999.into();
        fs::write(&sidecar, serde_json::to_vec(&altered).unwrap()).unwrap();
        assert!(replay_census(&sidecar).is_err());
    }

    #[test]
    fn comparisons_pair_only_unique_names_and_preserve_unknown_groups() {
        let temp = scratch("compare");
        let left = temp.0.join("left.wgsl");
        let right = temp.0.join("right.wgsl");
        fs::write(&left, "fn f() { s.a = x.b; call(); s.a = y.b; }").unwrap();
        fs::write(&right, "fn f() { s.a = x.b; } fn added() {}").unwrap();
        let l = save_census(&temp.0.join("l.json"), source(&left)).unwrap();
        let r = save_census(&temp.0.join("r.json"), source(&right)).unwrap();
        let comparison = compare_censuses(&l, &r);
        assert!(
            comparison
                .functions
                .iter()
                .find(|f| f.name == "f")
                .unwrap()
                .byte_delta
                .unwrap()
                < 0
        );
        assert!(
            comparison
                .functions
                .iter()
                .find(|f| f.name == "added")
                .unwrap()
                .byte_delta
                .is_none()
        );
        assert!(
            comparison
                .patterns
                .iter()
                .all(|p| p.right_occurrences.is_none())
        );
        fs::write(&right, "fn f() {} fn f() {}").unwrap();
        let r = save_census(&temp.0.join("ambiguous.json"), source(&right)).unwrap();
        assert!(compare_censuses(&l, &r).functions[0].byte_delta.is_none());
    }
}
