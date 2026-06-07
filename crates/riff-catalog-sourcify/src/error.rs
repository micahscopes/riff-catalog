use thiserror::Error;

#[derive(Debug, Error)]
pub enum SourcifyError {
    #[error("http: {0}")]
    Http(String),
    #[error("contract not found or not verified: chain {chain_id}, {address}")]
    NotFound { chain_id: u64, address: String },
    #[error("unexpected response shape: {0}")]
    Shape(String),
    #[error(transparent)]
    Solc(riff_catalog_solc::SolcError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
