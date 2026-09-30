use std::{collections::BTreeMap, fs, path::PathBuf};

use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use riff_catalog_bloat::{EvmRunKey, EvmRunOptions};
use riff_catalog_bytes::*;

#[derive(Parser)]
#[command(
    name = "riffcat-bytes",
    about = "Where the bytes of an EVM runtime come from"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Lift EVM bytecode into basic blocks (`evm-dataflow/2`) and write each
    /// block's bytes by role and its flat and dataflow facet addresses.
    EvmDataflow {
        /// Runtime artifact (raw bytes or hex).
        artifact: PathBuf,
        /// How the artifact file is written: `auto` (hex text when it is only
        /// hex digits, else raw bytes), `hex` or `raw`.
        #[arg(long, value_enum, default_value = "auto")]
        artifact_format: ArtifactFormat,
        /// Lift only the first N bytes. Default: up to the manifest's data
        /// region, else up to a solc metadata trailer, else everything.
        #[arg(long)]
        code_end: Option<usize>,
        /// Write every block with its facet addresses as JSON.
        #[arg(long)]
        out: PathBuf,
        /// riffcat-regions/1 manifest whose `function` regions name functions.
        #[arg(long)]
        regions: Option<PathBuf>,
        /// Fe attribution details, to count scheduling bytes with no source.
        #[arg(long, requires = "contract")]
        attribution: Option<PathBuf>,
        #[arg(long)]
        contract: Option<String>,
        /// Minimum block sizes for the per-facet block census.
        #[arg(long, value_delimiter = ',', default_value = "8,16,32")]
        min_block_bytes: Vec<u32>,
        /// Write the report as JSON.
        #[arg(long)]
        report_out: Option<PathBuf>,
        #[arg(long, default_value_t = 15)]
        top: usize,
    },
    /// Trace emitted bytes back through Fe's compiler stages: named
    /// selections (memory operations, byte patterns, run classes, functions
    /// and their call sites, no-source bytes), expansion per Fe body, and
    /// content-addressed expansion chains.
    FeTraceStages {
        #[arg(long)]
        trace: PathBuf,
        #[arg(long)]
        attribution: PathBuf,
        #[arg(long)]
        contract: String,
        #[arg(long)]
        artifact: PathBuf,
        /// How the artifact file is written: `auto` (hex text when it is only
        /// hex digits, else raw bytes), `hex` or `raw`.
        #[arg(long, value_enum, default_value = "auto")]
        artifact_format: ArtifactFormat,
        /// riffcat-regions/1 manifest with `function` regions.
        #[arg(long)]
        regions: PathBuf,
        /// Census output (`census --json`) whose EVM run classes to trace.
        #[arg(long)]
        census: Option<PathBuf>,
        /// How many of the census's largest run classes to trace.
        #[arg(long, default_value_t = 6)]
        runs: usize,
        /// Trace every function whose name starts with this, and its call sites.
        #[arg(long)]
        function_prefix: Vec<String>,
        /// A named byte pattern, `name=hex` with `??` for any byte.
        #[arg(long)]
        pattern: Vec<String>,
        /// A named pc set, `name=path`: a JSON array of pcs, or an object whose
        /// `entries` are objects with a `pc` (for example Sonatina memory-plan
        /// tags). The name `backend_spill` feeds the memory-origin buckets.
        #[arg(long)]
        pc_set: Vec<String>,
        /// Write every instruction's mechanism as pc sets: a directory with
        /// one JSON array of pcs per mechanism, plus an index.
        #[arg(long)]
        mechanisms_out: Option<PathBuf>,
        #[arg(long)]
        json_out: Option<PathBuf>,
        #[arg(long, default_value_t = 12)]
        top: usize,
    },
    /// Function regions of a solc runtime from its source map and AST, as a
    /// riffcat-regions/1 manifest the census and dataflow commands take.
    SolcFunctions {
        /// solc standard-JSON output with ASTs and deployedBytecode.sourceMap.
        #[arg(long)]
        standard_output: PathBuf,
        #[arg(long)]
        source: String,
        #[arg(long)]
        contract: String,
        /// Where to write the runtime bytes.
        #[arg(long)]
        artifact_out: PathBuf,
        #[arg(long)]
        manifest_out: PathBuf,
        #[arg(long)]
        json_out: Option<PathBuf>,
        /// Ask the manifest for EVM runs of at least this many bytes.
        #[arg(long)]
        min_run_bytes: Option<u32>,
        #[arg(long, default_value_t = 20)]
        top: usize,
    },
    /// Compare bytes per source function between a Fe stage report and solc
    /// function reports over a pairing file.
    CompareFunctions {
        /// `fe-trace-stages --json-out` report.
        #[arg(long)]
        fe_stages: PathBuf,
        /// `name=path` of a `solc-functions --json-out` report; repeatable.
        #[arg(long)]
        solc: Vec<String>,
        /// JSON list of pairs: {label, fe: [bodies], solc: {build: [names]}, confidence}.
        #[arg(long)]
        pairs: PathBuf,
        /// Fe artifact bytes, for the residual row.
        #[arg(long)]
        fe_total: Option<u64>,
        #[arg(long)]
        json_out: Option<PathBuf>,
    },
    /// Blocks two artifacts share at each facet (from `evm-dataflow --out`).
    EvmDataflowCompare {
        #[arg(long)]
        left: PathBuf,
        #[arg(long)]
        right: PathBuf,
        #[arg(long, value_delimiter = ',', default_value = "8,16,32")]
        min_block_bytes: Vec<u32>,
        #[arg(long)]
        json_out: Option<PathBuf>,
        #[arg(long, default_value_t = 10)]
        top: usize,
    },
    /// Group a Sonatina IR module's functions by facet (exact, types blind,
    /// types and constants blind), with emitted bytes from a regions manifest.
    SonatinaFunctions {
        #[arg(long)]
        ir: PathBuf,
        /// riffcat-regions/1 manifest whose `function` regions name emitted functions.
        #[arg(long)]
        regions: Option<PathBuf>,
        #[arg(long)]
        json_out: Option<PathBuf>,
        #[arg(long, default_value_t = 12)]
        top: usize,
    },
    /// Put every byte of an EVM runtime in one cause bucket: named causes in
    /// the order given, then role buckets by opcode.
    EvmByteCauses {
        artifact: PathBuf,
        /// How the artifact file is written: `auto` (hex text when it is only
        /// hex digits, else raw bytes), `hex` or `raw`.
        #[arg(long, value_enum, default_value = "auto")]
        artifact_format: ArtifactFormat,
        /// End of the instructions. Default: the start of the manifest's
        /// data region, else of a solc metadata trailer, else the end.
        #[arg(long)]
        code_end: Option<usize>,
        /// riffcat-regions/1 manifest with `function` regions.
        #[arg(long)]
        regions: PathBuf,
        /// A named cause, in priority order: `pcs:NAME=path.json` (an array
        /// of pcs or an object with `entries[].pc`), `pattern:NAME=hex`,
        /// `repeats:NAME=census.json` (run occurrences beyond each class's
        /// first copy), `duplicates:NAME=sonatina-functions.json#facet-index`
        /// (functions beyond the largest copy of each class),
        /// `regions:NAME=a|b` (function regions whose name contains one),
        /// `bodies:NAME=a|b` (Fe primary or synthetic-for source bodies
        /// containing one; needs --attribution).
        #[arg(long)]
        cause: Vec<String>,
        /// A detail label, same syntax as --cause: cross-tabulates each
        /// bucket's bytes by the first detail set holding the instruction.
        #[arg(long)]
        detail: Vec<String>,
        #[arg(long, requires = "contract")]
        attribution: Option<PathBuf>,
        #[arg(long)]
        contract: Option<String>,
        #[arg(long)]
        json_out: PathBuf,
    },
    /// Compare two byte-cause ledgers bucket by bucket.
    EvmByteCausesCompare {
        #[arg(long)]
        left: PathBuf,
        #[arg(long)]
        right: PathBuf,
        #[arg(long)]
        json_out: Option<PathBuf>,
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
        /// How the artifact file is written: `auto` (hex text when it is only
        /// hex digits, else raw bytes), `hex` or `raw`.
        #[arg(long, value_enum, default_value = "auto")]
        artifact_format: ArtifactFormat,
        /// Report repeated EVM runs of at least this many bytes per copy.
        #[arg(long, default_value_t = 32)]
        min_run_bytes: u32,
        /// Run key: `exact`, `memory-offsets-blind` (constants classed as
        /// memory or calldata offsets are ports, so the same code for another
        /// struct layout groups together) or `constants-blind` (every constant
        /// is a port).
        #[arg(long, value_enum, default_value = "exact")]
        run_key: EvmRunKey,
        /// Runs of fewer instructions are not reported.
        #[arg(long, default_value_t = 2)]
        min_run_instructions: u32,
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
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::EvmDataflow {
            artifact,
            artifact_format,
            code_end,
            out,
            regions,
            attribution,
            contract,
            min_block_bytes,
            report_out,
            top,
        } => {
            let bytes = load_artifact(&artifact, artifact_format)?;
            let manifest = match &regions {
                Some(path) => Some(load_regions(path, &bytes)?),
                None => None,
            };
            let end = riff_catalog_bytes::code_end(&bytes, code_end, manifest.as_ref())?;
            let blocks = evm_dataflow_blocks(&bytes[..end])?;
            fs::write(&out, serde_json::to_vec(&blocks)?)?;
            let functions = manifest
                .as_ref()
                .map(FunctionRegions::from_manifest)
                .unwrap_or_default();
            let no_source: Option<Vec<(u32, u32)>> = match (&attribution, &contract) {
                (Some(path), Some(contract)) => Some(
                    load_details(path, contract, &bytes[..end])?
                        .iter()
                        .filter(|r| r.has_no_source())
                        .map(|r| (r.pc_start, r.pc_end))
                        .collect(),
                ),
                _ => None,
            };
            let report = dataflow_report(
                &blocks,
                &bytes[..end],
                &functions,
                no_source.as_deref(),
                &min_block_bytes,
                top,
            );
            if let Some(path) = report_out {
                fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
            }
            print!("{}", render_dataflow_report(&report, top));
        }
        Command::FeTraceStages {
            trace,
            attribution,
            contract,
            artifact,
            artifact_format,
            regions,
            census,
            runs,
            function_prefix,
            pattern,
            pc_set,
            mechanisms_out,
            json_out,
            top,
        } => {
            let artifact_bytes = load_artifact(&artifact, artifact_format)?;
            let details = fs::read_to_string(&attribution)
                .with_context(|| format!("read {}", attribution.display()))?;
            let open = || -> Result<_> {
                Ok(std::io::BufReader::new(
                    fs::File::open(&trace).with_context(|| format!("open {}", trace.display()))?,
                ))
            };
            // The trace, the details and the artifact must describe the same
            // code before any stage is traced.
            let (ledger, _) = read_checked_ledger(open()?, &details, &contract, &artifact_bytes)?;
            let rows = riff_catalog_ingest_trace::bytes::read_runtime_details(&details, &contract)?;
            ensure!(
                rows.iter()
                    .all(|r| r.code_object.as_deref() == Some(ledger.code_object.as_str())),
                "the attribution rows for contract `{contract}` are not from the code object the trace names for it"
            );
            let code = &artifact_bytes[..ledger.code_len as usize];
            let graph = riff_catalog_ingest_trace::stages::StageGraph::read(open()?)
                .with_context(|| format!("read {}", trace.display()))?;
            let manifest = load_regions(&regions, &artifact_bytes)?;
            let functions = FunctionRegions::from_manifest(&manifest);
            let patterns = pattern
                .iter()
                .map(|p| named(p, "--pattern").map(|(n, h)| (n.to_string(), h.to_string())))
                .collect::<Result<Vec<_>>>()?;
            let pc_sets = pc_set
                .iter()
                .map(|p| {
                    let (name, path) = named(p, "--pc-set")?;
                    let set = read_pc_set(name, &read_file(std::path::Path::new(path))?)
                        .with_context(|| format!("pc set {path}"))?;
                    check_instruction_starts(&set, code)
                        .with_context(|| format!("pc set {path}"))?;
                    Ok(set)
                })
                .collect::<Result<Vec<_>>>()?;
            let census_runs = match census {
                Some(path) => parse_census_runs(
                    &read_file(&path)?,
                    &artifact_bytes,
                    &path.display().to_string(),
                )?,
                None => Vec::new(),
            };
            let request = StageRequest {
                contract: &contract,
                functions: &functions,
                patterns: &patterns,
                pc_sets: &pc_sets,
                census_runs: &census_runs,
                runs,
                function_prefixes: &function_prefix,
                top,
            };
            let inputs = StageInputs {
                graph: &graph,
                rows: &rows,
                code,
            };
            if let Some(dir) = &mechanisms_out {
                let (_, clamp, spill) = inputs.selections(&request)?;
                fs::create_dir_all(dir)?;
                let mut index = BTreeMap::new();
                for (k, (name, pcs)) in inputs
                    .byte_mechanisms(&clamp, &spill)
                    .into_iter()
                    .enumerate()
                {
                    let file = format!("mechanism-{k}.json");
                    fs::write(dir.join(&file), serde_json::to_vec(&pcs)?)?;
                    index.insert(name, file);
                }
                fs::write(dir.join("index.json"), serde_json::to_vec_pretty(&index)?)?;
            }
            let report = inputs.stage_report(&request)?;
            if let Some(path) = json_out {
                fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
            }
            print!("{}", render_fe_stages(&report, top));
        }
        Command::SolcFunctions {
            standard_output,
            source,
            contract,
            artifact_out,
            manifest_out,
            json_out,
            min_run_bytes,
            top,
        } => {
            let raw: serde_json::Value = serde_json::from_slice(&fs::read(&standard_output)?)?;
            let output = riff_catalog_solc::SolcOutput::new(raw);
            let evm_runs = min_run_bytes.map(|m| EvmRunOptions {
                min_run_bytes: m,
                run_key: EvmRunKey::Exact,
                min_run_instructions: 2,
            });
            let (code, report, manifest) = solc_functions(&output, &source, &contract, evm_runs)?;
            fs::write(&artifact_out, &code)?;
            fs::write(&manifest_out, serde_json::to_vec_pretty(&manifest)?)?;
            if let Some(path) = json_out {
                fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
            }
            println!(
                "{} {}: {} runtime bytes, code ends at {}",
                source, contract, report.runtime_bytes, report.code_end
            );
            for (owner, bytes) in report.by_owner.iter().take(top) {
                println!("{bytes:>7}  {owner}");
            }
        }
        Command::CompareFunctions {
            fe_stages,
            solc,
            pairs,
            fe_total,
            json_out,
        } => {
            let fe: FeStagesReport = read_json(&fe_stages)?;
            check_schema(&fe.schema, FE_STAGES_SCHEMA, &fe_stages)?;
            let mut builds = Vec::new();
            let mut tables = BTreeMap::new();
            for item in &solc {
                let (name, path) = named(item, "--solc")?;
                let path = std::path::Path::new(path);
                let table: SolcFunctions = read_json(path)?;
                check_schema(&table.schema, SOLC_FUNCTIONS_SCHEMA, path)?;
                ensure!(
                    tables.insert(name.to_string(), table).is_none(),
                    "--solc names build `{name}` twice"
                );
                builds.push(name.to_string());
            }
            let pairs: Vec<FunctionPair> = read_json(&pairs)?;
            let mut rows = compare_functions(&fe, &tables, &pairs)?;
            if let Some(total) = fe_total {
                let residual = residual_row(&rows, total, &tables);
                rows.push(residual);
            }
            if let Some(path) = json_out {
                let report = FunctionComparisonReport {
                    schema: FUNCTION_COMPARISON_SCHEMA.into(),
                    builds: builds.clone(),
                    rows: rows.clone(),
                };
                fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
            }
            print!("{}", render_function_comparison(&rows, &builds));
        }
        Command::EvmDataflowCompare {
            left,
            right,
            min_block_bytes,
            json_out,
            top,
        } => {
            let l: DataflowBlocks = read_json(&left)?;
            check_schema(&l.schema, EVM_DATAFLOW_BLOCKS_SCHEMA, &left)?;
            let r: DataflowBlocks = read_json(&right)?;
            check_schema(&r.schema, EVM_DATAFLOW_BLOCKS_SCHEMA, &right)?;
            let cmp = compare_blocks(&l, &r, &min_block_bytes, top)?;
            if let Some(path) = json_out {
                let report = DataflowComparison {
                    schema: EVM_DATAFLOW_COMPARE_SCHEMA.into(),
                    facets: cmp.clone(),
                };
                fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
            }
            println!(
                "facet, min block bytes: shared addresses; left blocks/bytes; right blocks/bytes"
            );
            for c in &cmp {
                println!(
                    "{:48} {:>3}: {:>5}; {:>5} {:>7}; {:>5} {:>7}",
                    c.facet,
                    c.min_block_bytes,
                    c.shared_addresses,
                    c.left_blocks,
                    c.left_bytes,
                    c.right_blocks,
                    c.right_bytes
                );
            }
        }
        Command::SonatinaFunctions {
            ir,
            regions,
            json_out,
            top,
        } => {
            let source = fs::read_to_string(&ir)?;
            let mut bytes: BTreeMap<String, u64> = BTreeMap::new();
            if let Some(path) = regions {
                let manifest = load_regions_unbound(&path)?;
                for r in manifest.regions.iter().filter(|r| r.kind == "function") {
                    *bytes.entry(r.name.clone()).or_default() += (r.end - r.start) as u64;
                }
            }
            let census = sonatina_function_facets(&source, &bytes)?;
            if let Some(path) = json_out {
                let report = SonatinaFunctions {
                    schema: SONATINA_FUNCTIONS_SCHEMA.into(),
                    facets: census.clone(),
                };
                fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
            }
            for c in &census {
                println!(
                    "\n== {}: {} functions, {} classes of 2+, upper-bound saving {} bytes",
                    c.facet,
                    c.functions,
                    c.classes.len(),
                    c.upper_bound_saving
                );
                for class in c.classes.iter().take(top) {
                    let names: Vec<String> = class
                        .functions
                        .iter()
                        .map(|(n, b)| format!("{n} {}", b.map_or("-".into(), |b| b.to_string())))
                        .collect();
                    println!(
                        "   {:>6} of {:>6}  {}",
                        class.upper_bound_saving,
                        class.emitted_bytes,
                        names.join(", ")
                    );
                }
            }
        }
        Command::EvmByteCauses {
            artifact,
            artifact_format,
            code_end,
            regions,
            cause,
            detail,
            attribution,
            contract,
            json_out,
        } => {
            let code = load_artifact(&artifact, artifact_format)?;
            let manifest = load_regions(&regions, &code)?;
            let end = riff_catalog_bytes::code_end(&code, code_end, Some(&manifest))?;
            let functions = FunctionRegions::from_manifest(&manifest);
            let rows = match (&attribution, &contract) {
                (Some(path), Some(contract)) => Some(load_details(path, contract, &code[..end])?),
                _ => None,
            };
            let inputs = CauseInputs {
                artifact: &code,
                code: &code[..end],
                functions: &functions,
                rows: rows.as_deref(),
            };
            let read = |path: &str| -> Result<Vec<u8>> { Ok(fs::read(path)?) };
            let causes = cause
                .iter()
                .map(|c| cause_selection(c, &inputs, &read))
                .collect::<Result<Vec<_>>>()?;
            let details = detail
                .iter()
                .map(|d| cause_selection(d, &inputs, &read))
                .collect::<Result<Vec<_>>>()?;
            let ledger = byte_cause_ledger(&code, end, &causes, &functions, &details)?;
            let total: u64 = ledger.buckets.values().map(|t| t.bytes).sum();
            fs::write(&json_out, serde_json::to_vec_pretty(&ledger)?)?;
            for name in &ledger.order {
                if let Some(t) = ledger.buckets.get(name) {
                    println!("{:>7} {:>6}  {name}", t.bytes, t.instructions);
                }
            }
            println!("{total:>7}         total");
            for name in &ledger.order {
                if let Some(d) = ledger.detail.get(name) {
                    let mut v: Vec<(&String, &u64)> = d.iter().collect();
                    v.sort_by(|a, b| b.1.cmp(a.1));
                    println!("\n  {name}:");
                    for (k, b) in v {
                        println!("    {b:>7}  {k}");
                    }
                }
            }
        }
        Command::EvmByteCausesCompare {
            left,
            right,
            json_out,
        } => {
            let read = |path: &std::path::Path| -> Result<ByteCauses> {
                let ledger: ByteCauses = read_json(path)?;
                check_schema(&ledger.schema, BYTE_CAUSES_SCHEMA, path)?;
                ledger
                    .check()
                    .with_context(|| format!("{} is not a whole ledger", path.display()))?;
                Ok(ledger)
            };
            let (l, r) = (read(&left)?, read(&right)?);
            let rows = compare_causes(&l, &r);
            let excess = l.artifact_bytes as i64 - r.artifact_bytes as i64;
            let sum: i64 = rows.iter().map(|x| x.excess).sum();
            ensure!(
                sum == excess,
                "bucket excesses add up to {sum} bytes, but the artifacts differ by {excess}"
            );
            if let Some(path) = json_out {
                let report = CauseComparison {
                    schema: BYTE_CAUSES_COMPARE_SCHEMA.into(),
                    left_artifact_blake3: l.artifact_blake3.clone(),
                    right_artifact_blake3: r.artifact_blake3.clone(),
                    rows: rows.clone(),
                };
                fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
            }
            // A share of no excess is undefined, not NaN.
            let share = |part: f64| {
                if excess == 0 {
                    "     -".to_string()
                } else {
                    format!("{:>5.1}%", 100.0 * part / excess as f64)
                }
            };
            println!(
                "{:>7} {:>7} {:>7} {:>6}  bucket",
                "left", "right", "excess", "share"
            );
            for x in &rows {
                println!(
                    "{:>7} {:>7} {:>7} {}  {}",
                    x.left,
                    x.right,
                    x.excess,
                    share(x.excess as f64),
                    x.bucket
                );
            }
            println!(
                "{:>7} {:>7} {:>7}         total",
                l.artifact_bytes, r.artifact_bytes, excess
            );
            if !l.detail.is_empty() {
                println!("\nestimate: role excess apportioned by the left side's mechanism shares");
                let est = apportion_excess(&l, &rows);
                for (name, v) in &est {
                    println!("{v:>9.0} {}  {name}", share(*v));
                }
                println!(
                    "{:>9.0}         total",
                    est.iter().map(|(_, v)| v).sum::<f64>()
                );
            }
        }
        Command::FeTraceBytes {
            trace,
            attribution,
            contract,
            artifact,
            artifact_format,
            min_run_bytes,
            run_key,
            min_run_instructions,
            json_out,
            census_dir,
            top,
        } => {
            let bytes = load_artifact(&artifact, artifact_format)?;
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
                    run_key,
                    min_run_instructions,
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
    }
    Ok(())
}
