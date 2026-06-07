//! Exact-version solc resolution backed by binaries.soliditylang.org (the
//! same artifact store the argotorg/solc-bin repo publishes). Downloads are
//! cached, so a pinned contract costs one ~8 MB fetch ever.
//!
//! This is what turns "skipped: needs solc 0.8.17" into a faithful
//! recompilation with the *same compiler the contract was verified with* —
//! strictly better than range-matching against whatever is installed.

use std::path::PathBuf;

use riff_catalog_solc::{SolcError, SolcResolver, SolcRunner};

/// Hard-coded to the platform we run demos on; other targets exist upstream
/// (macosx-amd64, windows-amd64) if this ever needs to be portable.
const PLATFORM: &str = "linux-amd64";
const BASE_URL: &str = "https://binaries.soliditylang.org";

pub struct SolcBinResolver {
    cache_dir: PathBuf,
    agent: ureq::Agent,
}

impl SolcBinResolver {
    pub fn new(cache_dir: impl Into<PathBuf>) -> Self {
        Self {
            cache_dir: cache_dir.into(),
            agent: ureq::AgentBuilder::new()
                .user_agent("riff-catalog (https://github.com/micahscopes/riff-catalog)")
                .build(),
        }
    }

    fn binary_path(&self, exact: &str) -> PathBuf {
        self.cache_dir
            .join("solc-bin")
            .join(format!("solc-{PLATFORM}-v{exact}"))
    }

    fn download(&self, exact: &str) -> Result<PathBuf, SolcError> {
        let path = self.binary_path(exact);
        if path.exists() {
            return Ok(path);
        }
        let url = format!("{BASE_URL}/{PLATFORM}/solc-{PLATFORM}-v{exact}");
        let response = self
            .agent
            .get(&url)
            .call()
            .map_err(|error| SolcError::Resolver(format!("download {url}: {error}")))?;
        let mut bytes = Vec::new();
        response
            .into_reader()
            .read_to_end(&mut bytes)
            .map_err(|error| SolcError::Resolver(format!("reading {url}: {error}")))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Write to a temp name then rename: a killed download must not leave
        // a half-binary at the cached path.
        let staging = path.with_extension("part");
        std::fs::write(&staging, &bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o755))?;
        }
        std::fs::rename(&staging, &path)?;
        Ok(path)
    }
}

impl SolcResolver for SolcBinResolver {
    fn resolve(&self, exact: &str) -> Result<SolcRunner, SolcError> {
        let path = self.download(exact)?;
        let runner = SolcRunner::locate(Some(&path));
        // Sanity: the downloaded binary must report the pinned base version.
        let pinned_base = exact.split('+').next().unwrap_or(exact);
        let reported = runner.version()?;
        if reported.to_string() != pinned_base {
            return Err(SolcError::VersionMismatch {
                found: reported.to_string(),
                required: pinned_base.to_string(),
            });
        }
        Ok(runner)
    }
}

/// Try the locally-installed solc when its base version matches the pin
/// exactly; otherwise download the pinned build. (A pre-release nightly
/// never matches an exact pragma pin, so in practice pins download.)
pub struct PinnedOrDownload {
    pub download: SolcBinResolver,
    pub installed: Option<std::path::PathBuf>,
}

impl SolcResolver for PinnedOrDownload {
    fn resolve(&self, exact: &str) -> Result<SolcRunner, SolcError> {
        let pinned_base = exact.split('+').next().unwrap_or(exact);
        let installed = SolcRunner::locate(self.installed.as_deref());
        if let Ok(version) = installed.version() {
            if version.to_string() == pinned_base && version.pre.is_empty() {
                return Ok(installed);
            }
        }
        self.download.resolve(exact)
    }
}
