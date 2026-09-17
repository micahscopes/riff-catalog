//! Native capture and transport only. Selection and matching live in the adapter.
use anyhow::{Context, Result, ensure};
use riff_catalog_solc::SolcRunner;
use riff_catalog_solidity::selection::{SourceArtifact, View, search};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
struct Bundle {
    schema: String,
    compiler: Value,
    artifacts: Vec<SourceArtifact>,
    captures: Vec<Value>,
}

fn write_new(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    writeln!(file)?;
    Ok(())
}

pub fn capture(paths: &[PathBuf], output: &Path, solc: Option<&Path>) -> Result<()> {
    ensure!(
        !paths.is_empty() && paths.len() <= 100,
        "capture requires 1 to 100 source files"
    );
    ensure!(!output.exists(), "output already exists");
    let runner = SolcRunner::locate(solc);
    let version = std::process::Command::new(runner.path())
        .arg("--version")
        .output()?;
    ensure!(version.status.success(), "compiler version command failed");
    // Require an explicit/resolved filesystem path for reproducible binary identity.
    let binary =
        fs::read(runner.path()).context("provide --solc with an absolute compiler path")?;
    let mut bundle = Bundle {
        schema: "riffcat-source-bundle/1".into(),
        compiler: json!({"path": runner.path(), "version": String::from_utf8_lossy(&version.stdout), "sha256": hex::encode(Sha256::digest(&binary))}),
        artifacts: Vec::new(),
        captures: Vec::new(),
    };
    for path in paths {
        ensure!(
            fs::metadata(path)?.len() <= 1_048_576,
            "source exceeds 1 MiB"
        );
        let source = fs::read_to_string(path)?;
        let input = json!({"language":"Solidity", "sources":{"input.sol":{"content":source}},
            "settings":{"outputSelection":{"*":{"": ["ast"]}}}});
        let compiled = runner.compile(&input)?;
        compiled
            .check_errors()
            .with_context(|| format!("compiling {}", path.display()))?;
        bundle.artifacts.push(SourceArtifact {
            path: path.to_string_lossy().into_owned(),
            source,
            ast: compiled.source_ast("input.sol")?.clone(),
        });
        bundle
            .captures
            .push(json!({"input":input,"output":compiled.raw()}));
    }
    write_new(output, &bundle)
}

pub fn run_search(
    bundle: &Path,
    query_file: usize,
    start: usize,
    end: usize,
    view: &str,
    overlap: bool,
    output: Option<&Path>,
) -> Result<()> {
    ensure!(
        fs::metadata(bundle)?.len() <= 128 * 1024 * 1024,
        "bundle exceeds 128 MiB"
    );
    let bundle: Bundle = serde_json::from_reader(fs::File::open(bundle)?)?;
    ensure!(
        bundle.schema == "riffcat-source-bundle/1",
        "unsupported bundle schema"
    );
    let view = match view {
        "names" => View::Names,
        "bindings" => View::Bindings,
        "bindings-ignore-literals" => View::BindingsIgnoreLiterals,
        _ => anyhow::bail!("unknown source view"),
    };
    let result = if overlap {
        use riff_catalog_solidity::similarity::{IndexBudget, SyntaxIndex};
        let index = SyntaxIndex::build(&bundle.artifacts, view, IndexBudget::default())?;
        serde_json::to_value(index.overlap(query_file, start, end, 3, 200)?)?
    } else {
        serde_json::to_value(search(&bundle.artifacts, query_file, start, end, view)?)?
    };
    match output {
        Some(path) => write_new(path, &result),
        None => {
            serde_json::to_writer_pretty(std::io::stdout().lock(), &result)?;
            Ok(())
        }
    }
}
