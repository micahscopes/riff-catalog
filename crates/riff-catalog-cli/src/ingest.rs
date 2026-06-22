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

    {
        let mut seen = std::collections::BTreeMap::new();
        for (origin, name, _) in &sources {
            if let Some(previous) = seen.insert(name.clone(), origin.clone()) {
                eprintln!(
                    "warning: `{name}` ingested from both {previous} and {origin}; \
                     artifacts stay distinct but identity-mode owners collide"
                );
            }
        }
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

/// Includes the origin (full user-supplied path / sourcify ref), so
/// src/Foo.sol and test/Foo.sol stay distinct artifacts even though their
/// basenames — and therefore their owner strings — collide (external review
/// pass 2, P2). Owner collisions still merge IDENTITY-mode node keys; the
/// ingester warns when that happens.
fn artifact_id(owner: &str, origin: &str, optimize: bool) -> String {
    let mut hasher = Sha256::new();
    hasher.update(owner.as_bytes());
    hasher.update([0x1f]);
    hasher.update(origin.as_bytes());
    hasher.update([u8::from(optimize)]);
    hex::encode(hasher.finalize())[..16].to_string()
}

fn short_origin_hash(origin: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(origin.as_bytes());
    hex::encode(hasher.finalize())[..8].to_string()
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
    let solc_version = solc.inner.version().ok().map(|v| v.to_string());
    let compiler = solc_version.as_deref();

    for optimize in args.optimize.variants() {
        let opt_tag = if optimize { "opt" } else { "noopt" };
        let options = CompileOptions {
            pipeline: Pipeline::ViaIr,
            optimize,
            ..Default::default()
        };
        let input = riff_catalog_solc::solidity_input(&inputs, &options);
        let first = solc.compile(&input)?;
        // Retry without the JSON-IR outputs if solc ICEs serializing them
        // (some ~0.8.25–0.8.29 contracts); the ingest then parses the text IR.
        let output = if first.check_errors().is_ok() {
            first
        } else {
            solc.compile(&riff_catalog_solc::strip_json_ir_outputs(input))?
        };
        output.check_errors()?;

        let mut records = Vec::new();

        // Source-level AST (identical across optimizer settings; ingest once)
        if !optimize || args.optimize == OptimizeChoice::On {
            let owner = format!("sol:{name}");
            let artifact = artifact_id(&owner, origin, false);
            records.push(artifact_record(
                &artifact, &owner, origin, "via-ir", false, compiler, args,
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
                compiler,
            )?;
        }

        let stem = format!(
            "{}-{}-{opt_tag}",
            name.trim_end_matches(".sol").replace(['/', '\\'], "_"),
            short_origin_hash(origin),
        );
        corpus.replace(&stem, &records)?;
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
    compiler: Option<&str>,
) -> Result<()> {
    // yul-ast level: unoptimized + optimized IR ASTs.
    // Interfaces/abstract contracts surface these keys as JSON null (Ok(null)),
    // which we skip quietly. A *missing* key (Err) means solc emitted no IR AST
    // at all — typically a compiler older than irAst support (< ~0.8.21), e.g. a
    // version-pinned sourcify contract — so warn loudly rather than silently
    // staging a corpus with no IR units (which makes IR-level queries empty).
    for (variant, ast_result, text_result) in [
        (
            "ir",
            output.ir_ast(source, contract),
            output.ir(source, contract),
        ),
        (
            "iropt",
            output.ir_optimized_ast(source, contract),
            output.ir_optimized(source, contract),
        ),
    ] {
        let object = match ast_result {
            Ok(value) if value.is_object() => from_solc_value(value)?,
            Ok(_) => continue, // interface/abstract contract: solc emits JSON null
            Err(_) => {
                // No IR-AST JSON. solc ICEs serializing it (and yulCFGJson) on
                // some ~0.8.25–0.8.29 contracts, and it didn't exist before
                // ~0.8.21 — but the *text* IR is clean across that whole range,
                // so parse it through riffcat's other front door. Conformance
                // guarantees the text and JSON paths produce the same AST.
                match text_result {
                    Ok(text) if !text.trim().is_empty() => {
                        parse_object(text).map_err(|error| {
                            anyhow::anyhow!(
                                "parsing {variant} IR text for {source}:{contract}: {error}"
                            )
                        })?
                    }
                    _ => {
                        eprintln!(
                            "WARN: {source}:{contract}: no {variant} IR (neither AST \
                             nor text); no {variant} yul units staged"
                        );
                        continue;
                    }
                }
            }
        };
        let owner = format!("yulir:{source}:{contract}:{variant}:{opt_tag}");
        let artifact = artifact_id(&owner, origin, optimize);
        records.push(artifact_record(
            &artifact, &owner, origin, "via-ir", optimize, compiler, args,
        ));
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

    // SSA level. As above: Ok(null) is an interface (skip quietly); Err means
    // solc emitted no yulCFGJson at all (the experimental SSA pipeline needs a
    // recent solc, ~0.8.29+), which is worth a warning.
    if want_unit(args, "ssa") {
        match output.yul_cfg_json(source, contract) {
            Ok(cfg) if cfg.is_object() => {
                let owner = format!("yulssa:{source}:{contract}:{opt_tag}");
                let artifact = artifact_id(&owner, origin, optimize);
                records.push(artifact_record(
                    &artifact, &owner, origin, "via-ir", optimize, compiler, args,
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
            Ok(_) => {}
            Err(_) => {
                eprintln!(
                    "WARN: {source}:{contract}: solc emitted no yulCFGJson \
                     (the SSA pipeline needs a recent solc, ~0.8.29+); no SSA units staged"
                );
            }
        }
    }

    // EVM level
    if want_unit(args, "evm") {
        let owner = format!("evm:{source}:{contract}:via-ir:{opt_tag}");
        let artifact = artifact_id(&owner, origin, optimize);
        records.push(artifact_record(
            &artifact, &owner, origin, "via-ir", optimize, compiler, args,
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

    // yul-ast level via our parser. Raw (e.g. fe-emitted) Yul: parsed, not
    // produced by solc, so there is no compiler version to record.
    let owner = format!("yul:{name}");
    let artifact = artifact_id(&owner, origin, false);
    records.push(artifact_record(
        &artifact, &owner, origin, "yul", false, None, args,
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
                    let ssa_artifact = artifact_id(&ssa_owner, origin, false);
                    let ssa_compiler = solc.inner.version().ok().map(|v| v.to_string());
                    records.push(artifact_record(
                        &ssa_artifact,
                        &ssa_owner,
                        origin,
                        "yul",
                        false,
                        ssa_compiler.as_deref(),
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

    let stem = format!(
        "{}-{}",
        name.trim_end_matches(".yul").replace(['/', '\\'], "_"),
        short_origin_hash(origin),
    );
    corpus.replace(&stem, &records)?;
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

    // Faithful recompilation: use the locally installed solc only when it
    // matches the verified pin exactly; otherwise download THE pinned build
    // from binaries.soliditylang.org (cached forever after).
    let resolver = riff_catalog_sourcify::PinnedOrDownload {
        download: riff_catalog_sourcify::SolcBinResolver::new(&args.cache_dir),
        installed: args.solc_path.clone(),
    };
    let output = match riff_catalog_sourcify::compile(&contract, &resolver, Pipeline::ViaIr) {
        Ok(output) => output,
        Err(riff_catalog_sourcify::SourcifyError::Solc(
            riff_catalog_solc::SolcError::Resolver(reason),
        )) => {
            println!("skipped {spec}: could not resolve pinned solc ({reason})");
            return Ok(());
        }
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
    // The verified pin — provenance only, never folded into the shape.
    let compiler = Some(contract.compiler_version.as_str());

    // Source ASTs for every source file in the verified bundle.
    for (source_path, _) in &contract.sources {
        let Ok(ast) = output.source_ast(source_path) else {
            continue;
        };
        let owner = format!("sf:{}:{}:{source_path}", id.chain_id, id.address);
        let artifact = artifact_id(&owner, &origin, false);
        records.push(artifact_record(
            &artifact, &owner, &origin, "via-ir", false, compiler, args,
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
            compiler,
        )?;
    }

    // Full address, not a prefix: vanity deployments (Permit2, EntryPoint,
    // Uniswap v4, ...) share long runs of leading zeros, and a truncated stem
    // makes them silently clobber each other's corpus files.
    let stem = format!("sf_{}_{}", id.chain_id, &id.address[2..]);
    corpus.replace(&stem, &records)?;
    println!("ingested {spec}: {} records", records.len());
    Ok(())
}

fn artifact_record(
    artifact: &str,
    owner: &str,
    origin: &str,
    pipeline: &str,
    optimize: bool,
    compiler: Option<&str>,
    args: &IngestArgs,
) -> Record {
    Record::Artifact {
        artifact_id: artifact.to_string(),
        owner: owner.to_string(),
        origin: origin.to_string(),
        pipeline: pipeline.to_string(),
        optimize,
        compiler: compiler.map(str::to_string),
        label: args.label.clone(),
    }
}
