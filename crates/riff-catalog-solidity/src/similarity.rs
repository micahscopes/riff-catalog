//! Bounded content postings and explanatory subtree overlap.
//! Overlap is a collection of independent exact fragment matches, not a claim
//! that all fragments admit one consistent whole-region binding substitution.
use crate::selection::{Document, NormalAst, Selection, SourceArtifact, Span, View};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub fn content_id(normal: &NormalAst) -> Result<String> {
    let bytes = serde_json::to_vec(normal)?;
    let mut hash = blake3::Hasher::new_derive_key("riffcat exact selected Solidity syntax v1");
    hash.update(&bytes);
    Ok(format!("solsyntax1:{}", hash.finalize().to_hex()))
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct IndexBudget {
    pub occurrences: usize,
    pub normalized_nodes: usize,
    pub serialized_bytes: usize,
}
impl Default for IndexBudget {
    fn default() -> Self {
        Self {
            occurrences: 100_000,
            normalized_nodes: 1_000_000,
            serialized_bytes: 64 * 1024 * 1024,
        }
    }
}

struct Entry {
    file: usize,
    selection: Selection,
    address: String,
}

pub struct SyntaxIndex {
    artifacts: Vec<SourceArtifact>,
    entries: Vec<Entry>,
    postings: BTreeMap<String, Vec<usize>>,
    view: View,
    pub unsupported: Vec<Value>,
    pub truncated: bool,
    pub normalized_nodes: usize,
    pub serialized_bytes: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct FragmentMatch {
    pub content_id: String,
    pub query: Span,
    pub candidate: Span,
    pub query_tokens: usize,
    pub candidate_tokens: usize,
    pub query_bindings: Vec<i64>,
    pub candidate_bindings: Vec<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Overlap {
    pub file: usize,
    pub path: String,
    pub span: Span,
    pub whole_match: bool,
    pub query_covered_tokens: usize,
    pub query_total_tokens: usize,
    pub candidate_covered_tokens: usize,
    pub candidate_total_tokens: usize,
    pub query_unmatched: Vec<Span>,
    pub candidate_unmatched: Vec<Span>,
    pub fragments: Vec<FragmentMatch>,
}

#[derive(Serialize)]
pub struct OverlapReport {
    pub schema: String,
    pub query: Selection,
    pub query_content_id: String,
    pub results: Vec<Overlap>,
    pub unsupported: Vec<Value>,
    pub index_occurrences: usize,
    pub normalized_nodes: usize,
    pub serialized_bytes: usize,
    pub candidates_considered: usize,
    pub truncated: bool,
    pub alignment: &'static str,
}

fn contains(outer: &Span, inner: &Span) -> bool {
    outer.file == inner.file
        && outer.start <= inner.start
        && inner.start.saturating_add(inner.length) <= outer.start.saturating_add(outer.length)
}
fn intersects(a: &Span, b: &Span) -> bool {
    a.file == b.file
        && a.start < b.start.saturating_add(b.length)
        && b.start < a.start.saturating_add(a.length)
}

/// Small lexical-unit scanner used only for coverage, never for syntax identity.
/// Comments/whitespace are omitted; quoted strings include escaped delimiters.
pub fn tokens(source: &str, file: i64) -> Vec<Span> {
    let mut result = Vec::new();
    let mut cursor = 0;
    while cursor < source.len() {
        let rest = &source[cursor..];
        let c = rest.chars().next().unwrap();
        if c.is_whitespace() {
            cursor += c.len_utf8();
            continue;
        }
        if rest.starts_with("//") {
            cursor += rest.find('\n').unwrap_or(rest.len());
            continue;
        }
        if rest.starts_with("/*") {
            cursor += rest.find("*/").map_or(rest.len(), |n| n + 2);
            continue;
        }
        let start = cursor;
        if c == '\'' || c == '"' {
            cursor += 1;
            while cursor < source.len() {
                let next = source[cursor..].chars().next().unwrap();
                cursor += next.len_utf8();
                if next == '\\' {
                    if let Some(escaped) = source[cursor..].chars().next() {
                        cursor += escaped.len_utf8();
                    }
                } else if next == c {
                    break;
                }
            }
        } else if c.is_alphanumeric() || c == '_' || c == '$' {
            cursor += rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$')
                .map(char::len_utf8)
                .sum::<usize>();
        } else {
            let op = [
                ">>=", "<<=", "**=", "==", "!=", "<=", ">=", "&&", "||", "++", "--", "+=", "-=",
                "*=", "/=", "%=", "&=", "|=", "^=", "<<", ">>", "**", "=>", ":=",
            ]
            .into_iter()
            .find(|op| rest.starts_with(op));
            cursor += op.map_or(c.len_utf8(), str::len);
        }
        result.push(Span {
            start,
            length: cursor - start,
            file,
        });
    }
    result
}

impl SyntaxIndex {
    pub fn build(artifacts: &[SourceArtifact], view: View, budget: IndexBudget) -> Result<Self> {
        ensure!(artifacts.len() <= 100, "source corpus exceeds 100 files");
        let mut index = Self {
            artifacts: artifacts.to_vec(),
            entries: Vec::new(),
            postings: BTreeMap::new(),
            view,
            unsupported: Vec::new(),
            truncated: false,
            normalized_nodes: 0,
            serialized_bytes: 0,
        };
        let mut visited = 0;
        for (file, artifact) in artifacts.iter().enumerate() {
            let doc = Document::new(&artifact.ast)?;
            for (id, _) in doc.candidates() {
                visited += 1;
                if visited > budget.occurrences {
                    index.truncated = true;
                    return Ok(index);
                }
                match doc.normalize(id, view) {
                    Err(error) => index
                        .unsupported
                        .push(json!({"file":file,"id":id,"reason":error.to_string()})),
                    Ok(selection) => {
                        let span = &selection.span;
                        ensure!(
                            artifact
                                .source
                                .get(
                                    span.start
                                        ..span
                                            .start
                                            .checked_add(span.length)
                                            .context("span overflow")?
                                )
                                .is_some(),
                            "invalid source span"
                        );
                        let bytes = serde_json::to_vec(&selection.normal)?.len();
                        if index.normalized_nodes + selection.normal.nodes.len()
                            > budget.normalized_nodes
                            || index.serialized_bytes + bytes > budget.serialized_bytes
                        {
                            index.truncated = true;
                            return Ok(index);
                        }
                        index.normalized_nodes += selection.normal.nodes.len();
                        index.serialized_bytes += bytes;
                        let address = content_id(&selection.normal)?;
                        index
                            .postings
                            .entry(address.clone())
                            .or_default()
                            .push(index.entries.len());
                        index.entries.push(Entry {
                            file,
                            selection,
                            address,
                        });
                    }
                }
            }
        }
        Ok(index)
    }

    pub fn overlap(
        &self,
        query_file: usize,
        start: usize,
        end: usize,
        minimum_fragment_nodes: usize,
        candidate_limit: usize,
    ) -> Result<OverlapReport> {
        ensure!(
            minimum_fragment_nodes > 0 && candidate_limit > 0 && candidate_limit <= 200,
            "invalid overlap limits"
        );
        let artifact = self
            .artifacts
            .get(query_file)
            .context("query file out of range")?;
        ensure!(
            artifact.source.get(start..end).is_some(),
            "invalid UTF-8 selection"
        );
        let doc = Document::new(&artifact.ast)?;
        let query = doc.normalize(doc.select_range(start, end)?, self.view)?;
        let query_content_id = content_id(&query.normal)?;
        let qtokens: Vec<_> = tokens(&artifact.source, query.span.file)
            .into_iter()
            .filter(|t| contains(&query.span, t))
            .collect();
        let mut votes: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
        // Postings retrieve shared fragments, not all-pairs graph comparison.
        let roots: Vec<_> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.selection.kind == query.kind)
            .collect();
        let mut pair_work = 0usize;
        let mut pairing_truncated = false;
        'pairing: for (qi, q) in self.entries.iter().enumerate().filter(|(_, e)| {
            e.file == query_file
                && contains(&query.span, &e.selection.span)
                && (e.selection.normal.nodes.len() >= minimum_fragment_nodes
                    || e.selection.id == query.id)
        }) {
            for &ci in &self.postings[&q.address] {
                let c = &self.entries[ci];
                if c.selection.normal != q.selection.normal {
                    continue;
                } // verify even digest hits
                for &(ri, root) in &roots {
                    pair_work += 1;
                    if pair_work > 1_000_000 {
                        pairing_truncated = true;
                        break 'pairing;
                    }
                    if root.file == c.file && contains(&root.selection.span, &c.selection.span) {
                        if self.artifacts[root.file].source == artifact.source
                            && root.selection.span == query.span
                        {
                            continue;
                        }
                        votes.entry(ri).or_default().push((qi, ci));
                    }
                }
            }
        }
        let candidates_considered = votes.len();
        let mut results = Vec::new();
        for (ri, mut pairs) in votes {
            let root = &self.entries[ri];
            let ctokens: Vec<_> =
                tokens(&self.artifacts[root.file].source, root.selection.span.file)
                    .into_iter()
                    .filter(|t| contains(&root.selection.span, t))
                    .collect();
            // Deterministic greedy alignment, not optimal edit distance.
            pairs.sort_by_key(|&(q, c)| {
                (
                    std::cmp::Reverse(
                        qtokens
                            .iter()
                            .filter(|t| contains(&self.entries[q].selection.span, t))
                            .count(),
                    ),
                    self.entries[q].selection.span.start,
                    self.entries[c].selection.span.start,
                )
            });
            let mut fragments: Vec<FragmentMatch> = Vec::new();
            for (qi, ci) in pairs {
                let q = &self.entries[qi];
                let c = &self.entries[ci];
                if fragments.iter().any(|m| {
                    intersects(&m.query, &q.selection.span)
                        || intersects(&m.candidate, &c.selection.span)
                }) {
                    continue;
                }
                fragments.push(FragmentMatch {
                    content_id: q.address.clone(),
                    query: q.selection.span.clone(),
                    candidate: c.selection.span.clone(),
                    query_tokens: qtokens
                        .iter()
                        .filter(|t| contains(&q.selection.span, t))
                        .count(),
                    candidate_tokens: ctokens
                        .iter()
                        .filter(|t| contains(&c.selection.span, t))
                        .count(),
                    query_bindings: q.selection.external_bindings.clone(),
                    candidate_bindings: c.selection.external_bindings.clone(),
                });
            }
            let query_unmatched: Vec<_> = qtokens
                .iter()
                .filter(|t| !fragments.iter().any(|m| contains(&m.query, t)))
                .cloned()
                .collect();
            let candidate_unmatched: Vec<_> = ctokens
                .iter()
                .filter(|t| !fragments.iter().any(|m| contains(&m.candidate, t)))
                .cloned()
                .collect();
            results.push(Overlap {
                file: root.file,
                path: self.artifacts[root.file].path.clone(),
                span: root.selection.span.clone(),
                whole_match: root.selection.normal == query.normal,
                query_covered_tokens: qtokens.len() - query_unmatched.len(),
                query_total_tokens: qtokens.len(),
                candidate_covered_tokens: ctokens.len() - candidate_unmatched.len(),
                candidate_total_tokens: ctokens.len(),
                query_unmatched,
                candidate_unmatched,
                fragments,
            });
        }
        results.sort_by(|a, b| {
            b.whole_match
                .cmp(&a.whole_match)
                .then(b.query_covered_tokens.cmp(&a.query_covered_tokens))
                .then(a.path.cmp(&b.path))
                .then(a.span.start.cmp(&b.span.start))
        });
        let truncated = self.truncated || pairing_truncated || results.len() > candidate_limit;
        results.truncate(candidate_limit);
        Ok(OverlapReport {
            schema: "riffcat-source-overlap/1".into(),
            query,
            query_content_id,
            results,
            unsupported: self.unsupported.clone(),
            index_occurrences: self.entries.len(),
            normalized_nodes: self.normalized_nodes,
            serialized_bytes: self.serialized_bytes,
            candidates_considered,
            truncated,
            alignment: "greedy nonoverlapping one-to-one fragments; independent boundary substitutions; lexical-unit coverage, not semantic similarity",
        })
    }
}
