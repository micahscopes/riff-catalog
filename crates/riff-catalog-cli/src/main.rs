//! riffcat: a catalog of riffs. Ingest compiler artifacts into a facet
//! corpus, then ask which ones rhyme.

mod claims_cmd;
mod conformance;
mod corpus;
mod facet;
mod ingest;
mod queries;
mod table;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

use corpus::Corpus;
use ingest::{IngestArgs, OptimizeChoice};
use queries::QueryArgs;

#[derive(Parser)]
#[command(
    name = "riffcat",
    about = "facet-relative content addressing for compiler artifacts",
    version
)]
struct Cli {
    /// Corpus directory (JSONL records).
    #[arg(long, global = true, default_value = "corpus")]
    corpus: PathBuf,
    /// Explicit solc binary (else $RIFFCAT_SOLC, $FE_SOLC_PATH, PATH).
    #[arg(long, global = true)]
    solc: Option<PathBuf>,
    /// Cache directory for solc outputs and sourcify fetches.
    #[arg(long, global = true, default_value_t = default_cache_dir())]
    cache: String,
    /// Machine output (JSON rows).
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

fn default_cache_dir() -> String {
    std::env::var("XDG_CACHE_HOME")
        .map(|cache| format!("{cache}/riff-catalog"))
        .unwrap_or_else(|_| {
            format!(
                "{}/.cache/riff-catalog",
                std::env::var("HOME").unwrap_or_else(|_| ".".into())
            )
        })
}

#[derive(Subcommand)]
enum Command {
    /// Compile artifacts (.sol, .yul, sourcify refs) and write graphs +
    /// digests into the corpus.
    Ingest {
        /// Files to ingest.
        paths: Vec<PathBuf>,
        /// Verified contracts to fetch: chainId:address (repeatable).
        #[arg(long)]
        sourcify: Vec<String>,
        #[arg(long, default_value = "both")]
        optimize: String,
        /// Units to emit: fn,object,ssa,evm (sol-contract/sol-fn always on
        /// for .sol inputs).
        #[arg(long, default_value = "fn,object,ssa")]
        units: String,
        /// Error on unknown Solidity node types instead of tagging them.
        #[arg(long)]
        strict: bool,
        #[arg(long)]
        label: Option<String>,
    },
    /// Group corpus units into equivalence classes at a facet.
    Bucket {
        #[command(flatten)]
        query: QueryFlags,
        #[arg(long, default_value_t = 2)]
        min_size: usize,
        #[arg(long, default_value_t = 25)]
        top: usize,
    },
    /// Structural-twin report between two selections.
    Overlap {
        left: String,
        right: String,
        #[command(flatten)]
        query: QueryFlags,
    },
    /// Per-dimension comparison between two selections.
    Diff {
        left: String,
        right: String,
        #[arg(long, default_value = "yul-fn")]
        unit: String,
        /// Restrict to one unit name (e.g. a function).
        #[arg(long)]
        name: Option<String>,
    },
    /// Run the dual-path + SSA-round-trip drift detectors over .sol files.
    Conformance {
        paths: Vec<PathBuf>,
        #[arg(long, default_value = "both")]
        optimize: String,
        #[arg(long)]
        keep_going: bool,
    },
    /// Witnessed equivalence claims.
    Claim {
        #[command(subcommand)]
        action: ClaimAction,
    },
    /// Witnessed property attestations.
    Attest {
        #[command(subcommand)]
        action: AttestAction,
    },
}

#[derive(Args)]
struct QueryFlags {
    #[arg(long, default_value = "yul-fn")]
    unit: String,
    /// identity | shape
    #[arg(long, default_value = "shape")]
    mode: String,
    /// all | names-blind | structure | structure+constants | ...
    #[arg(long, default_value = "all")]
    facet: String,
    /// Apply recorded claims (union-find merge of classes).
    #[arg(long)]
    claims: bool,
    /// Only include rows attested with this property (gating).
    #[arg(long)]
    require: Option<String>,
}

#[derive(Subcommand)]
enum ClaimAction {
    Add {
        #[arg(long)]
        left: String,
        #[arg(long)]
        right: String,
        #[command(flatten)]
        assert: AssertFlags,
    },
    List,
}

#[derive(Subcommand)]
enum AttestAction {
    Add {
        #[arg(long)]
        subject: String,
        #[arg(long)]
        property: String,
        #[command(flatten)]
        assert: AssertFlags,
    },
    List,
}

#[derive(Args)]
struct AssertFlags {
    #[arg(long, default_value = "yul-fn")]
    unit: String,
    #[arg(long, default_value = "shape")]
    mode: String,
    #[arg(long, default_value = "structure")]
    facet: String,
    #[arg(long, default_value = "note")]
    witness_kind: String,
    /// Witness payload entries, ordered: key=value (repeatable).
    #[arg(long = "witness")]
    witness: Vec<String>,
    #[arg(long)]
    note: Option<String>,
}

impl AssertFlags {
    fn into_args(self) -> Result<claims_cmd::AssertArgs> {
        let witness = self
            .witness
            .iter()
            .map(|entry| match entry.split_once('=') {
                Some((key, value)) => Ok((key.to_string(), value.to_string())),
                None => anyhow::bail!("--witness expects key=value, got `{entry}`"),
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(claims_cmd::AssertArgs {
            unit: self.unit,
            mode: self.mode,
            facet: self.facet,
            witness_kind: self.witness_kind,
            witness,
            note: self.note,
        })
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let corpus = Corpus::open(&cli.corpus)?;
    let cache_dir = PathBuf::from(&cli.cache);

    match cli.command {
        Command::Ingest {
            paths,
            sourcify,
            optimize,
            units,
            strict,
            label,
        } => {
            let args = IngestArgs {
                paths,
                sourcify,
                optimize: OptimizeChoice::parse(&optimize)?,
                units: units
                    .split(',')
                    .map(|unit| unit.trim().to_string())
                    .collect(),
                strict,
                label,
                solc_path: cli.solc.clone(),
                cache_dir,
            };
            ingest::run(&corpus, &args)
        }
        Command::Bucket {
            query,
            min_size,
            top,
        } => queries::bucket(&corpus, &query.into_query_args(), min_size, top, cli.json),
        Command::Overlap { left, right, query } => {
            queries::overlap(&corpus, &query.into_query_args(), &left, &right, cli.json)
        }
        Command::Diff {
            left,
            right,
            unit,
            name,
        } => queries::diff(&corpus, &unit, &left, &right, name.as_deref(), cli.json),
        Command::Conformance {
            paths,
            optimize,
            keep_going,
        } => {
            let green = conformance::run(
                &paths,
                OptimizeChoice::parse(&optimize)?,
                cli.solc.as_deref(),
                &cache_dir,
                keep_going,
                cli.json,
            )?;
            if !green {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::Claim { action } => match action {
            ClaimAction::Add {
                left,
                right,
                assert,
            } => claims_cmd::claim_add(&corpus, &assert.into_args()?, &left, &right),
            ClaimAction::List => claims_cmd::list(&corpus, cli.json),
        },
        Command::Attest { action } => match action {
            AttestAction::Add {
                subject,
                property,
                assert,
            } => claims_cmd::attest_add(&corpus, &assert.into_args()?, &subject, &property),
            AttestAction::List => claims_cmd::list(&corpus, cli.json),
        },
    }
}

impl QueryFlags {
    fn into_query_args(self) -> QueryArgs {
        QueryArgs {
            unit: self.unit,
            mode: self.mode,
            facet: self.facet,
            use_claims: self.claims,
            require: self.require,
        }
    }
}
