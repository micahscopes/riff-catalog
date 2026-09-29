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
    /// Verify a saved census by recomputing it from its source.
    CensusReplay {
        census: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 10)]
        top: usize,
    },
    /// Replay two censuses and compare bytes using explicit alignment hints.
    CensusCompare {
        left: PathBuf,
        right: PathBuf,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 10)]
        top: usize,
    },
    /// Census an artifact from a verified capture, preserving completion/provenance.
    CensusCapture {
        capture: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long, default_value = "shader.wgsl")]
        artifact: String,
        #[arg(long)]
        regions: Option<PathBuf>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 10)]
        top: usize,
    },
    /// Measure exact artifact regions and scoped textual repetition, not savings.
    Census {
        artifact: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
        /// Digest-bound riffcat-regions/1 manifest for arbitrary artifact formats.
        #[arg(long)]
        regions: Option<PathBuf>,
        #[arg(long)]
        json: bool,
        #[arg(long, default_value_t = 10)]
        top: usize,
    },
    /// Byte census of one Fe EVM contract from `fe dev trace emit` plus
    /// `fe dev debug emit --attribution-details`, checked against the artifact.
    FeTraceBytes {
        /// Trace bundle JSONL from `fe dev trace emit`.
        #[arg(long)]
        trace: PathBuf,
        /// Attribution details JSON from `fe dev debug emit --attribution-details`.
        #[arg(long)]
        attribution: PathBuf,
        /// Contract whose runtime code object to read.
        #[arg(long)]
        contract: String,
        /// Runtime artifact (raw bytes or Fe's hex `.bin`).
        #[arg(long)]
        artifact: PathBuf,
        /// Report repeated EVM runs of at least this many bytes per copy.
        #[arg(long, default_value_t = 32)]
        min_run_bytes: u32,
        /// Compare PUSH constants as ports too (copies that differ only in
        /// constants group together).
        #[arg(long)]
        constants_as_ports: bool,
        /// Write the full report as JSON.
        #[arg(long)]
        json_out: Option<PathBuf>,
        /// Write the raw runtime bytes and a digest-bound riffcat-regions/1
        /// manifest (emitted functions, EVM runs enabled) so `census` can
        /// replay the run census: `<dir>/runtime.bin`, `<dir>/regions.json`.
        #[arg(long)]
        census_dir: Option<PathBuf>,
        #[arg(long, default_value_t = 25)]
        top: usize,
    },
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
        Command::CensusReplay { census, json, top } => {
            let file = replay_census(&census)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&file.census)?);
            } else {
                print!("{}", render_census(&file.census, top));
            }
        }
        Command::CensusCompare {
            left,
            right,
            json,
            top,
        } => {
            let left = replay_census(&left)?;
            let right = replay_census(&right)?;
            let value = compare_censuses(&left, &right);
            if json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                print!("{}", render_census_comparison(&value, top));
            }
        }
        Command::CensusCapture {
            capture,
            output,
            artifact,
            regions,
            json,
            top,
        } => {
            let source = CensusSource::Capture {
                path: capture,
                artifact_id: artifact,
                regions,
            };
            let value = if let Some(path) = output {
                save_census(&path, source)?.census
            } else {
                source.run()?
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                print!("{}", render_census(&value, top));
            }
        }
        Command::Census {
            artifact,
            output,
            regions,
            json,
            top,
        } => {
            let source = CensusSource::Artifact {
                path: artifact,
                regions,
            };
            let value = if let Some(path) = output {
                save_census(&path, source)?.census
            } else {
                source.run()?
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                print!("{}", render_census(&value, top));
            }
        }
        Command::FeTraceBytes {
            trace,
            attribution,
            contract,
            artifact,
            min_run_bytes,
            constants_as_ports,
            json_out,
            census_dir,
            top,
        } => {
            let bytes = decode_artifact(&fs::read(&artifact)?)?;
            let details = fs::read_to_string(&attribution)
                .with_context(|| format!("read {}", attribution.display()))?;
            let reader = std::io::BufReader::new(
                fs::File::open(&trace).with_context(|| format!("open {}", trace.display()))?,
            );
            let (_, manifest, _, report) = fe_trace_bytes(
                reader,
                &details,
                &contract,
                &bytes,
                &EvmRunOptions {
                    min_run_bytes,
                    constants_as_ports,
                },
            )?;
            if let Some(dir) = census_dir {
                fs::create_dir_all(&dir)?;
                fs::write(dir.join("runtime.bin"), &bytes)?;
                fs::write(
                    dir.join("regions.json"),
                    serde_json::to_vec_pretty(&manifest)?,
                )?;
            }
            if let Some(path) = json_out {
                fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
            }
            print!("{}", render_fe_trace_bytes(&report, top));
        }
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
