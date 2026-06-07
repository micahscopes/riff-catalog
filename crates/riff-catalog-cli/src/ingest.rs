//! `riffcat ingest`: compile artifacts and write graphs + digests into the
//! corpus. Every unit is hashed under exactly two policies per level —
//! identity-bound and anonymous-shape, all dimensions, CondenseScc — and
//! query-time facets are dimension subsets (invariant I9).

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use riff_catalog_core::{
    CyclePolicy, DigestRequest, Graph, GraphKey, HashPolicy, ViewMode, digest_graph,
};
use riff_catalog_evm::{BytecodeKind, EVM_LEVEL, lower_bytecode};
use riff_catalog_solc::{CachedSolc, CompileOptions, Pipeline, SolcOutput, SolcRunner};
use riff_catalog_solidity::{SOL_AST_LEVEL, WalkOptions, lower_source_unit};
use riff_catalog_sourcify::{ContractId, SourcifyClient};
use riff_catalog_yul::lower::{LowerOptions, YUL_AST_LEVEL, lower_object};
use riff_catalog_yul::ssa::{YUL_SSA_LEVEL, lower_yul_cfg};
use riff_catalog_yul::{from_solc_value, parse_object};
use sha2::{Digest as _, Sha256};

use crate::corpus::{Corpus, Record};

pub struct IngestArgs {
    pub paths: Vec<PathBuf>,
    pub sourcify: Vec<String>,
    pub optimize: OptimizeChoice,
    pub units: Vec<String>,
    pub strict: bool,
    pub label: Option<String>,
    pub solc_path: Option<PathBuf>,
    pub cache_dir: PathBuf,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OptimizeChoice {
    On,
    Off,
    Both,
}

impl OptimizeChoice {
    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "on" => Ok(Self::On),
            "off" => Ok(Self::Off),
            "both" => Ok(Self::Both),
            other => bail!("--optimize must be on|off|both, got {other}"),
        }
    }

    fn variants(self) -> Vec<bool> {
        match self {
            Self::On => vec![true],
            Self::Off => vec![false],
            Self::Both => vec![false, true],
        }
    }
}

pub fn run(corpus: &Corpus, args: &IngestArgs) -> Result<()> {
    let solc = CachedSolc::new(
        SolcRunner::locate(args.solc_path.as_deref()),
        args.cache_dir.join("solc"),
    );

    let mut sources: Vec<(String, String, String)> = Vec::new(); // (origin, name, content)
    for path in &args.paths {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "input".to_string());
        sources.push((path.display().to_string(), name, content));
    }

    for (origin, name, content) in &sources {
        if name.ends_with(".yul") {
            ingest_yul(corpus, &solc, args, origin, name, content)?;
        } else {
            ingest_solidity(corpus, &solc, args, origin, name, content)?;
        }
    }

    for spec in &args.sourcify {
        ingest_sourcify(corpus, args, spec)?;
    }

    Ok(())
}

fn want_unit(args: &IngestArgs, unit: &str) -> bool {
    args.units.iter().any(|wanted| wanted == unit)
}

fn artifact_id(owner: &str, optimize: bool) -> String {
    let mut hasher = Sha256::new();
    hasher.update(owner.as_bytes());
    hasher.update([u8::from(optimize)]);
    hex::encode(hasher.finalize())[..16].to_string()
}

/// Hash a unit graph under both view modes and emit graph + digest records.
fn emit_unit(
    records: &mut Vec<Record>,
    artifact: &str,
    owner: &str,
    level: &str,
    unit: &str,
    name: &str,
    graph_key: &GraphKey,
    graph: &Graph,
) -> Result<()> {
    records.push(Record::Graph {
        artifact_id: artifact.to_string(),
        unit: unit.to_string(),
        level: level.to_string(),
        name: name.to_string(),
        graph_key: graph_key.clone(),
        graph: graph.clone(),
    });
    for (mode, view_mode) in [
        ("identity", ViewMode::IdentityBound),
        ("shape", ViewMode::AnonymousShape),
    ] {
        let policy = HashPolicy::new(level, view_mode, CyclePolicy::CondenseScc)?;
        let result = digest_graph(
            &DigestRequest::all_dimensions(graph_key.clone(), policy.clone()),
            graph,
        )?;
        records.push(Record::Digest {
            artifact_id: artifact.to_string(),
            owner: owner.to_string(),
            unit: unit.to_string(),
            level: level.to_string(),
            name: name.to_string(),
            mode: mode.to_string(),
            policy_id: policy.policy_id(),
            digests: result.hashes.graph.values.clone(),
            node_count: graph.nodes.len(),
        });
    }
    Ok(())
}

fn ingest_solidity(
    corpus: &Corpus,
    solc: &CachedSolc,
    args: &IngestArgs,
    origin: &str,
    name: &str,
    content: &str,
) -> Result<()> {
    let inputs: BTreeMap<String, String> = [(name.to_string(), content.to_string())].into();

    for optimize in args.optimize.variants() {
        let opt_tag = if optimize { "opt" } else { "noopt" };
        let options = CompileOptions {
            pipeline: Pipeline::ViaIr,
            optimize,
            ..Default::default()
        };
        let output = solc.compile(&riff_catalog_solc::solidity_input(&inputs, &options))?;
        output.check_errors()?;

        let mut records = Vec::new();

        // Source-level AST (identical across optimizer settings; ingest once)
        if !optimize || args.optimize == OptimizeChoice::On {
            let owner = format!("sol:{name}");
            let artifact = artifact_id(&owner, false);
            records.push(artifact_record(
                &artifact, &owner, origin, "via-ir", false, args,
            ));
            let ast = output.source_ast(name)?;
            let lowered = lower_source_unit(
                &ast.clone(),
                &owner,
                &WalkOptions {
                    strict: args.strict,
                },
            )?;
            for warning in &lowered.warnings {
                eprintln!("warning: {warning}");
            }
            for unit in std::iter::once(&lowered.source_unit)
                .chain(&lowered.contracts)
                .chain(&lowered.functions)
            {
                if want_unit(args, unit.unit)
                    || unit.unit == "sol-contract"
                    || unit.unit == "sol-fn"
                {
                    emit_unit(
                        &mut records,
                        &artifact,
                        &owner,
                        SOL_AST_LEVEL,
                        unit.unit,
                        &unit.name,
                        &unit.graph_key,
                        &unit.graph,
                    )?;
                }
            }
        }

        for (source, contract) in output.contract_names() {
            ingest_contract_ir(
                &mut records,
                args,
                &output,
                origin,
                &source,
                &contract,
                opt_tag,
                optimize,
            )?;
        }

        let stem = format!(
            "{}-{opt_tag}",
            name.trim_end_matches(".sol").replace(['/', '\\'], "_")
        );
        corpus.append(&stem, &records)?;
        println!("ingested {name} ({opt_tag}): {} records", records.len());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn ingest_contract_ir(
    records: &mut Vec<Record>,
    args: &IngestArgs,
    output: &SolcOutput,
    origin: &str,
    source: &str,
    contract: &str,
    opt_tag: &str,
    optimize: bool,
) -> Result<()> {
    // yul-ast level: unoptimized + optimized IR ASTs
    for (variant, ast_value) in [
        ("ir", output.ir_ast(source, contract).ok()),
        ("iropt", output.ir_optimized_ast(source, contract).ok()),
    ] {
        let Some(ast_value) = ast_value else { continue };
        let owner = format!("yulir:{source}:{contract}:{variant}:{opt_tag}");
        let artifact = artifact_id(&owner, optimize);
        records.push(artifact_record(
            &artifact, &owner, origin, "via-ir", optimize, args,
        ));
        let object = from_solc_value(ast_value)?;
        let lowered = lower_object(&object, &owner, &LowerOptions::default())?;
        if want_unit(args, "object") {
            emit_unit(
                records,
                &artifact,
                &owner,
                YUL_AST_LEVEL,
                lowered.object.unit,
                &lowered.object.name,
                &lowered.object.graph_key,
                &lowered.object.graph,
            )?;
        }
        if want_unit(args, "fn") {
            for unit in &lowered.functions {
                emit_unit(
                    records,
                    &artifact,
                    &owner,
                    YUL_AST_LEVEL,
                    unit.unit,
                    &unit.name,
                    &unit.graph_key,
                    &unit.graph,
                )?;
            }
        }
    }

    // SSA level
    if want_unit(args, "ssa") {
        if let Ok(cfg) = output.yul_cfg_json(source, contract) {
            let owner = format!("yulssa:{source}:{contract}:{opt_tag}");
            let artifact = artifact_id(&owner, optimize);
            records.push(artifact_record(
                &artifact, &owner, origin, "via-ir", optimize, args,
            ));
            let lowered = lower_yul_cfg(cfg, &owner)?;
            for unit in lowered.objects.iter().chain(&lowered.functions) {
                emit_unit(
                    records,
                    &artifact,
                    &owner,
                    YUL_SSA_LEVEL,
                    unit.unit,
                    &unit.name,
                    &unit.graph_key,
                    &unit.graph,
                )?;
            }
        }
    }

    // EVM level
    if want_unit(args, "evm") {
        let owner = format!("evm:{source}:{contract}:via-ir:{opt_tag}");
        let artifact = artifact_id(&owner, optimize);
        records.push(artifact_record(
            &artifact, &owner, origin, "via-ir", optimize, args,
        ));
        for (kind, bytes) in [
            (BytecodeKind::Creation, output.bytecode(source, contract)),
            (
                BytecodeKind::Runtime,
                output.deployed_bytecode(source, contract),
            ),
        ] {
            let Ok(bytes) = bytes else { continue };
            if bytes.is_empty() {
                continue;
            }
            let lowered = lower_bytecode(&owner, kind, &bytes)?;
            emit_unit(
                records,
                &artifact,
                &owner,
                EVM_LEVEL,
                if kind == BytecodeKind::Creation {
                    "evm-creation"
                } else {
                    "evm-runtime"
                },
                &format!("{contract}/{}", kind.as_str()),
                &lowered.graph_key,
                &lowered.graph,
            )?;
        }
    }

    Ok(())
}

fn ingest_yul(
    corpus: &Corpus,
    solc: &CachedSolc,
    args: &IngestArgs,
    origin: &str,
    name: &str,
    content: &str,
) -> Result<()> {
    let mut records = Vec::new();

    // yul-ast level via our parser
    let owner = format!("yul:{name}");
    let artifact = artifact_id(&owner, false);
    records.push(artifact_record(
        &artifact, &owner, origin, "yul", false, args,
    ));
    let object =
        parse_object(content).map_err(|error| anyhow::anyhow!("parsing {name}: {error}"))?;
    let lowered = lower_object(&object, &owner, &LowerOptions::default())?;
    if want_unit(args, "object") {
        emit_unit(
            &mut records,
            &artifact,
            &owner,
            YUL_AST_LEVEL,
            lowered.object.unit,
            &lowered.object.name,
            &lowered.object.graph_key,
            &lowered.object.graph,
        )?;
    }
    if want_unit(args, "fn") {
        for unit in &lowered.functions {
            emit_unit(
                &mut records,
                &artifact,
                &owner,
                YUL_AST_LEVEL,
                unit.unit,
                &unit.name,
                &unit.graph_key,
                &unit.graph,
            )?;
        }
    }

    // SSA level via solc (works for direct Yul input)
    if want_unit(args, "ssa") {
        let output = solc.compile(&riff_catalog_solc::yul_input(name, content, false))?;
        if output.check_errors().is_ok() {
            for (source, contract) in output.contract_names() {
                if let Ok(cfg) = output.yul_cfg_json(&source, &contract) {
                    let ssa_owner = format!("yulssa:{name}:{contract}");
                    let ssa_artifact = artifact_id(&ssa_owner, false);
                    records.push(artifact_record(
                        &ssa_artifact,
                        &ssa_owner,
                        origin,
                        "yul",
                        false,
                        args,
                    ));
                    let lowered = lower_yul_cfg(cfg, &ssa_owner)?;
                    for unit in lowered.objects.iter().chain(&lowered.functions) {
                        emit_unit(
                            &mut records,
                            &ssa_artifact,
                            &ssa_owner,
                            YUL_SSA_LEVEL,
                            unit.unit,
                            &unit.name,
                            &unit.graph_key,
                            &unit.graph,
                        )?;
                    }
                }
            }
        }
    }

    let stem = name.trim_end_matches(".yul").replace(['/', '\\'], "_");
    corpus.append(&stem, &records)?;
    println!("ingested {name}: {} records", records.len());
    Ok(())
}

fn ingest_sourcify(corpus: &Corpus, args: &IngestArgs, spec: &str) -> Result<()> {
    let Some(id) = ContractId::parse(spec) else {
        bail!("--sourcify expects chainId:address, got {spec}");
    };
    let client = SourcifyClient::new(&args.cache_dir);
    let contract = client
        .fetch(&id)
        .with_context(|| format!("fetching {spec} from sourcify"))?;
    println!(
        "fetched {} ({}, compiler {})",
        contract.contract_name, contract.match_kind, contract.compiler_version
    );

    let resolver = riff_catalog_solc::InstalledSolc::new(
        semver::VersionReq::parse("^0.8").expect("valid range"),
        args.solc_path.as_deref(),
    );
    let output = match riff_catalog_sourcify::compile(&contract, &resolver, Pipeline::ViaIr) {
        Ok(output) => output,
        Err(riff_catalog_sourcify::SourcifyError::Solc(
            riff_catalog_solc::SolcError::VersionMismatch { found, required },
        )) => {
            println!("skipped {spec}: needs solc {found}, resolver accepts {required}");
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    if let Err(error) = output.check_errors() {
        println!("skipped {spec}: recompilation errors:\n{error}");
        return Ok(());
    }

    let mut records = Vec::new();
    let origin = format!("sourcify:{}:{}", id.chain_id, id.address);

    // Source ASTs for every source file in the verified bundle.
    for (source_path, _) in &contract.sources {
        let Ok(ast) = output.source_ast(source_path) else {
            continue;
        };
        let owner = format!("sf:{}:{}:{source_path}", id.chain_id, id.address);
        let artifact = artifact_id(&owner, false);
        records.push(artifact_record(
            &artifact, &owner, &origin, "via-ir", false, args,
        ));
        // Real-world contracts: never strict.
        let lowered = lower_source_unit(&ast.clone(), &owner, &WalkOptions { strict: false })?;
        for warning in lowered.warnings.iter().take(5) {
            eprintln!("warning: {warning}");
        }
        for unit in std::iter::once(&lowered.source_unit)
            .chain(&lowered.contracts)
            .chain(&lowered.functions)
        {
            emit_unit(
                &mut records,
                &artifact,
                &owner,
                SOL_AST_LEVEL,
                unit.unit,
                &unit.name,
                &unit.graph_key,
                &unit.graph,
            )?;
        }
    }

    // IR levels for the verified target contract wherever it appears.
    for (source, name) in output.contract_names() {
        if name != contract.contract_name {
            continue;
        }
        ingest_contract_ir(
            &mut records,
            args,
            &output,
            &origin,
            &source,
            &name,
            "sf",
            false,
        )?;
    }

    let stem = format!(
        "sf_{}_{}",
        id.chain_id,
        &id.address[2..10.min(id.address.len())]
    );
    corpus.append(&stem, &records)?;
    println!("ingested {spec}: {} records", records.len());
    Ok(())
}

fn artifact_record(
    artifact: &str,
    owner: &str,
    origin: &str,
    pipeline: &str,
    optimize: bool,
    args: &IngestArgs,
) -> Record {
    Record::Artifact {
        artifact_id: artifact.to_string(),
        owner: owner.to_string(),
        origin: origin.to_string(),
        pipeline: pipeline.to_string(),
        optimize,
        label: args.label.clone(),
    }
}
