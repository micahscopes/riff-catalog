use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
#[error("yul parse error at {line}:{col}: {message}")]
pub struct YulParseError {
    pub line: u32,
    pub col: u32,
    pub message: String,
}

impl YulParseError {
    pub(crate) fn new(line: u32, col: u32, message: impl Into<String>) -> Self {
        Self {
            line,
            col,
            message: message.into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum YulLowerError {
    #[error("invalid number literal `{0}`")]
    InvalidNumber(String),
    #[error("invalid string literal hex `{0}`")]
    InvalidStringHex(String),
    #[error(transparent)]
    Core(#[from] riff_catalog_core::CatalogError),
    #[error("ssa cfg json: {0}")]
    SsaJson(#[from] serde_json::Error),
    #[error("ssa cfg: {0}")]
    SsaShape(String),
}
