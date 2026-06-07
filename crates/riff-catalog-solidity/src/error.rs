use thiserror::Error;

#[derive(Debug, Error)]
pub enum SolLowerError {
    #[error("unknown nodeType `{0}` (strict mode)")]
    UnknownNodeType(String),
    #[error("AST node missing nodeType at {0}")]
    MissingNodeType(String),
    #[error("expected SourceUnit at the AST root, found `{0}`")]
    NotASourceUnit(String),
    #[error(transparent)]
    Core(#[from] riff_catalog_core::CatalogError),
    #[error("literal: {0}")]
    Literal(#[from] riff_catalog_yul::YulLowerError),
}
