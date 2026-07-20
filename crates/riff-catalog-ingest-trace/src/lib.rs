//! riff-catalog-ingest-trace: turn an fe origin/provenance trace bundle into
//! riff-catalog graphs, with no hashing engine in sight.
//!
//! fe emits a JSONL trace bundle: one JSON object per line, each tagged with a
//! `record` discriminator (`"fact"` or `"metadata"`) and, for facts, a `type`
//! naming the fact kind. This reader ingests the ORIGIN graph only, the two fact
//! kinds that carry provenance:
//!
//! - `origin_node` carries a `key` (fe's `OriginExportKey`). It parses into an
//!   [`EntityKey`] through the `owner_key` / `local_key` serde aliases the schema
//!   crate already declares, and becomes a riff [`Node`](riff_catalog_schema::Node)
//!   keyed by [`NodeKey::Entity`].
//! - `origin_edge` carries `from`, `to`, `label`, and an optional `introduced_by`
//!   phase. It becomes a riff [`Edge`](riff_catalog_schema::Edge) with
//!   [`EdgeRole::Origin`], its `label` in the edge label slot and its
//!   `introduced_by` phase as an edge payload field.
//!
//! Every other fact kind (`instruction`, `storage`, `source_span`, `block`,
//! `shape_node_hash`, …) and every unknown kind is skipped, not an error: fe's
//! stream carries many facts this reader does not model.
//!
//! ## Grouping: one origin graph
//!
//! fe's origin edges cross owner boundaries (a lowered `runtime.stmt` in one
//! owner points at the `hir.expr` it lowered from in another), and a riff
//! [`Graph`] requires both endpoints of an edge to live in the same graph. The
//! bundle carries no per-unit partition that keeps every edge intra-graph, so
//! this reader produces exactly ONE origin graph spanning the whole bundle
//! rather than inventing a partition the data does not describe. The graph is
//! keyed off the bundle's `input_path` when the metadata record supplies one.
//!
//! ## Provenance is inert to shape
//!
//! Origin edges (and their payload fields) never move a facet address: the
//! engine's structural fold excludes `EdgeRole::Origin` edges entirely. Ingesting
//! the origin graph therefore attaches a full provenance topology to the riff
//! form without perturbing the fingerprint the catalog dedups on. This crate does
//! no hashing itself; hand the returned graphs to `riff_catalog::ingest_graph`.

use serde::Deserialize;

use riff_catalog_schema::{
    CatalogError, Dimension, EdgeRole, EntityKey, Field, Graph, GraphKey, NodeKey,
};

/// `kind` of the synthesized [`EntityKey`] that owns the single origin graph.
const ORIGIN_GRAPH_KIND: &str = "fe.origin.bundle";
/// Owner used when the bundle carries no `input_path` metadata.
const ORIGIN_GRAPH_DEFAULT_OWNER: &str = "bundle";
/// `local` of the synthesized graph key and its owning entity.
const ORIGIN_GRAPH_LOCAL: &str = "origins";
/// Field name that carries an origin edge's introducing compiler phase.
const INTRODUCED_BY_FIELD: &str = "introduced_by";

/// Error raised while reading a trace bundle.
#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    /// A line was not valid JSON, or an origin record did not match its shape.
    #[error("trace bundle line {line}: invalid JSON: {source}")]
    Json {
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    /// A well-formed origin record held a value the schema rejects (an empty
    /// key part, a separator-bearing name, a duplicate node, …).
    #[error("trace bundle line {line}: {source}")]
    Record {
        line: usize,
        #[source]
        source: CatalogError,
    },
    /// Building the origin graph's own key failed.
    #[error("building origin graph key: {0}")]
    GraphKey(#[source] CatalogError),
}

/// One `origin_node` record's payload. Extra line fields (`record`, `type`) are
/// ignored; `key` parses into an [`EntityKey`] via the schema's serde aliases.
#[derive(Deserialize)]
struct OriginNodeLine {
    key: EntityKey,
}

/// One `origin_edge` record's payload. `introduced_by` is optional (fe's
/// `Option<CompilerPhase>`); absent or null both read as `None`.
#[derive(Deserialize)]
struct OriginEdgeLine {
    from: EntityKey,
    to: EntityKey,
    label: String,
    #[serde(default)]
    introduced_by: Option<String>,
}

/// The synthesized key of the single origin graph for a bundle with the given
/// `input_path` (or the default owner when the bundle names no input).
fn origin_graph_key(input_path: Option<&str>) -> Result<GraphKey, CatalogError> {
    let owner = input_path.unwrap_or(ORIGIN_GRAPH_DEFAULT_OWNER);
    GraphKey::new(
        EntityKey::new(ORIGIN_GRAPH_KIND, owner, ORIGIN_GRAPH_LOCAL)?,
        ORIGIN_GRAPH_LOCAL,
    )
}

/// Ensure an entity is present as a node, keyed by [`NodeKey::Entity`] with its
/// `kind` as the node kind. Idempotent, so a key that appears as a node and as
/// an edge endpoint is added once.
fn ensure_node(graph: &mut Graph, key: &EntityKey) -> Result<(), CatalogError> {
    let node_key = NodeKey::entity(key.clone());
    if !graph.nodes.contains_key(&node_key) {
        graph.add_node(node_key, key.kind())?;
    }
    Ok(())
}

/// Read an fe origin/provenance trace bundle (JSONL) into riff graphs.
///
/// Returns one [`Graph`] holding the bundle's whole origin graph, or an empty
/// vector when the bundle carries no origin facts. Non-origin and unknown record
/// kinds are skipped. See the module docs for the mapping and the one-graph
/// grouping rationale.
pub fn ingest_trace_bundle(jsonl: &str) -> Result<Vec<Graph>, IngestError> {
    let mut input_path: Option<String> = None;
    let mut nodes: Vec<(usize, EntityKey)> = Vec::new();
    let mut edges: Vec<(usize, OriginEdgeLine)> = Vec::new();

    for (index, raw) in jsonl.lines().enumerate() {
        let line = index + 1;
        let text = raw.trim();
        if text.is_empty() {
            continue;
        }
        let value: serde_json::Value =
            serde_json::from_str(text).map_err(|source| IngestError::Json { line, source })?;

        if value.get("record").and_then(|r| r.as_str()) == Some("metadata") {
            if let Some(path) = value.get("input_path").and_then(|p| p.as_str()) {
                input_path = Some(path.to_string());
            }
            continue;
        }

        match value.get("type").and_then(|t| t.as_str()) {
            Some("origin_node") => {
                let node: OriginNodeLine = serde_json::from_value(value)
                    .map_err(|source| IngestError::Json { line, source })?;
                nodes.push((line, node.key));
            }
            Some("origin_edge") => {
                let edge: OriginEdgeLine = serde_json::from_value(value)
                    .map_err(|source| IngestError::Json { line, source })?;
                edges.push((line, edge));
            }
            // Every other (or unknown) fact kind is intentionally skipped: this
            // reader ingests the origin graph only.
            _ => {}
        }
    }

    if nodes.is_empty() && edges.is_empty() {
        return Ok(Vec::new());
    }

    let graph_key = origin_graph_key(input_path.as_deref()).map_err(IngestError::GraphKey)?;
    let mut graph = Graph::new(graph_key);

    for (line, key) in nodes {
        ensure_node(&mut graph, &key).map_err(|source| IngestError::Record { line, source })?;
    }

    for (line, edge) in edges {
        let record = |source| IngestError::Record { line, source };
        ensure_node(&mut graph, &edge.from).map_err(record)?;
        ensure_node(&mut graph, &edge.to).map_err(record)?;
        let source = NodeKey::entity(edge.from);
        let target = NodeKey::entity(edge.to);
        // Provenance payload rides on the edge: the fe label fills the edge's
        // label slot, and the introducing phase becomes a payload field. Both
        // are inert to every facet address (Origin edges are excluded from the
        // structural fold), so the field's dimension is immaterial; Names is
        // used because the phase is a name-like token.
        let mut fields = Vec::new();
        if let Some(phase) = edge.introduced_by {
            fields.push(Field::new(Dimension::Names, INTRODUCED_BY_FIELD, phase).map_err(record)?);
        }
        graph
            .add_edge_with_fields(&source, edge.label, &target, EdgeRole::Origin, fields)
            .map_err(record)?;
    }

    Ok(vec![graph])
}
