use std::{collections::BTreeMap, fs, path::PathBuf};

use anyhow::{Context, Result, bail, ensure};
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
    /// Lift EVM bytecode into basic blocks (`evm-dataflow/1`) and write each
    /// block's bytes by role and its flat and dataflow facet addresses.
    EvmDataflow {
        /// Runtime artifact (raw bytes or hex).
        artifact: PathBuf,
        /// Lift only the first N bytes (exclude trailing data or metadata).
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
        /// For every --pattern, also trace the nearest later instruction that
        /// lowers from this post-opt operation (for example `evm_malloc`).
        #[arg(long)]
        anchor_op: Option<String>,
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
        /// End of the instructions (defaults to the whole artifact).
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
        /// Report repeated EVM runs of at least this many bytes per copy.
        #[arg(long, default_value_t = 32)]
        min_run_bytes: u32,
        /// Compare PUSH constants as ports too (copies that differ only in
        /// constants group together).
        #[arg(long)]
        constants_as_ports: bool,
        /// Runs of fewer instructions are not reported.
        #[arg(long, default_value_t = 2)]
        min_run_instructions: u32,
        /// Compare only constants that are memory or calldata offsets as
        /// ports (the same code for different struct layouts groups together).
        #[arg(long)]
        memory_offsets_as_ports: bool,
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
        Command::EvmDataflow {
            artifact,
            code_end,
            out,
            regions,
            attribution,
            contract,
            min_block_bytes,
            report_out,
            top,
        } => {
            let bytes = decode_artifact(&fs::read(&artifact)?)?;
            let end = code_end.unwrap_or(bytes.len()).min(bytes.len());
            let blocks = evm_dataflow_blocks(&bytes[..end])?;
            fs::write(&out, serde_json::to_vec(&blocks)?)?;
            let functions: Vec<(String, u32, u32)> = match &regions {
                Some(path) => {
                    let manifest: RegionManifest = serde_json::from_slice(&fs::read(path)?)?;
                    manifest
                        .regions
                        .iter()
                        .filter(|r| r.kind == "function")
                        .map(|r| (r.name.clone(), r.start as u32, r.end as u32))
                        .collect()
                }
                None => Vec::new(),
            };
            let no_source: Option<Vec<(u32, u32)>> = match (&attribution, &contract) {
                (Some(path), Some(contract)) => Some(
                    riff_catalog_ingest_trace::bytes::read_runtime_details(
                        &fs::read_to_string(path)?,
                        contract,
                    )?
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
            regions,
            census,
            runs,
            function_prefix,
            pattern,
            pc_set,
            anchor_op,
            mechanisms_out,
            json_out,
            top,
        } => {
            let code = decode_artifact(&fs::read(&artifact)?)?;
            let rows = riff_catalog_ingest_trace::bytes::read_runtime_details(
                &fs::read_to_string(&attribution)?,
                &contract,
            )?;
            let code_len = rows.last().map_or(0, |r| r.pc_end as usize);
            let code = &code[..code_len.min(code.len())];
            let reader = std::io::BufReader::new(
                fs::File::open(&trace).with_context(|| format!("open {}", trace.display()))?,
            );
            let graph = riff_catalog_ingest_trace::stages::StageGraph::read(reader)?;
            let manifest: RegionManifest = serde_json::from_slice(&fs::read(&regions)?)?;
            let functions: Vec<&RegionSpec> = manifest
                .regions
                .iter()
                .filter(|r| r.kind == "function")
                .collect();
            let mut selections = vec![
                range_selection("all", code, &[(0, code.len() as u32)]),
                opcode_selection("memory_ops", code, &MEMORY_OPCODES),
                Selection {
                    name: "no_source".into(),
                    pcs: rows
                        .iter()
                        .filter(|r| r.has_no_source())
                        .map(|r| r.pc_start)
                        .collect(),
                },
            ];
            let mut clamp = std::collections::BTreeSet::new();
            for p in &pattern {
                let (name, hex) = p.split_once('=').context("--pattern name=hex")?;
                let sel = pattern_selection(name, code, hex)?;
                if name == "free_pointer_clamp" {
                    clamp = sel.pcs.clone();
                }
                selections.push(sel);
                selections.push(pattern_next_selection(
                    &format!("{name} (next instruction after each copy)"),
                    code,
                    hex,
                )?);
            }
            let mut spill = std::collections::BTreeSet::new();
            for p in &pc_set {
                let (name, path) = p.split_once('=').context("--pc-set name=path")?;
                let value: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
                let items = value
                    .as_array()
                    .or_else(|| value["entries"].as_array())
                    .context("pc set: expected an array or an object with entries")?;
                let pcs: std::collections::BTreeSet<u32> = items
                    .iter()
                    .filter_map(|e| e.as_u64().or_else(|| e["pc"].as_u64()))
                    .map(|v| v as u32)
                    .collect();
                if name == "backend_spill" {
                    spill = pcs.clone();
                }
                selections.push(Selection {
                    name: name.to_string(),
                    pcs,
                });
            }
            if let Some(path) = census {
                let value: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
                let by_id: BTreeMap<String, (u32, u32)> = value["regions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|r| {
                        let r = &r["region"];
                        Some((
                            r["id"].as_str()?.to_string(),
                            (r["start"].as_u64()? as u32, r["end"].as_u64()? as u32),
                        ))
                    })
                    .collect();
                let mut patterns: Vec<&serde_json::Value> = value["patterns"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|p| p["region_kind"] == "evm_run")
                    .collect();
                patterns.sort_by_key(|p| std::cmp::Reverse(p["covered_bytes"].as_u64()));
                for p in patterns.into_iter().take(runs) {
                    let ranges: Vec<(u32, u32)> = p["regions"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|id| by_id.get(id.as_str()?).copied())
                        .collect();
                    let digest = p["digest"].as_str().unwrap_or("");
                    let name = format!(
                        "run {} ({} copies x {} bytes)",
                        &digest[..12.min(digest.len())],
                        ranges.len(),
                        ranges.first().map_or(0, |r| r.1 - r.0)
                    );
                    selections.push(range_selection(&name, code, &ranges));
                }
            }
            let insts = riff_catalog_evm::runs::decode(code);
            for prefix in &function_prefix {
                for f in functions
                    .iter()
                    .filter(|f| f.name.starts_with(prefix.as_str()))
                {
                    selections.push(range_selection(
                        &format!("function {}", f.name),
                        code,
                        &[(f.start as u32, f.end as u32)],
                    ));
                    let entry = f.start as u64;
                    let callers: std::collections::BTreeSet<u32> = insts
                        .iter()
                        .filter(|i| {
                            (0x60..=0x63).contains(&i.opcode)
                                && i.len as usize == usize::from(i.opcode - 0x5e)
                        })
                        .filter(|i| {
                            code[i.pc as usize + 1..(i.pc + i.len) as usize]
                                .iter()
                                .fold(0u64, |a, b| (a << 8) | u64::from(*b))
                                == entry
                        })
                        .map(|i| i.pc)
                        .collect();
                    selections.push(Selection {
                        name: format!("call sites of {} (entry label pushes)", f.name),
                        pcs: callers,
                    });
                }
            }
            let inputs = StageInputs {
                graph: &graph,
                rows: &rows,
                code,
            };
            if let Some(dir) = &mechanisms_out {
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
            if let Some(op) = &anchor_op {
                for p in &pattern {
                    let (name, hex) = p.split_once('=').context("--pattern name=hex")?;
                    let ranges = pattern_matches(code, hex)?;
                    selections.push(inputs.nearest_after(
                        &format!("{name} (nearest later instruction lowering from {op})"),
                        &ranges,
                        op,
                        48,
                    ));
                }
            }
            let (chain_classes, top_constructs) = inputs.chain_classes(top.max(40))?;
            let report = FeStagesReport {
                schema: FE_STAGES_SCHEMA.into(),
                contract: contract.clone(),
                stage_graph_nodes: graph.len(),
                selections: selections
                    .iter()
                    .map(|s| inputs.report(s, &clamp, &spill, top))
                    .collect(),
                expansion_by_body: inputs.expansion_by_body(),
                chain_classes,
                top_constructs,
                category_by_function: inputs.category_by_function(
                    &functions
                        .iter()
                        .map(|f| (f.name.clone(), f.start as u32, f.end as u32))
                        .collect::<Vec<_>>(),
                ),
            };
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
                constants_as_ports: false,
                min_run_instructions: 2,
                memory_offsets_as_ports: false,
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
            let fe: FeStagesReport = serde_json::from_slice(&fs::read(&fe_stages)?)?;
            let mut builds = Vec::new();
            let mut tables = BTreeMap::new();
            for item in &solc {
                let (name, path) = item.split_once('=').context("--solc name=path")?;
                let table: SolcFunctions = serde_json::from_slice(&fs::read(path)?)?;
                builds.push(name.to_string());
                tables.insert(name.to_string(), table);
            }
            let pairs: Vec<FunctionPair> = serde_json::from_slice(&fs::read(&pairs)?)?;
            let mut rows = compare_functions(&fe, &tables, &pairs);
            if let Some(total) = fe_total {
                let residual = residual_row(&rows, total, &tables);
                rows.push(residual);
            }
            if let Some(path) = json_out {
                fs::write(&path, serde_json::to_vec_pretty(&rows)?)?;
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
            let l: DataflowBlocks = serde_json::from_slice(&fs::read(&left)?)?;
            let r: DataflowBlocks = serde_json::from_slice(&fs::read(&right)?)?;
            let cmp = compare_blocks(&l, &r, &min_block_bytes, top);
            if let Some(path) = json_out {
                fs::write(&path, serde_json::to_vec_pretty(&cmp)?)?;
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
                let manifest: RegionManifest = serde_json::from_slice(&fs::read(path)?)?;
                for r in manifest.regions.iter().filter(|r| r.kind == "function") {
                    *bytes.entry(r.name.clone()).or_default() += (r.end - r.start) as u64;
                }
            }
            let census = sonatina_function_facets(&source, &bytes)?;
            if let Some(path) = json_out {
                fs::write(&path, serde_json::to_vec_pretty(&census)?)?;
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
            code_end,
            regions,
            cause,
            detail,
            attribution,
            contract,
            json_out,
        } => {
            let code = decode_artifact(&fs::read(&artifact)?)?;
            let end = code_end.unwrap_or(code.len()).min(code.len());
            let manifest: RegionManifest = serde_json::from_slice(&fs::read(&regions)?)?;
            let functions: Vec<(String, u32, u32)> = manifest
                .regions
                .iter()
                .filter(|r| r.kind == "function")
                .map(|r| (r.name.clone(), r.start as u32, r.end as u32))
                .collect();
            let rows = match (&attribution, &contract) {
                (Some(p), Some(c)) => Some(riff_catalog_ingest_trace::bytes::read_runtime_details(
                    &fs::read_to_string(p)?,
                    c,
                )?),
                _ => None,
            };
            let insts = riff_catalog_evm::runs::decode(&code[..end]);
            let mut causes = Vec::new();
            let mut details = Vec::new();
            for (is_detail, spec) in cause
                .iter()
                .map(|c| (false, c))
                .chain(detail.iter().map(|d| (true, d)))
            {
                let (kind, rest) = spec.split_once(':').context("--cause kind:NAME=arg")?;
                let (name, arg) = rest.split_once('=').context("--cause kind:NAME=arg")?;
                let pcs: std::collections::BTreeSet<u32> = match kind {
                    "pcs" => {
                        let v: serde_json::Value = serde_json::from_slice(&fs::read(arg)?)?;
                        v.as_array()
                            .or_else(|| v["entries"].as_array())
                            .context("pc set")?
                            .iter()
                            .filter_map(|e| e.as_u64().or_else(|| e["pc"].as_u64()))
                            .map(|v| v as u32)
                            .collect()
                    }
                    "pattern" => pattern_selection(name, &code[..end], arg)?.pcs,
                    "repeats" => {
                        let v: serde_json::Value = serde_json::from_slice(&fs::read(arg)?)?;
                        let by_id: BTreeMap<String, (u32, u32)> = v["regions"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|r| {
                                let r = &r["region"];
                                Some((
                                    r["id"].as_str()?.to_string(),
                                    (r["start"].as_u64()? as u32, r["end"].as_u64()? as u32),
                                ))
                            })
                            .collect();
                        let mut ranges = Vec::new();
                        for p in v["patterns"].as_array().into_iter().flatten() {
                            if p["region_kind"] != "evm_run" {
                                continue;
                            }
                            let mut occ: Vec<(u32, u32)> = p["regions"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .filter_map(|id| by_id.get(id.as_str()?).copied())
                                .collect();
                            occ.sort();
                            ranges.extend(occ.into_iter().skip(1));
                        }
                        range_selection(name, &code[..end], &ranges).pcs
                    }
                    "duplicates" => {
                        let (path, index) = arg.split_once('#').unwrap_or((arg, "0"));
                        let census: Vec<FunctionFacetCensus> =
                            serde_json::from_slice(&fs::read(path)?)?;
                        let facet = census
                            .get(index.parse::<usize>()?)
                            .context("duplicates facet index")?;
                        let mut ranges = Vec::new();
                        for class in &facet.classes {
                            let largest = class
                                .functions
                                .iter()
                                .filter_map(|(n, b)| b.map(|b| (b, n.clone())))
                                .max();
                            for (n, b) in &class.functions {
                                if b.is_some() && largest.as_ref().map(|l| &l.1) != Some(n) {
                                    ranges.extend(
                                        functions.iter().filter(|f| &f.0 == n).map(|f| (f.1, f.2)),
                                    );
                                }
                            }
                        }
                        range_selection(name, &code[..end], &ranges).pcs
                    }
                    "regions" => {
                        let needles: Vec<&str> = arg.split('|').collect();
                        let ranges: Vec<(u32, u32)> = functions
                            .iter()
                            .filter(|f| needles.iter().any(|n| f.0.contains(n)))
                            .map(|f| (f.1, f.2))
                            .collect();
                        range_selection(name, &code[..end], &ranges).pcs
                    }
                    "bodies" => {
                        let rows = rows.as_ref().context("bodies: needs --attribution")?;
                        let needles: Vec<&str> = arg.split('|').collect();
                        let hit = |key: &str| {
                            riff_catalog_ingest_trace::bytes::source_body(key)
                                .is_some_and(|b| needles.iter().any(|n| b.contains(n)))
                        };
                        rows.iter()
                            .filter(|r| match &r.primary_source {
                                Some(p) => hit(p),
                                None => {
                                    r.classification_reason.as_deref() == Some("SyntheticFor")
                                        && r.all_origins.iter().any(|o| hit(o))
                                }
                            })
                            .map(|r| r.pc_start)
                            .collect()
                    }
                    other => bail!("unknown cause kind `{other}`"),
                };
                let sel = Selection {
                    name: name.to_string(),
                    pcs,
                };
                if is_detail {
                    details.push(sel);
                } else {
                    causes.push(sel);
                }
            }
            let labels = riff_catalog_evm::runs::with_labels(&code[..end], &insts)
                .into_iter()
                .filter(|(_, l)| l.is_some())
                .map(|(i, _)| i.pc)
                .collect();
            let memory_address = riff_catalog_evm::dataflow::lift_code(&code[..end])?
                .into_iter()
                .flat_map(|b| b.memory_offset_pushes)
                .collect();
            let ledger = classify_bytes(
                &code,
                end,
                &causes,
                &labels,
                &memory_address,
                &functions,
                &details,
            );
            let total: u64 = ledger.buckets.values().map(|t| t.bytes).sum();
            ensure!(
                total == ledger.artifact_bytes,
                "buckets cover {total} of {} bytes",
                ledger.artifact_bytes
            );
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
            let l: ByteCauses = serde_json::from_slice(&fs::read(&left)?)?;
            let r: ByteCauses = serde_json::from_slice(&fs::read(&right)?)?;
            let rows = compare_causes(&l, &r);
            let excess = l.artifact_bytes as i64 - r.artifact_bytes as i64;
            ensure!(
                rows.iter().map(|x| x.excess).sum::<i64>() == excess,
                "excess does not add up"
            );
            if let Some(path) = json_out {
                fs::write(&path, serde_json::to_vec_pretty(&rows)?)?;
            }
            println!(
                "{:>7} {:>7} {:>7} {:>6}  bucket",
                "left", "right", "excess", "share"
            );
            for x in &rows {
                println!(
                    "{:>7} {:>7} {:>7} {:>5.1}%  {}",
                    x.left,
                    x.right,
                    x.excess,
                    100.0 * x.excess as f64 / excess as f64,
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
                    println!("{v:>9.0} {:>5.1}%  {name}", 100.0 * v / excess as f64);
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
            min_run_bytes,
            constants_as_ports,
            min_run_instructions,
            memory_offsets_as_ports,
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
                    min_run_instructions,
                    memory_offsets_as_ports,
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
