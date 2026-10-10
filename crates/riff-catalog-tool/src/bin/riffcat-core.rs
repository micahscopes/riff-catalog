//! riffcat-core: the catalog with no recipes built in. It admits units that
//! any producer wrote as JSON lines, checks them against a declaration file,
//! and groups, compares and commits to them.

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use riff_catalog_tool::{CoreCommand, Corpus, commands};

#[derive(Parser)]
#[command(name = "riffcat-core", about = "content addressing by facet for graphs from any producer", version)]
struct Cli {
    /// Corpus directory (JSONL records).
    #[arg(long, global = true, default_value = "corpus")]
    corpus: PathBuf,
    /// Machine output (JSON rows).
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: CoreCommand,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut corpus = Corpus::open(&cli.corpus)?;
    commands::run(&mut corpus, cli.json, cli.command)
}
