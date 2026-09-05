use std::{collections::BTreeMap, fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use riff_catalog_bloat::*;

#[derive(Parser)]
#[command(
    name = "riffcat-bloat",
    about = "Capture and replay explicit compiler code-growth evidence"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Seal a compiler-independent Capture JSON body into an immutable capture.
    Seal {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Import bounded compatibility evidence from existing Fe stderr.
    ImportFe {
        #[arg(long)]
        trace: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        wgsl: Option<PathBuf>,
        #[arg(long)]
        label: String,
        #[arg(long)]
        source_id: String,
        #[arg(long)]
        compiler_id: String,
        #[arg(long, default_value = "unknown")]
        producer_revision: String,
        #[arg(long, default_value = "unknown")]
        command: String,
        #[arg(long = "setting", value_parser = key_value)]
        settings: Vec<(String, String)>,
        #[arg(long = "env", value_parser = key_value)]
        environment: Vec<(String, String)>,
    },
    /// Import versioned machine-readable events emitted by Fe.
    ImportEvents {
        #[arg(long)]
        events: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        label: String,
        #[arg(long)]
        source_id: String,
        #[arg(long)]
        compiler_id: String,
        #[arg(long, default_value = "unknown")]
        producer_revision: String,
        #[arg(long, default_value = "unknown")]
        command: String,
        #[arg(long = "setting", value_parser = key_value)]
        settings: Vec<(String, String)>,
    },
    Report {
        capture: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        verify_artifacts: bool,
    },
    Replay {
        capture: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Compare {
        left: PathBuf,
        right: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        verify_artifacts: bool,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Seal { input, output } => {
            if fs::metadata(&input)?.len() > 256 * 1024 * 1024 {
                bail!("capture body exceeds 256 MiB limit");
            }
            let capture: Capture = serde_json::from_slice(&fs::read(&input)?)
                .with_context(|| format!("parse capture body {}", input.display()))?;
            println!("{}", save_capture(&output, capture)?.capture_id);
        }
        Command::ImportFe {
            trace,
            output,
            wgsl,
            label,
            source_id,
            compiler_id,
            producer_revision,
            command,
            settings,
            environment,
        } => {
            let capture = import_fe_trace(FeImport {
                trace,
                wgsl,
                label,
                source_id,
                compiler_id,
                producer_revision,
                command,
                settings: collect_unique(settings)?,
                environment: collect_unique(environment)?,
            })?;
            println!("{}", save_capture(&output, capture)?.capture_id);
        }
        Command::ImportEvents {
            events,
            output,
            label,
            source_id,
            compiler_id,
            producer_revision,
            command,
            settings,
        } => {
            let capture = import_fe_events(FeEventsImport {
                events,
                label,
                source_id,
                compiler_id,
                producer_revision,
                command,
                settings: collect_unique(settings)?,
            })?;
            println!("{}", save_capture(&output, capture)?.capture_id);
        }
        Command::Report {
            capture,
            json,
            verify_artifacts: verify,
        } => show_report(&capture, json, verify)?,
        Command::Replay { capture, json } => show_report(&capture, json, true)?,
        Command::Compare {
            left,
            right,
            json,
            verify_artifacts: verify,
        } => {
            let left_file = load_capture(&left)?;
            let right_file = load_capture(&right)?;
            if verify {
                verify_artifacts(&left, &left_file)?;
                verify_artifacts(&right, &right_file)?;
            }
            let value = compare(&left_file, &right_file);
            if json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                print!("{}", render_compare_table(&value));
            }
        }
    }
    Ok(())
}

fn show_report(path: &std::path::Path, json: bool, verify: bool) -> Result<()> {
    let file = load_capture(path)?;
    if verify {
        verify_artifacts(path, &file)?;
    }
    let value = report(&file);
    if json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        print!("{}", render_table(&value));
    }
    Ok(())
}

fn key_value(value: &str) -> Result<(String, String), String> {
    let (key, value) = value
        .split_once('=')
        .ok_or_else(|| "expected KEY=VALUE".to_owned())?;
    if key.is_empty() {
        return Err("key must not be empty".into());
    }
    Ok((key.into(), value.into()))
}

fn collect_unique(values: Vec<(String, String)>) -> Result<BTreeMap<String, String>> {
    let mut map = BTreeMap::new();
    for (key, value) in values {
        if map.insert(key.clone(), value).is_some() {
            bail!("duplicate key `{key}`");
        }
    }
    Ok(map)
}
