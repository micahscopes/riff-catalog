//! Exact artifact regions and explicitly scoped textual pattern matches.
//! This is a source-text census, not a compiler parser or semantic equivalence proof.

use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_REGIONS: usize = 100_000;
const WGSL_POLICY: &str = "wgsl-projection-run/root-renaming-preserve-members-indices/1";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionSpec {
    pub id: String,
    pub kind: String,
    pub name: String,
    /// Half-open byte offsets, not character or line offsets.
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionManifest {
    pub schema: String,
    pub artifact_blake3: String,
    /// Producer-owned adapter name and version. Not a semantic identity policy.
    pub adapter: String,
    pub regions: Vec<RegionSpec>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CensusRegion {
    pub region: RegionSpec,
    pub bytes: usize,
    pub content_blake3: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement_count: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternGroup {
    pub policy: String,
    pub region_kind: String,
    pub digest: String,
    pub regions: Vec<String>,
    pub occurrences: usize,
    pub statements_per_occurrence: Option<usize>,
    /// Union of the participating ranges, not predicted removable bytes.
    pub covered_bytes: usize,
    pub function_scopes: Vec<PatternScope>,
    pub unassigned_occurrences: usize,
    pub unassigned_covered_bytes: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatternScope {
    pub region_id: String,
    pub name: String,
    pub occurrences: usize,
    pub covered_bytes: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactCensus {
    pub schema: String,
    pub artifact_blake3: String,
    pub artifact_bytes: usize,
    pub adapter: String,
    pub regions: Vec<CensusRegion>,
    pub region_union_bytes: usize,
    pub outside_region_bytes: usize,
    pub patterns: Vec<PatternGroup>,
    pub caveats: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_context: Option<CensusCaptureContext>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CensusCaptureContext {
    pub capture_id: String,
    pub artifact_id: String,
    pub completion: crate::CaptureCompletion,
    pub alignment: crate::Alignment,
    pub intervention: crate::Intervention,
    pub provenance: crate::Provenance,
}

fn union_bytes(mut spans: Vec<(usize, usize)>) -> usize {
    spans.sort_unstable();
    let (mut end, mut total) = (0, 0);
    for (start, stop) in spans {
        if stop > end {
            total += stop - start.max(end);
            end = stop;
        }
    }
    total
}

pub fn census_regions(bytes: &[u8], manifest: RegionManifest) -> Result<ArtifactCensus> {
    ensure!(
        bytes.len() <= MAX_BYTES,
        "artifact exceeds 64 MiB census limit"
    );
    ensure!(
        manifest.schema == "riffcat-regions/1",
        "unsupported region schema"
    );
    ensure!(
        !manifest.adapter.is_empty(),
        "adapter name/version is required"
    );
    let digest = blake3::hash(bytes).to_hex().to_string();
    ensure!(
        manifest.artifact_blake3 == digest,
        "region manifest artifact digest mismatch"
    );
    ensure!(
        manifest.regions.len() <= MAX_REGIONS,
        "too many census regions"
    );
    let mut ids = std::collections::BTreeSet::new();
    let mut ranges = std::collections::BTreeSet::new();
    let mut regions = Vec::new();
    let mut selected_bytes = 0usize;
    let selection_budget = bytes.len().saturating_mul(8).min(256 * 1024 * 1024);
    for region in manifest.regions {
        ensure!(!region.kind.is_empty(), "region kind must not be empty");
        ensure!(
            !region.id.is_empty() && ids.insert(region.id.clone()),
            "empty or duplicate region ID"
        );
        ensure!(
            region.start < region.end && region.end <= bytes.len(),
            "invalid region range: {}",
            region.id
        );
        ensure!(
            ranges.insert((region.kind.clone(), region.start, region.end)),
            "duplicate range within region kind"
        );
        selected_bytes = selected_bytes
            .checked_add(region.end - region.start)
            .ok_or_else(|| anyhow::anyhow!("selected region bytes overflow"))?;
        ensure!(
            selected_bytes <= selection_budget,
            "selected region bytes exceed 8x artifact / 256 MiB work budget"
        );
        regions.push(CensusRegion {
            bytes: region.end - region.start,
            content_blake3: blake3::hash(&bytes[region.start..region.end])
                .to_hex()
                .to_string(),
            statement_count: None,
            region,
        });
    }
    let union = union_bytes(
        regions
            .iter()
            .map(|r| (r.region.start, r.region.end))
            .collect(),
    );
    let mut exact: BTreeMap<(&str, &[u8]), Vec<&CensusRegion>> = BTreeMap::new();
    for r in &regions {
        exact
            .entry((&r.region.kind, &bytes[r.region.start..r.region.end]))
            .or_default()
            .push(r);
    }
    let mut patterns: Vec<_> = exact
        .into_iter()
        .filter(|(_, rs)| rs.len() > 1)
        .map(|((kind, content), rs)| PatternGroup {
            policy: "exact-region-bytes/blake3/1".into(),
            region_kind: kind.into(),
            digest: blake3::hash(content).to_hex().to_string(),
            occurrences: rs.len(),
            regions: rs.iter().map(|r| r.region.id.clone()).collect(),
            statements_per_occurrence: None,
            covered_bytes: union_bytes(rs.iter().map(|r| (r.region.start, r.region.end)).collect()),
            function_scopes: Vec::new(),
            unassigned_occurrences: rs.len(),
            unassigned_covered_bytes: union_bytes(
                rs.iter().map(|r| (r.region.start, r.region.end)).collect(),
            ),
        })
        .collect();
    patterns.sort_by(|a, b| {
        b.covered_bytes
            .cmp(&a.covered_bytes)
            .then_with(|| a.region_kind.cmp(&b.region_kind))
            .then_with(|| a.digest.cmp(&b.digest))
    });
    Ok(ArtifactCensus {
        schema: "riffcat-artifact-census/1".into(), artifact_blake3: digest,
        artifact_bytes: bytes.len(), adapter: manifest.adapter, regions,
        region_union_bytes: union, outside_region_bytes: bytes.len() - union, patterns,
        caveats: vec![
            "Byte ranges are exact; region selection is adapter evidence, not semantic attribution.".into(),
            "Nested/overlapping regions and pattern groups are not additive. Union counts avoid double counting within each reported group.".into(),
            "The same bytes can appear under exact-byte and projection policies. Do not add policy totals. Manifest-driven occurrences have no automatic function-scope attribution.".into(),
            "Repeated text is not necessarily redundant work or removable bytes. No behavior, aliasing or runtime-cost claim is made.".into(),
        ],
        capture_context: None,
    })
}

#[derive(Clone, Copy)]
struct Token<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}

fn lex(source: &str) -> Result<Vec<Token<'_>>> {
    lex_with_budget(source, 2_000_000)
}

fn lex_with_budget(source: &str, token_budget: usize) -> Result<Vec<Token<'_>>> {
    let b = source.as_bytes();
    let mut i = 0;
    let mut tokens = Vec::new();
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if b[i..].starts_with(b"//") {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b[i..].starts_with(b"/*") {
            i += 2;
            let mut depth = 1;
            while i < b.len() && depth > 0 {
                if b[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if b[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            ensure!(depth == 0, "unterminated WGSL block comment");
            continue;
        }
        let start = i;
        if b[i].is_ascii_alphanumeric() || b[i] == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
        } else {
            let ch = source[i..].chars().next().unwrap();
            ensure!(
                ch != '"',
                "quoted WGSL text is unsupported by this census adapter"
            );
            i += ch.len_utf8();
        }
        tokens.push(Token {
            text: &source[start..i],
            start,
            end: i,
        });
        ensure!(
            tokens.len() <= token_budget,
            "WGSL census exceeds token budget"
        );
    }
    Ok(tokens)
}

fn identifier(text: &str) -> bool {
    text.as_bytes()
        .first()
        .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
        && text.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

/// Accept only root/member/constant-index projections, optionally dereferenced.
/// Calls, arithmetic, dynamic indices and compound assignments are not matches.
fn projection<'a>(ts: &[Token<'a>]) -> Option<(&'a str, Vec<&'a str>)> {
    let (root, mut i, mut normalized) = if ts.first()?.text == "(" {
        if ts.len() < 4 || ts[1].text != "*" || !identifier(ts[2].text) || ts[3].text != ")" {
            return None;
        }
        (ts[2].text, 4, vec!["(", "*", "$root", ")"])
    } else {
        if !identifier(ts[0].text) || matches!(ts[0].text, "true" | "false") {
            return None;
        }
        (ts[0].text, 1, vec!["$root"])
    };
    while i < ts.len() {
        if ts[i].text == "." && i + 1 < ts.len() && identifier(ts[i + 1].text) {
            normalized.extend([".", ts[i + 1].text]);
            i += 2;
        } else if ts[i].text == "["
            && i + 2 < ts.len()
            && ts[i + 2].text == "]"
            && ts[i + 1]
                .text
                .trim_end_matches('u')
                .bytes()
                .all(|c| c.is_ascii_digit())
            && !ts[i + 1].text.trim_end_matches('u').is_empty()
        {
            normalized.extend(["[", ts[i + 1].text, "]"]);
            i += 3;
        } else {
            return None;
        }
    }
    Some((root, normalized))
}

struct Move<'a> {
    start: usize,
    end: usize,
    left: &'a str,
    right: &'a str,
    key: Vec<&'a str>,
}

fn simple_move<'a>(ts: &[Token<'a>], source: &str) -> Option<Move<'a>> {
    let equals = ts.iter().position(|t| t.text == "=")?;
    let left_start = usize::from(ts.first()?.text == "let");
    let (left, mut key) = projection(&ts[left_start..equals])?;
    let (right, rhs) = projection(&ts[equals + 1..ts.len() - 1])?;
    key.insert(0, if left_start == 1 { "let" } else { "assign" });
    key.push(if left == right {
        "=same-root"
    } else {
        "=distinct-root"
    });
    key.extend(rhs);
    let mut start = ts[0].start;
    let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
    if source[line_start..start]
        .bytes()
        .all(|c| c.is_ascii_whitespace())
    {
        start = line_start;
    }
    let mut end = ts.last()?.end;
    let line_end = source[end..].find('\n').map(|i| end + i + 1);
    if let Some(stop) = line_end {
        if source[end..stop].bytes().all(|c| c.is_ascii_whitespace()) {
            end = stop;
        }
    }
    Some(Move {
        start,
        end,
        left,
        right,
        key,
    })
}

pub fn census_wgsl(source: &str) -> Result<ArtifactCensus> {
    ensure!(
        source.len() <= MAX_BYTES,
        "artifact exceeds 64 MiB census limit"
    );
    let ts = lex(source)?;
    let mut regions = Vec::new();
    let mut groups: BTreeMap<Vec<u8>, (usize, Vec<String>)> = BTreeMap::new();
    let mut i = 0;
    let mut top_depth = 0usize;
    while i < ts.len() {
        if top_depth == 0 && ts[i].text == "fn" {
            let begin = i;
            ensure!(
                i + 2 < ts.len() && identifier(ts[i + 1].text) && ts[i + 2].text == "(",
                "unsupported WGSL function header"
            );
            let name = ts[i + 1].text;
            while i < ts.len() && ts[i].text != "{" {
                i += 1;
            }
            ensure!(i < ts.len(), "WGSL function has no body");
            let body = i + 1;
            let mut depth = 1;
            i += 1;
            while i < ts.len() && depth > 0 {
                match ts[i].text {
                    "{" => depth += 1,
                    "}" => depth -= 1,
                    _ => {}
                }
                i += 1;
            }
            ensure!(depth == 0, "unterminated WGSL function");
            let function_id = format!("function:{}", ts[begin].start);
            regions.push(RegionSpec {
                id: function_id.clone(),
                kind: "function".into(),
                name: name.into(),
                start: ts[begin].start,
                end: ts[i - 1].end,
            });
            let mut run: Vec<Move<'_>> = Vec::new();
            let mut flush = |run: &mut Vec<Move<'_>>| -> Result<()> {
                if run.is_empty() {
                    return Ok(());
                }
                let first = &run[0];
                let id = format!("projection-run:{}", first.start);
                let key = serde_json::to_vec(&run.iter().map(|m| &m.key).collect::<Vec<_>>())?;
                let entry = groups.entry(key).or_insert_with(|| (run.len(), Vec::new()));
                entry.1.push(id.clone());
                regions.push(RegionSpec {
                    id,
                    kind: "projection_run".into(),
                    name: format!(
                        "{name}: {} <- {} ({} statements)",
                        first.left,
                        first.right,
                        run.len()
                    ),
                    start: first.start,
                    end: run.last().unwrap().end,
                });
                ensure!(regions.len() <= MAX_REGIONS, "too many WGSL census regions");
                run.clear();
                Ok(())
            };
            let (mut start, mut parens) = (body, 0usize);
            for j in body..i - 1 {
                match ts[j].text {
                    "(" => parens += 1,
                    ")" => {
                        parens = parens.saturating_sub(1);
                    }
                    "{" | "}" => {
                        flush(&mut run)?;
                        start = j + 1;
                    }
                    ";" if parens == 0 => {
                        if let Some(m) = simple_move(&ts[start..=j], source) {
                            if run.last().is_some_and(|p| {
                                p.left != m.left
                                    || p.right != m.right
                                    || !source[p.end..m.start.max(p.end)]
                                        .bytes()
                                        .all(|c| c.is_ascii_whitespace())
                            }) {
                                flush(&mut run)?;
                            }
                            run.push(m);
                        } else {
                            flush(&mut run)?;
                        }
                        start = j + 1;
                    }
                    _ => {}
                }
            }
            flush(&mut run)?;
            ensure!(regions.len() <= MAX_REGIONS, "too many WGSL census regions");
        } else {
            match ts[i].text {
                "{" => top_depth += 1,
                "}" => {
                    ensure!(top_depth > 0, "unbalanced WGSL braces");
                    top_depth -= 1;
                }
                _ => {}
            }
            i += 1;
        }
    }
    ensure!(top_depth == 0, "unbalanced WGSL braces");
    let manifest = RegionManifest {
        schema: "riffcat-regions/1".into(),
        artifact_blake3: blake3::hash(source.as_bytes()).to_hex().to_string(),
        adapter: "wgsl-lexical-regions/1".into(),
        regions,
    };
    let mut report = census_regions(source.as_bytes(), manifest)?;
    let by_id: BTreeMap<_, _> = report
        .regions
        .iter()
        .map(|r| (r.region.id.clone(), (r.region.start, r.region.end)))
        .collect();
    let counts: BTreeMap<_, _> = groups
        .values()
        .flat_map(|(count, ids)| ids.iter().map(move |id| (id.clone(), *count)))
        .collect();
    for region in &mut report.regions {
        region.statement_count = counts.get(&region.region.id).copied();
    }
    for (key, (statements, ids)) in groups {
        if ids.len() < 2 {
            continue;
        }
        report.patterns.push(PatternGroup {
            policy: WGSL_POLICY.into(),
            region_kind: "projection_run".into(),
            digest: {
                let mut hash = blake3::Hasher::new_derive_key(WGSL_POLICY);
                hash.update(&key);
                hash.finalize().to_hex().to_string()
            },
            occurrences: ids.len(),
            statements_per_occurrence: Some(statements),
            covered_bytes: union_bytes(ids.iter().map(|id| by_id[id]).collect()),
            regions: ids,
            function_scopes: Vec::new(),
            unassigned_occurrences: 0,
            unassigned_covered_bytes: 0,
        });
    }
    let functions: Vec<_> = report
        .regions
        .iter()
        .filter(|r| r.region.kind == "function")
        .collect();
    for pattern in &mut report.patterns {
        let mut scopes: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
        let mut unassigned = Vec::new();
        for id in &pattern.regions {
            let (start, end) = by_id[id];
            if let Some(index) = functions
                .partition_point(|f| f.region.start <= start)
                .checked_sub(1)
            {
                if end <= functions[index].region.end {
                    scopes.entry(index).or_default().push((start, end));
                    continue;
                }
            }
            unassigned.push((start, end));
        }
        pattern.unassigned_occurrences = unassigned.len();
        pattern.unassigned_covered_bytes = union_bytes(unassigned);
        pattern.function_scopes = scopes
            .into_iter()
            .map(|(index, spans)| PatternScope {
                region_id: functions[index].region.id.clone(),
                name: functions[index].region.name.clone(),
                occurrences: spans.len(),
                covered_bytes: union_bytes(spans),
            })
            .collect();
    }
    report.patterns.sort_by(|a, b| {
        b.covered_bytes
            .cmp(&a.covered_bytes)
            .then_with(|| a.policy.cmp(&b.policy))
            .then_with(|| a.digest.cmp(&b.digest))
    });
    report.caveats.push("WGSL lexical adapter is not a validator. Function spans start at fn and exclude preceding attributes. Projection-run spans include surrounding line whitespace when alone on a line; only runs separated by whitespace are combined. Root names are erased within each run; member names, constant indices, statement order and same-root relationships are retained. This is not binding-aware alpha-equivalence.".into());
    Ok(report)
}

pub fn census_file(path: &Path, manifest_path: Option<&Path>) -> Result<ArtifactCensus> {
    use std::io::Read;
    fn read_bounded(path: &Path) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        fs::File::open(path)?
            .take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= MAX_BYTES, "census input exceeds 64 MiB");
        Ok(bytes)
    }
    let bytes = read_bounded(path)?;
    if let Some(manifest) = manifest_path {
        census_regions(&bytes, serde_json::from_slice(&read_bounded(manifest)?)?)
    } else if path.extension().is_some_and(|e| e == "wgsl") {
        census_wgsl(std::str::from_utf8(&bytes)?)
    } else {
        bail!("provide --regions for non-WGSL artifacts");
    }
}

pub fn census_capture(
    capture_path: &Path,
    artifact_id: &str,
    manifest_path: Option<&Path>,
) -> Result<ArtifactCensus> {
    let capture = crate::load_capture(capture_path)?;
    crate::verify_artifacts(capture_path, &capture)?;
    let artifact = capture
        .capture
        .artifacts
        .iter()
        .find(|a| a.id == artifact_id)
        .ok_or_else(|| anyhow::anyhow!("capture has no artifact `{artifact_id}`"))?;
    let path = crate::manifest::resolve_artifact(capture_path, &artifact.path);
    let mut report = census_file(&path, manifest_path)?;
    ensure!(
        report.artifact_blake3 == artifact.blake3 && report.artifact_bytes as u64 == artifact.bytes,
        "selected artifact changed between capture verification and census"
    );
    report.capture_context = Some(CensusCaptureContext {
        capture_id: capture.capture_id,
        artifact_id: artifact_id.into(),
        completion: capture.capture.completion,
        alignment: capture.capture.alignment,
        intervention: capture.capture.intervention,
        provenance: capture.capture.provenance,
    });
    Ok(report)
}

pub fn render_census(report: &ArtifactCensus, top: usize) -> String {
    let mut out = format!(
        "artifact blake3:{}\nbytes: {}  covered union: {}  outside regions: {}\nadapter: {}\n",
        report.artifact_blake3,
        report.artifact_bytes,
        report.region_union_bytes,
        report.outside_region_bytes,
        report.adapter
    );
    let mut regions: Vec<_> = report
        .regions
        .iter()
        .filter(|r| r.region.kind != "projection_run")
        .collect();
    if let Some(context) = &report.capture_context {
        out.push_str(&format!(
            "capture: {}  artifact: {}  completion: {:?}\n",
            context.capture_id, context.artifact_id, context.completion
        ));
    }
    if report
        .capture_context
        .as_ref()
        .is_some_and(|c| !matches!(c.completion, crate::CaptureCompletion::Complete { .. }))
    {
        out.push_str("WARNING: capture is not complete; artifact bytes are measurable but compiler attribution is partial.\n");
    }
    regions.sort_by(|a, b| {
        b.bytes
            .cmp(&a.bytes)
            .then_with(|| a.region.id.cmp(&b.region.id))
    });
    out.push_str("\nLargest regions (half-open byte ranges):\n");
    for r in regions.into_iter().take(top) {
        out.push_str(&format!(
            "{}\t{}..{}\t{}\t{}\n",
            r.bytes, r.region.start, r.region.end, r.region.kind, r.region.name
        ));
    }
    out.push_str("\nRepeated patterns (not savings estimates):\n");
    for p in report.patterns.iter().take(top) {
        out.push_str(&format!(
            "{} bytes\t{} occurrences\t{:?} statements each\t{}\t{}\n",
            p.covered_bytes, p.occurrences, p.statements_per_occurrence, p.policy, p.digest
        ));
        if p.unassigned_occurrences > 0 {
            out.push_str(&format!(
                "  unassigned to function scopes: {} occurrences, {} bytes\n",
                p.unassigned_occurrences, p.unassigned_covered_bytes
            ));
        }
        for scope in p.function_scopes.iter().take(top) {
            out.push_str(&format!(
                "  {}: {} occurrences, {} bytes\n",
                scope.name, scope.occurrences, scope.covered_bytes
            ));
        }
    }
    for caveat in &report.caveats {
        out.push_str(&format!("note: {caveat}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn many_interval_sets_match_independent_byte_masks() {
        let mut seed = 19u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            seed as usize
        };
        for _ in 0..2000 {
            let n = next() % 257;
            let mut spans = Vec::new();
            for _ in 0..next() % 65 {
                let a = next() % (n + 1);
                let b = next() % (n + 1);
                spans.push((a.min(b), a.max(b)));
            }
            let count = (0..n)
                .filter(|i| spans.iter().any(|(a, b)| a <= i && i < b))
                .count();
            assert_eq!(union_bytes(spans.clone()), count);
            spans.reverse();
            assert_eq!(union_bytes(spans), count);
        }
    }

    #[test]
    fn manifest_work_budget_bounds_overlapping_hash_work() {
        let bytes = vec![b'x'; 1024];
        let make = |count| RegionManifest {
            schema: "riffcat-regions/1".into(),
            adapter: "test/1".into(),
            artifact_blake3: blake3::hash(&bytes).to_hex().to_string(),
            regions: (0..count)
                .map(|i| RegionSpec {
                    id: i.to_string(),
                    kind: format!("kind-{i}"),
                    name: String::new(),
                    start: 0,
                    end: bytes.len(),
                })
                .collect(),
        };
        assert!(census_regions(&bytes, make(8)).is_ok());
        assert!(
            census_regions(&bytes, make(9))
                .unwrap_err()
                .to_string()
                .contains("work budget")
        );
        let mut bad = make(1);
        bad.regions[0].kind.clear();
        assert!(census_regions(&bytes, bad).is_err());
    }

    #[test]
    fn token_budget_rejects_instead_of_panicking() {
        assert!(lex_with_budget("a b c", 2).is_err());
        assert_eq!(lex_with_budget("a b", 2).unwrap().len(), 2);
    }

    #[test]
    fn actual_token_limit_rejects_a_generated_large_input() {
        let input = "x ".repeat(2_000_001);
        assert!(
            census_wgsl(&input)
                .unwrap_err()
                .to_string()
                .contains("token budget")
        );
    }

    #[test]
    fn function_reordering_preserves_pattern_and_region_content() {
        let a = "fn a() { s.a = x.b; }";
        let b = "fn b() { t.a = y.b; }";
        let left = census_wgsl(&format!("{a}\n{b}")).unwrap();
        let right = census_wgsl(&format!("{b}\n{a}")).unwrap();
        let signature = |r: &ArtifactCensus| {
            let mut regions: Vec<_> = r.regions.iter().map(|r| r.content_blake3.clone()).collect();
            regions.sort();
            let patterns: Vec<_> = r
                .patterns
                .iter()
                .map(|p| {
                    (
                        p.policy.clone(),
                        p.digest.clone(),
                        p.occurrences,
                        p.covered_bytes,
                    )
                })
                .collect();
            (regions, patterns)
        };
        assert_eq!(signature(&left), signature(&right));
    }

    #[test]
    fn shared_lean_bridge_vectors() {
        let input: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/census-bridge.json")).unwrap();
        for case in input["cases"].as_array().unwrap() {
            let ranges: Vec<(usize, usize)> =
                serde_json::from_value(case["ranges"].clone()).unwrap();
            let covered = union_bytes(ranges);
            assert_eq!(covered as u64, case["covered"].as_u64().unwrap());
            assert_eq!(
                case["bytes"].as_u64().unwrap() - covered as u64,
                case["outside"].as_u64().unwrap()
            );
        }
        assert_eq!(
            "s.a = x.b;".len() as u64,
            input["short_bytes"].as_u64().unwrap()
        );
        assert_eq!(
            "state.a = longer.b;".len() as u64,
            input["long_bytes"].as_u64().unwrap()
        );
    }

    #[test]
    fn report_round_trip_duplicate_ranges_and_overlapping_occurrences() {
        let bytes = b"aaaa";
        let region = |id: &str, start, end| RegionSpec {
            id: id.into(),
            kind: "part".into(),
            name: id.into(),
            start,
            end,
        };
        let mut manifest = RegionManifest {
            schema: "riffcat-regions/1".into(),
            artifact_blake3: blake3::hash(bytes).to_hex().to_string(),
            adapter: "test/1".into(),
            regions: vec![region("a", 0, 3), region("b", 1, 4)],
        };
        let report = census_regions(bytes, manifest.clone()).unwrap();
        assert_eq!(report.patterns[0].occurrences, 2);
        assert_eq!(report.patterns[0].covered_bytes, 4);
        assert_eq!(report.patterns[0].unassigned_occurrences, 2);
        assert_eq!(report.patterns[0].unassigned_covered_bytes, 4);
        let encoded = serde_json::to_vec(&report).unwrap();
        let decoded: ArtifactCensus = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), encoded);
        manifest.regions.push(region("alias", 0, 3));
        assert!(census_regions(bytes, manifest.clone()).is_err());
        manifest.regions.last_mut().unwrap().kind = "another-kind".into();
        let report = census_regions(bytes, manifest).unwrap();
        assert_eq!(report.patterns.len(), 1);
        assert_eq!(report.patterns[0].occurrences, 2);
    }

    #[test]
    fn interval_union_matches_finite_byte_set_exhaustively() {
        // Executable refinement check against the finite-set accounting spec.
        for n in 0..9 {
            for a in 0..=n {
                for b in a..=n {
                    for c in 0..=n {
                        for d in c..=n {
                            let expected = (0..n)
                                .filter(|i| (a..b).contains(i) || (c..d).contains(i))
                                .count();
                            let actual = union_bytes(vec![(a, b), (c, d)]);
                            assert_eq!(actual, expected);
                            assert_eq!(actual + (n - actual), n);
                            assert_eq!(union_bytes(vec![(c, d), (a, b), (a, b)]), actual);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn manifest_is_digest_bound_and_rejects_bad_ranges_and_ids() {
        let source = b"abcdefghij";
        let base = RegionManifest {
            schema: "riffcat-regions/1".into(),
            adapter: "test/1".into(),
            artifact_blake3: blake3::hash(source).to_hex().to_string(),
            regions: vec![
                RegionSpec {
                    id: "outer".into(),
                    kind: "function".into(),
                    name: "f".into(),
                    start: 1,
                    end: 9,
                },
                RegionSpec {
                    id: "inner".into(),
                    kind: "copy".into(),
                    name: "c".into(),
                    start: 2,
                    end: 4,
                },
            ],
        };
        let report = census_regions(source, base.clone()).unwrap();
        assert_eq!(report.region_union_bytes, 8);
        assert_eq!(report.outside_region_bytes, 2);
        assert!(census_regions(b"abcdefghik", base.clone()).is_err());
        for (start, end) in [(0, 11), (3, 3), (4, 2)] {
            let mut bad = base.clone();
            bad.regions[0].start = start;
            bad.regions[0].end = end;
            assert!(census_regions(source, bad).is_err());
        }
        let mut bad = base;
        bad.regions[1].id = "outer".into();
        assert!(census_regions(source, bad).is_err());
    }

    fn projected(report: &ArtifactCensus) -> Vec<&PatternGroup> {
        report
            .patterns
            .iter()
            .filter(|p| p.policy == WGSL_POLICY)
            .collect()
    }

    #[test]
    fn repeated_reconstruction_is_scoped_and_not_a_savings_claim() {
        let source = "// fn fake() { }\n@compute @workgroup_size(1)\nfn real() {\n  state.a[0] = first.r0;\n  state.b = first.r1;\n  let value = multiply();\n  state.a[0] = second.r0;\n  state.b = second.r1;\n}\n";
        let report = census_wgsl(source).unwrap();
        assert_eq!(
            report
                .regions
                .iter()
                .filter(|r| r.region.kind == "function")
                .count(),
            1
        );
        let patterns = projected(&report);
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].occurrences, 2);
        assert_eq!(patterns[0].statements_per_occurrence, Some(2));
        assert_eq!(
            patterns[0].covered_bytes,
            "  state.a[0] = first.r0;\n  state.b = first.r1;\n".len()
                + "  state.a[0] = second.r0;\n  state.b = second.r1;\n".len()
        );
        let again = census_wgsl(source).unwrap();
        assert_eq!(
            serde_json::to_vec(&report).unwrap(),
            serde_json::to_vec(&again).unwrap()
        );
    }

    #[test]
    fn member_indices_aliasing_and_statement_order_are_not_erased() {
        for alternative in [
            "s.a[1] = y.r0; s.b = y.r1;",
            "s.a[0] = y.r2; s.b = y.r1;",
            "s.b = y.r1; s.a[0] = y.r0;",
            "s.a[0] = s.r0; s.b = s.r1;",
        ] {
            let source = format!("fn f() {{ s.a[0] = x.r0; s.b = x.r1; call(); {alternative} }}");
            assert!(
                projected(&census_wgsl(&source).unwrap()).is_empty(),
                "{alternative}"
            );
        }
    }

    #[test]
    fn renamed_pattern_cannot_determine_exact_byte_cost() {
        let a = census_wgsl("fn f() { s.a = x.b; call(); s.a = y.b; }").unwrap();
        let b = census_wgsl("fn f() { state.a = longer.b; call(); state.a = another.b; }").unwrap();
        let pa = projected(&a);
        let pb = projected(&b);
        assert_eq!(pa[0].digest, pb[0].digest);
        assert_ne!(pa[0].covered_bytes, pb[0].covered_bytes);
    }

    #[test]
    fn comments_control_flow_and_non_projection_operations_do_not_fake_runs() {
        let report = census_wgsl("/* outer { /* fn fake() {} */ } */ fn f() { for (var i = 0; i < 2; i = i + 1) { a.b = call(); a.c += x.d; a.e = x.d + 1; a.f = x[i]; a.g = true; a.h = false; } }").unwrap();
        assert_eq!(report.regions.len(), 1);
        assert!(report.patterns.is_empty());
        for invalid in ["fn f() {", "/* unterminated", "}", "struct S {"] {
            assert!(census_wgsl(invalid).is_err());
        }
    }
}
