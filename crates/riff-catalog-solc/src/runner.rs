use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest as _, Sha256};

use crate::error::SolcError;
use crate::output::SolcOutput;

/// Resolution order: explicit path > $RIFFCAT_SOLC > $FE_SOLC_PATH > `solc`
/// on PATH.
pub struct SolcRunner {
    path: PathBuf,
}

impl SolcRunner {
    pub fn locate(explicit: Option<&Path>) -> Self {
        let path = explicit
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os("RIFFCAT_SOLC").map(PathBuf::from))
            .or_else(|| std::env::var_os("FE_SOLC_PATH").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("solc"));
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Parse `solc --version`, stripping any pre-release suffix so semver
    /// ranges match (e.g. "0.8.31-pre.1+commit..." -> 0.8.31).
    pub fn version(&self) -> Result<semver::Version, SolcError> {
        let output = Command::new(&self.path)
            .arg("--version")
            .output()
            .map_err(|source| SolcError::Spawn {
                path: self.path.display().to_string(),
                source,
            })?;
        let text = String::from_utf8_lossy(&output.stdout);
        let line = text
            .lines()
            .find(|line| line.starts_with("Version:"))
            .ok_or_else(|| SolcError::VersionParse(text.to_string()))?;
        // "Version: 0.8.31-pre.1+commit.b59566f6.Linux.g++"
        let raw = line
            .trim_start_matches("Version:")
            .trim()
            .split('+')
            .next()
            .unwrap_or_default();
        let base = raw.split('-').next().unwrap_or_default();
        semver::Version::parse(base).map_err(|_| SolcError::VersionParse(line.to_string()))
    }

    /// Run `solc --standard-json` with `input` on stdin.
    pub fn compile(&self, input: &serde_json::Value) -> Result<SolcOutput, SolcError> {
        let raw = self.compile_raw(input)?;
        Ok(SolcOutput::new(serde_json::from_str(&raw)?))
    }

    fn compile_raw(&self, input: &serde_json::Value) -> Result<String, SolcError> {
        let mut child = Command::new(&self.path)
            .arg("--standard-json")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| SolcError::Spawn {
                path: self.path.display().to_string(),
                source,
            })?;
        child
            .stdin
            .take()
            .expect("stdin piped")
            .write_all(input.to_string().as_bytes())?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err(SolcError::NonZeroExit {
                status: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

/// Disk cache over [`SolcRunner`]: key = sha256(canonical input + version),
/// value = raw stdout at `dir/<key>.json`. Makes corpus re-runs offline.
pub struct CachedSolc {
    pub inner: SolcRunner,
    pub dir: PathBuf,
}

impl CachedSolc {
    pub fn new(inner: SolcRunner, dir: impl Into<PathBuf>) -> Self {
        Self {
            inner,
            dir: dir.into(),
        }
    }

    pub fn compile(&self, input: &serde_json::Value) -> Result<SolcOutput, SolcError> {
        let version = self.inner.version()?;
        let mut hasher = Sha256::new();
        hasher.update(input.to_string().as_bytes());
        hasher.update(version.to_string().as_bytes());
        let key = hex::encode(hasher.finalize());
        let path = self.dir.join(format!("{key}.json"));

        if let Ok(cached) = std::fs::read_to_string(&path) {
            if let Ok(value) = serde_json::from_str(&cached) {
                return Ok(SolcOutput::new(value));
            }
        }

        let output = self.inner.compile(input)?;
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(&path, output.raw().to_string())?;
        Ok(output)
    }
}
