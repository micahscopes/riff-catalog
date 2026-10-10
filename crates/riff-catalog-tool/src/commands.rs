//! The generic commands: everything that works on units and a corpus without
//! knowing where the units came from. An application adds them to its own
//! commands with `#[command(flatten)]`; `riffcat-core` has only these.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand};
use riff_catalog_core::{RecipeDecl, check_recipe, read_units};

use crate::corpus::Corpus;
use crate::queries::QueryArgs;
use crate::{admission, claims_cmd, queries, view_cmd};

#[derive(Subcommand)]
pub enum CoreCommand {
    /// Admit units written as JSON lines (one unit per line) into the corpus.
    Add {
        /// Unit files. Each file is one replacement scope: adding it again
        /// replaces what it added before.
        units: Vec<PathBuf>,
        /// Recipe declaration files (JSON) for the units' recipes.
        #[arg(long = "declaration")]
        declarations: Vec<PathBuf>,
        /// The world the units come from, e.g. a compiler and its commit.
        #[arg(long)]
        world: String,
    },
    /// Check unit files against a recipe declaration without admitting them.
    Check {
        units: Vec<PathBuf>,
        #[arg(long = "declaration")]
        declarations: Vec<PathBuf>,
    },
    /// List the recipes this tool has declarations for.
    Recipes,
    /// Print a recipe's declaration, or the narrowest one some units satisfy.
    Declaration {
        #[command(subcommand)]
        action: DeclarationAction,
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
        #[arg(long)]
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

#[derive(Subcommand)]
pub enum DeclarationAction {
    /// Print the declaration of a registered recipe as JSON.
    Print { recipe: String },
    /// Print the narrowest declaration the given units satisfy: a draft to
    /// check by hand, since it cannot know what the producer may also emit.
    Infer {
        recipe: String,
        units: Vec<PathBuf>,
    },
}

#[derive(Args)]
pub struct QueryFlags {
    /// The population to query, e.g. `yul-fn`.
    #[arg(long)]
    pub unit: String,
    /// identity | shape
    #[arg(long, default_value = "shape")]
    pub mode: String,
    /// all | names-blind | structure | structure+constants | ...
    #[arg(long, default_value = "all")]
    pub facet: String,
    /// Apply recorded claims (union-find merge of classes).
    #[arg(long)]
    pub claims: bool,
    /// Accept conditional claims whose assumptions root matches (hex,
    /// repeatable; implies --claims). Unlisted roots never merge.
    #[arg(long)]
    pub assume: Vec<String>,
    /// Only include rows attested with this property (gating).
    #[arg(long)]
    pub require: Option<String>,
}

impl QueryFlags {
    pub fn into_query_args(self) -> QueryArgs {
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

#[derive(Subcommand)]
pub enum ClaimAction {
    Add {
        #[arg(long)]
        left: String,
        #[arg(long)]
        right: String,
        /// Merkle root (hex) of the assumption set this claim is conditional
        /// on (see `root`). Conditional claims merge only under
        /// `--assume <root>`.
        #[arg(long)]
        assumptions: Option<String>,
        #[command(flatten)]
        assert: AssertFlags,
    },
    List,
}

#[derive(Subcommand)]
pub enum AttestAction {
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
pub struct AssertFlags {
    #[arg(long)]
    pub unit: String,
    #[arg(long, default_value = "shape")]
    pub mode: String,
    #[arg(long, default_value = "structure")]
    pub facet: String,
    #[arg(long, default_value = "note")]
    pub witness_kind: String,
    /// Witness payload entries, ordered: key=value (repeatable).
    #[arg(long = "witness")]
    pub witness: Vec<String>,
    #[arg(long)]
    pub note: Option<String>,
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

fn units_of(files: &[PathBuf]) -> Result<Vec<riff_catalog_core::Unit>> {
    let mut units = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(file)?;
        units.extend(read_units(&text).map_err(|error| anyhow::anyhow!("{}: {error}", file.display()))?);
    }
    Ok(units)
}

/// Run one generic command against a corpus.
pub fn run(corpus: &mut Corpus, json: bool, command: CoreCommand) -> Result<()> {
    match command {
        CoreCommand::Add { units, declarations: files, world } => {
            for file in &files {
                corpus.declare(admission::read_declaration(file)?)?;
            }
            let admitted = admission::admit_files(corpus, &world, &units)?;
            println!("admitted {admitted} units into world {world}");
            Ok(())
        }
        CoreCommand::Check { units, declarations: files } => {
            let mut declarations = corpus.declarations().clone();
            for file in &files {
                declarations.declare(admission::read_declaration(file)?)?;
            }
            let units = units_of(&units)?;
            let mut broken = 0;
            for unit in &units {
                let Some(decl) = declarations.get(unit.recipe.as_str()) else {
                    anyhow::bail!("no declaration for recipe {}", unit.recipe.as_str());
                };
                if let Err(violations) = check_recipe(decl, &unit.graph) {
                    broken += 1;
                    for violation in violations.iter().take(5) {
                        println!(
                            "{}: {}{}",
                            unit.name,
                            violation.at.as_deref().map(|at| format!("{at}: ")).unwrap_or_default(),
                            violation.problem
                        );
                    }
                }
            }
            println!("{} units checked, {broken} break their declaration", units.len());
            if broken > 0 {
                std::process::exit(1);
            }
            Ok(())
        }
        CoreCommand::Recipes => {
            for recipe in corpus.declarations().recipes() {
                println!("{recipe}");
            }
            Ok(())
        }
        CoreCommand::Declaration { action } => match action {
            DeclarationAction::Print { recipe } => {
                let Some(decl) = corpus.declarations().get(&recipe) else {
                    anyhow::bail!("no declaration for recipe {recipe}");
                };
                println!("{}", serde_json::to_string_pretty(decl)?);
                Ok(())
            }
            DeclarationAction::Infer { recipe, units } => {
                let units = units_of(&units)?;
                let graphs = units.iter().filter(|unit| unit.recipe.as_str() == recipe).map(|unit| &unit.graph);
                match RecipeDecl::infer(&recipe, graphs) {
                    Ok(decl) => {
                        println!("{}", serde_json::to_string_pretty(&decl)?);
                        Ok(())
                    }
                    Err(problems) => anyhow::bail!("no single declaration fits:\n{}", problems.join("\n")),
                }
            }
        },
        CoreCommand::View { spec, selector, unit, name } => {
            view_cmd::run(corpus, &view_cmd::ViewArgs { spec, selector, unit, name }, json)
        }
        CoreCommand::Bucket { query, min_size, top } => {
            queries::bucket(corpus, &query.into_query_args(), min_size, top, json)
        }
        CoreCommand::Overlap { left, right, query } => {
            queries::overlap(corpus, &query.into_query_args(), &left, &right, json)
        }
        CoreCommand::Diff { left, right, unit, name } => {
            queries::diff(corpus, &unit, &left, &right, name.as_deref(), json)
        }
        CoreCommand::Root { unit, mode, check } => {
            queries::root(corpus, unit.as_deref(), mode.as_deref(), check.as_deref(), json)
        }
        CoreCommand::Claim { action } => match action {
            ClaimAction::Add { left, right, assumptions, assert } => {
                let assumptions = assumptions.as_deref().map(riff_catalog_core::Digest::from_hex).transpose()?;
                claims_cmd::claim_add(corpus, &assert.into_args()?, &left, &right, assumptions)
            }
            ClaimAction::List => claims_cmd::list(corpus, json),
        },
        CoreCommand::Attest { action } => match action {
            AttestAction::Add { subject, property, assert } => {
                claims_cmd::attest_add(corpus, &assert.into_args()?, &subject, &property)
            }
            AttestAction::List => claims_cmd::list(corpus, json),
        },
    }
}
