//! Sourcify HTTP API access, cache-first.
//!
//! Endpoint inventory (re-verify against docs.sourcify.dev when something
//! breaks — the v2 API is the current one):
//! - `GET {base}/v2/contract/{chainId}/{address}?fields=sources,metadata,stdJsonInput,compilation`
//!   → full verified-contract record.
//! - Legacy fallback: `GET {base}/files/any/{chainId}/{address}` → file list
//!   with metadata.json among them.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::Value;

use crate::error::SourcifyError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractId {
    pub chain_id: u64,
    /// normalized 0x-lowercase
    pub address: String,
}

impl ContractId {
    pub fn new(chain_id: u64, address: &str) -> Self {
        Self {
            chain_id,
            address: address.to_lowercase(),
        }
    }

    /// Parse "chainId:address".
    pub fn parse(text: &str) -> Option<Self> {
        let (chain, address) = text.split_once(':')?;
        Some(Self::new(chain.parse().ok()?, address))
    }
}

#[derive(Clone, Debug)]
pub struct VerifiedContract {
    pub id: ContractId,
    pub contract_name: String,
    /// exact pinned version, e.g. "0.8.26+commit.8a97fa7a"
    pub compiler_version: String,
    pub match_kind: String,
    pub std_json_input: Option<Value>,
    pub sources: BTreeMap<String, String>,
    pub metadata: Value,
}

pub struct SourcifyClient {
    pub base: String,
    pub cache_dir: PathBuf,
    agent: ureq::Agent,
}

impl SourcifyClient {
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            base: "https://sourcify.dev/server".to_string(),
            cache_dir: cache_dir.into(),
            agent: ureq::AgentBuilder::new()
                .user_agent("riff-catalog (https://github.com/micahscopes/riff-catalog)")
                .build(),
        }
    }

    fn cache_path(&self, id: &ContractId) -> PathBuf {
        self.cache_dir
            .join("sourcify")
            .join(id.chain_id.to_string())
            .join(format!("{}.json", id.address))
    }

    /// Fetch the raw v2 record, cache-first.
    fn fetch_raw(&self, id: &ContractId) -> Result<Value, SourcifyError> {
        let cache_path = self.cache_path(id);
        if let Ok(cached) = std::fs::read_to_string(&cache_path) {
            if let Ok(value) = serde_json::from_str(&cached) {
                return Ok(value);
            }
        }

        let url = format!(
            "{}/v2/contract/{}/{}?fields=sources,metadata,stdJsonInput,compilation",
            self.base, id.chain_id, id.address
        );
        let response = self.agent.get(&url).call().map_err(|error| match error {
            ureq::Error::Status(404, _) => SourcifyError::NotFound {
                chain_id: id.chain_id,
                address: id.address.clone(),
            },
            other => SourcifyError::Http(other.to_string()),
        })?;
        let value: Value = response
            .into_json()
            .map_err(|error| SourcifyError::Http(error.to_string()))?;

        if let Some(parent) = cache_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&cache_path, serde_json::to_string(&value)?)?;
        Ok(value)
    }

    pub fn fetch(&self, id: &ContractId) -> Result<VerifiedContract, SourcifyError> {
        let raw = self.fetch_raw(id)?;

        let contract_name = raw
            .pointer("/compilation/name")
            .or_else(|| raw.pointer("/name"))
            .and_then(Value::as_str)
            .unwrap_or("Unknown")
            .to_string();
        let compiler_version = raw
            .pointer("/compilation/compilerVersion")
            .or_else(|| raw.pointer("/compilerVersion"))
            .and_then(Value::as_str)
            .map(|version| version.trim_start_matches('v').to_string())
            .ok_or_else(|| SourcifyError::Shape("missing compiler version".into()))?;
        let match_kind = raw
            .pointer("/match")
            .or_else(|| raw.pointer("/matchId"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();

        let mut sources = BTreeMap::new();
        if let Some(map) = raw.pointer("/sources").and_then(Value::as_object) {
            for (path, entry) in map {
                let content = entry
                    .get("content")
                    .and_then(Value::as_str)
                    .or_else(|| entry.as_str())
                    .unwrap_or_default();
                sources.insert(path.clone(), content.to_string());
            }
        }

        let metadata = raw.pointer("/metadata").cloned().unwrap_or(Value::Null);
        let std_json_input = raw
            .pointer("/stdJsonInput")
            .filter(|value| value.is_object())
            .cloned();

        Ok(VerifiedContract {
            id: id.clone(),
            contract_name,
            compiler_version,
            match_kind,
            std_json_input,
            sources,
            metadata,
        })
    }
}
