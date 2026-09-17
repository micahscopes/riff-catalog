//! riffcat: a catalog of riffs. Ingest compiler artifacts into a facet
//! corpus, then ask which ones rhyme.

mod claims_cmd;
mod conformance;
mod corpus;
mod facet;
mod ingest;
mod queries;
mod region_cmd;
mod source_cmd;
mod ssa_trace_cmd;
mod stack_oracle_cmd;
mod stack_policy_cmd;
mod stack_trace_cmd;
mod table;
mod view_cmd;

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
    /// Capture source text and resolved Solidity ASTs in an offline bundle.
    SourceCapture {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        #[arg(long)]
        output: PathBuf,
    },
    /// Find selected syntax in a captured corpus, without a detector query.
    SourceSearch {
        bundle: PathBuf,
        #[arg(long, default_value_t = 0)]
        query_file: usize,
        #[arg(long)]
        start: usize,
        #[arg(long)]
        end: usize,
        #[arg(long, default_value = "bindings")]
        view: String,
        /// Rank shared subtrees and show nonoverlapping coverage.
        #[arg(long)]
        overlap: bool,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Compare explicit ordered pure regions (experimental JSONL protocol).
    Region {
        /// Read requests from stdin and write one versioned result per line.
        #[arg(long, required = true)]
        jsonl: bool,
    },
    /// Compile or parse artifacts (.sol, .yul, .sona, .rmir, sourcify refs) and write graphs +
    /// digests into the corpus.
    Ingest {
        /// Files to ingest.
        paths: Vec<PathBuf>,
        /// Verified contracts to fetch: chainId:address (repeatable).
        #[arg(long)]
        sourcify: Vec<String>,
        #[arg(long, default_value = "both")]
        optimize: String,
        /// Units to emit: fn,object,ssa,evm,rmir-function (sol-contract/sol-fn,
        /// sona-module, and package-level RMIR views are always on for their inputs).
        #[arg(long, default_value = "fn,object,ssa")]
        units: String,
        /// Error on unknown Solidity node types instead of tagging them.
        #[arg(long)]
        strict: bool,
        #[arg(long)]
        label: Option<String>,
    },
    /// Ingest versioned JSONL snapshots from solc's SSA observer.
    IngestSsaTrace {
        /// JSONL trace emitted by yulssatrace.
        path: PathBuf,
        /// Stable identity namespace for graph addresses.
        #[arg(long)]
        owner: Option<String>,
    },
    /// Ingest solc stack decisions or a full compiler event stream.
    #[command(visible_alias = "ingest-compiler-trace")]
    IngestStackTrace {
        /// JSONL emitted by yulssatrace's stack or compiler event output.
        path: PathBuf,
        /// Stable identity namespace for event addresses.
        #[arg(long)]
        owner: Option<String>,
    },
    /// Export content-addressed pure-SWAP cases for an exact oracle.
    StackOracleExport {
        /// JSONL emitted by yulssatrace's stack or compiler event output.
        path: PathBuf,
        /// Largest fully reachable stack admitted to the oracle domain.
        #[arg(long, default_value_t = 16)]
        max_size: usize,
        /// Stable identity namespace used while lowering the trace.
        #[arg(long)]
        owner: Option<String>,
    },
    /// Emit a solc replay policy from a content-addressed stack tradeoff class.
    StackPolicy {
        /// Shape-address prefix from the solc.stack-in-tradeoff/1 view.
        profile: String,
        /// Restrict matches by owner, unit name, or artifact id.
        #[arg(long)]
        selector: Option<String>,
    },
    /// List SSA stage metrics and snapshot addresses.
    Observations {
        /// Substring over owner, function, or snapshot id.
        selector: Option<String>,
        /// Restrict to an exact function name. Use <main> for main code.
        #[arg(long)]
        function: Option<String>,
        /// Restrict to transform or layout observations.
        #[arg(long)]
        stage_kind: Option<String>,
    },
    /// Materialize a declarative graph view and persist its facet addresses.
    View {
        /// Path to a riffcat-view/1 specification.
        spec: PathBuf,
        /// Substring over artifact id, owner, or graph name.
        selector: String,
        /// Restrict the input graph unit.
        #[arg(long)]
        unit: Option<String>,
        /// Restrict to an exact graph name.
        #[arg(long)]
        name: Option<String>,
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
    /// The corpus root: a canonical, order-independent merkle root over the
    /// corpus's distinct facet addresses, the commitment conditional claims
    /// reference.
    Root {
        /// Restrict to one unit (default: every unit).
        #[arg(long)]
        unit: Option<String>,
        /// Restrict to one mode (default: every mode).
        #[arg(long)]
        mode: Option<String>,
        /// Recompute and compare against a published root (hex); mismatch
        /// fails loudly with exit 1.
        #[arg(long)]
        check: Option<String>,
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
    /// Accept conditional claims whose assumptions root matches (hex,
    /// repeatable; implies --claims). Unlisted roots never merge.
    #[arg(long)]
    assume: Vec<String>,
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
        /// Merkle root (hex) of the assumption set this claim is conditional
        /// on (see `riffcat root`). Conditional claims merge only under
        /// `--assume <root>`.
        #[arg(long)]
        assumptions: Option<String>,
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
    match &cli.command {
        Command::SourceCapture { paths, output } => {
            return source_cmd::capture(paths, output, cli.solc.as_deref());
        }
        Command::SourceSearch {
            bundle,
            query_file,
            start,
            end,
            view,
            overlap,
            output,
        } => {
            return source_cmd::run_search(
                bundle,
                *query_file,
                *start,
                *end,
                view,
                *overlap,
                output.as_deref(),
            );
        }
        _ => {}
    }
    if matches!(&cli.command, Command::Region { .. }) {
        return region_cmd::run();
    }
    let corpus = Corpus::open(&cli.corpus)?;
    let cache_dir = PathBuf::from(&cli.cache);

    match cli.command {
        Command::Region { .. } | Command::SourceCapture { .. } | Command::SourceSearch { .. } => {
            unreachable!("handled before corpus access")
        }
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
        Command::IngestSsaTrace { path, owner } => ssa_trace_cmd::ingest(
            &corpus,
            &ssa_trace_cmd::TraceIngestArgs { path, owner },
            cli.json,
        ),
        Command::IngestStackTrace { path, owner } => stack_trace_cmd::ingest(
            &corpus,
            &stack_trace_cmd::StackTraceIngestArgs { path, owner },
            cli.json,
        ),
        Command::StackOracleExport {
            path,
            max_size,
            owner,
        } => stack_oracle_cmd::run(&stack_oracle_cmd::StackOracleExportArgs {
            path,
            max_size,
            owner,
        }),
        Command::StackPolicy { profile, selector } => stack_policy_cmd::run(
            &corpus,
            &stack_policy_cmd::StackPolicyArgs { profile, selector },
        ),
        Command::Observations {
            selector,
            function,
            stage_kind,
        } => ssa_trace_cmd::list(
            &corpus,
            selector.as_deref(),
            function.as_deref(),
            stage_kind.as_deref(),
            cli.json,
        ),
        Command::View {
            spec,
            selector,
            unit,
            name,
        } => view_cmd::run(
            &corpus,
            &view_cmd::ViewArgs {
                spec,
                selector,
                unit,
                name,
            },
            cli.json,
        ),
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
        Command::Root { unit, mode, check } => queries::root(
            &corpus,
            unit.as_deref(),
            mode.as_deref(),
            check.as_deref(),
            cli.json,
        ),
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
                assumptions,
                assert,
            } => {
                let assumptions = assumptions
                    .as_deref()
                    .map(riff_catalog_core::Digest::from_hex)
                    .transpose()?;
                claims_cmd::claim_add(&corpus, &assert.into_args()?, &left, &right, assumptions)
            }
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
            assume: self.assume,
            require: self.require,
        }
    }
}
