use thiserror::Error;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct SolcDiagnostic {
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub message: String,
    #[serde(default, rename = "formattedMessage")]
    pub formatted: String,
}

#[derive(Debug, Error)]
pub enum SolcError {
    #[error("failed to spawn solc `{path}`: {source}")]
    Spawn {
        path: String,
        source: std::io::Error,
    },
    #[error("solc exited with {status}: {stderr}")]
    NonZeroExit { status: i32, stderr: String },
    #[error("solc reported errors:\n{}", .0.iter().map(|d| d.formatted.as_str()).collect::<Vec<_>>().join("\n"))]
    Diagnostics(Vec<SolcDiagnostic>),
    #[error("solc output missing `{0}`")]
    MissingOutput(String),
    #[error("could not parse solc version from `{0}`")]
    VersionParse(String),
    #[error("unsupported solc version {found}, need {required}")]
    VersionMismatch { found: String, required: String },
    #[error("invalid bytecode hex: {0}")]
    BytecodeHex(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
