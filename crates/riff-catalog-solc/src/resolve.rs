use std::path::Path;

use crate::error::SolcError;
use crate::runner::SolcRunner;

/// Resolve a solc binary for an exactly-pinned version requirement (sourcify
/// metadata pins e.g. "0.8.26+commit.8a97fa7a"). v1 only checks the installed
/// binary against an accept range; an svm-style downloader backed by
/// argotorg/solc-bin can implement this trait later.
pub trait SolcResolver {
    fn resolve(&self, exact: &str) -> Result<SolcRunner, SolcError>;
}

pub struct InstalledSolc {
    pub accept: semver::VersionReq,
    explicit: Option<std::path::PathBuf>,
}

impl InstalledSolc {
    pub fn new(accept: semver::VersionReq, explicit: Option<&Path>) -> Self {
        Self {
            accept,
            explicit: explicit.map(Path::to_path_buf),
        }
    }
}

impl Default for InstalledSolc {
    fn default() -> Self {
        Self {
            accept: semver::VersionReq::parse("^0.8").expect("valid range"),
            explicit: None,
        }
    }
}

impl SolcResolver for InstalledSolc {
    fn resolve(&self, exact: &str) -> Result<SolcRunner, SolcError> {
        // Pinned version like "0.8.26+commit.8a97fa7a" — match its base
        // against the accept range AND the installed binary's range.
        let base = exact.split('+').next().unwrap_or(exact);
        let pinned =
            semver::Version::parse(base).map_err(|_| SolcError::VersionParse(exact.to_string()))?;
        if !self.accept.matches(&pinned) {
            return Err(SolcError::VersionMismatch {
                found: exact.to_string(),
                required: self.accept.to_string(),
            });
        }
        let runner = SolcRunner::locate(self.explicit.as_deref());
        let installed = runner.version()?;
        if !self.accept.matches(&installed) {
            return Err(SolcError::VersionMismatch {
                found: installed.to_string(),
                required: self.accept.to_string(),
            });
        }
        Ok(runner)
    }
}
