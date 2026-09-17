use anyhow::{Context, Result, ensure};
use riff_catalog_region::{
    exact::Budget,
    yul_cfg::{compare, materialize},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

pub fn capture(path: &Path, object: &str, output: &Path, solc: Option<&Path>) -> Result<()> {
    ensure!(!output.exists(), "output already exists");
    ensure!(
        fs::metadata(path)?.len() <= 1_048_576,
        "source exceeds 1 MiB"
    );
    let source = fs::read_to_string(path)?;
    let runner = riff_catalog_solc::SolcRunner::locate(solc);
    let binary =
        fs::read(runner.path()).context("provide --solc with a filesystem compiler path")?;
    let version = std::process::Command::new(runner.path())
        .arg("--version")
        .output()?;
    ensure!(version.status.success(), "compiler version failed");
    let mut input = riff_catalog_solc::yul_input("input.yul", &source, false);
    input["settings"]["experimental"] = json!(true);
    let compiled = runner.compile(&input)?;
    compiled.check_errors()?;
    let cfg = compiled.yul_cfg_json("input.yul", object)?;
    crate::source_cmd::write_new(
        output,
        &json!({"schema":"riffcat-yul-bundle/1","source":source,"path":path,
        "compiler":{"path":runner.path(),"version":String::from_utf8_lossy(&version.stdout),"sha256":hex::encode(Sha256::digest(&binary))},
        "input":input,"output":compiled.raw(),"cfg":cfg}),
    )
}

pub fn run(
    bundle: &Path,
    left: &str,
    right: &str,
    left_blocks: &[String],
    right_blocks: &[String],
    states: usize,
    output: Option<&Path>,
) -> Result<()> {
    ensure!(
        fs::metadata(bundle)?.len() <= 128 * 1024 * 1024,
        "bundle exceeds 128 MiB"
    );
    ensure!(states <= 100_000, "state budget cannot exceed 100000");
    let bundle: Value = serde_json::from_reader(fs::File::open(bundle)?)?;
    ensure!(
        bundle["schema"] == "riffcat-yul-bundle/1",
        "unsupported bundle schema"
    );
    let a = bundle["cfg"]
        .pointer(left)
        .context("left function JSON pointer not found")?;
    let b = bundle["cfg"]
        .pointer(right)
        .context("right function JSON pointer not found")?;
    let result = compare(
        materialize(a, left_blocks)?,
        materialize(b, right_blocks)?,
        Budget {
            states,
            ..Default::default()
        },
    )?;
    if let Some(path) = output {
        crate::source_cmd::write_new(path, &result)?;
    } else {
        serde_json::to_writer_pretty(std::io::stdout().lock(), &result)?;
    }
    Ok(())
}
