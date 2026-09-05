use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use crate::{Capture, CaptureFile, SCHEMA_VERSION, validate};

pub fn artifact_digest(path: &Path) -> Result<(String, u64)> {
    let mut file =
        fs::File::open(path).with_context(|| format!("open artifact {}", path.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut bytes = 0u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes = bytes
            .checked_add(u64::try_from(read)?)
            .context("artifact byte count overflows")?;
    }
    Ok((hasher.finalize().to_hex().to_string(), bytes))
}

pub fn capture_id(capture: &Capture) -> Result<String> {
    capture_id_for_schema(capture, SCHEMA_VERSION)
}

pub(crate) fn capture_id_for_schema(capture: &Capture, schema: &str) -> Result<String> {
    let body = serde_json::to_vec(capture)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"riff-catalog-bloat:capture-id\0");
    hasher.update(schema.as_bytes());
    hasher.update(b"\0");
    hasher.update(&body);
    Ok(hasher.finalize().to_hex().to_string())
}

pub fn save_capture(path: &Path, capture: Capture) -> Result<CaptureFile> {
    let file = CaptureFile {
        schema: SCHEMA_VERSION.to_owned(),
        capture_id: capture_id(&capture)?,
        capture,
    };
    validate(&file)?;
    let bytes = serde_json::to_vec_pretty(&file)?;
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut output) => output
            .write_all(&bytes)
            .with_context(|| format!("write capture {}", path.display()))?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing = load_capture(path)?;
            if existing.schema != file.schema || existing.capture_id != file.capture_id {
                bail!("refusing to overwrite immutable capture {}", path.display());
            }
        }
        Err(error) => {
            return Err(error).with_context(|| format!("create capture {}", path.display()));
        }
    }
    Ok(file)
}

pub fn load_capture(path: &Path) -> Result<CaptureFile> {
    const MAX_CAPTURE_BYTES: u64 = 256 * 1024 * 1024;
    let metadata =
        fs::metadata(path).with_context(|| format!("stat capture {}", path.display()))?;
    if metadata.len() > MAX_CAPTURE_BYTES {
        bail!("capture exceeds {MAX_CAPTURE_BYTES} byte limit");
    }
    let bytes = fs::read(path).with_context(|| format!("read capture {}", path.display()))?;
    let file: CaptureFile = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse capture {}", path.display()))?;
    validate(&file)?;
    Ok(file)
}

fn resolve_artifact(capture_path: &Path, artifact_path: &str) -> PathBuf {
    let path = Path::new(artifact_path);
    if path.is_absolute() {
        path.to_owned()
    } else {
        capture_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(path)
    }
}

pub fn verify_artifacts(capture_path: &Path, file: &CaptureFile) -> Result<()> {
    for artifact in &file.capture.artifacts {
        let path = resolve_artifact(capture_path, &artifact.path);
        let (digest, bytes) = artifact_digest(&path)?;
        if digest != artifact.blake3 || bytes != artifact.bytes {
            bail!(
                "artifact `{}` failed verification: expected {} bytes blake3 {}, got {} bytes blake3 {}",
                artifact.id,
                artifact.bytes,
                artifact.blake3,
                bytes,
                digest
            );
        }
    }
    Ok(())
}
