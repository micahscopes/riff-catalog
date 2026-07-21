use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum CatalogError {
    #[error("{field} must not be empty")]
    EmptyText { field: &'static str },
    #[error("{field} must not contain the unit separator (\\u{{1f}})")]
    InvalidText { field: &'static str },
    #[error("a facet must include at least one dimension")]
    EmptyDimensions,
    #[error("digest must be 64 lowercase hex characters")]
    InvalidDigest,
    #[error("unsupported digest algorithm {algorithm}")]
    UnsupportedAlgorithm { algorithm: &'static str },
    #[error("unsupported schema version {found}, this build speaks {supported}")]
    SchemaVersionMismatch { found: u32, supported: u32 },
    #[error("duplicate node key {key}")]
    DuplicateNode { key: String },
    #[error("missing node key {key}")]
    MissingNode { key: String },
    #[error("node stored under {map_key} but contains key {node_key}")]
    NodeKeyMismatch { map_key: String, node_key: String },
    #[error("edge payload fields are only supported on origin edges")]
    EdgeFieldsRequireOrigin,
    #[error("digest requested for graph {requested} but got {actual}")]
    GraphKeyMismatch { requested: String, actual: String },
    #[error("index policy {actual} does not match {expected}")]
    IndexPolicyMismatch { expected: String, actual: String },
    #[error("facet {actual} does not match {expected}")]
    FacetMismatch { expected: String, actual: String },
    #[error("facet address dimensions do not match the facet's dimension set")]
    FacetDimensionMismatch,
    #[error("graph hashes are missing dimension {dimension} required by the facet")]
    MissingDimension { dimension: &'static str },
    #[error("graph contains a cycle at {key}")]
    CycleDetected { key: String },
    #[error("invalid artifact reference: {reason}")]
    InvalidReference { reason: String },
}
