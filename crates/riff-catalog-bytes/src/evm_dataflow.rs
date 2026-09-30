//! Per-block facet addresses of EVM bytecode, flat (`evm-run/1`) and lifted
//! (`evm-dataflow/2`), for censuses that compare them.
//!
//! Every address is a core facet address. The facets that erase one field
//! class (memory offsets, entry slots) are `riffcat-view/1` plans over
//! `evm-dataflow/2`, so their policy level names the plan.

use anyhow::{Context, Result};
use riff_catalog_core::{Dimension, Graph};
use riff_catalog_evm::dataflow::{EVM_DATAFLOW_LEVEL, address, dataflow_policy, lift_code};
pub use riff_catalog_evm::dataflow::{
    INPUT_ORDER_BLIND_VIEW, MEMORY_OFFSETS_AND_INPUT_ORDER_BLIND_VIEW, MEMORY_OFFSETS_BLIND_VIEW,
};
use riff_catalog_evm::runs::{RunKey, RunKeyer, decode, with_labels};
use riff_catalog_view::ViewPlan;
use serde::{Deserialize, Serialize};

use crate::regions::FunctionRegions;

pub const EVM_DATAFLOW_BLOCKS_SCHEMA: &str = "riffcat-evm-dataflow-blocks/2";
pub const EVM_DATAFLOW_COMPARE_SCHEMA: &str = "riffcat-evm-dataflow-compare/1";

/// The `evm-dataflow-compare` report.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataflowComparison {
    pub schema: String,
    pub facets: Vec<CrossFacet>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockRecord {
    pub start: u32,
    pub end: u32,
    pub instructions: u32,
    pub bytes: u32,
    pub scheduling_bytes: u32,
    pub dup_bytes: u32,
    pub swap_bytes: u32,
    pub pop_bytes: u32,
    pub push_bytes: u32,
    pub jumpdest_bytes: u32,
    pub consumed: usize,
    pub produced: usize,
    pub constants: usize,
    pub memory_offset_constants: usize,
    pub memory_offset_pushes: Vec<u32>,
    /// Facet name to address (hex).
    pub addresses: std::collections::BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataflowBlocks {
    pub schema: String,
    pub code_bytes: usize,
    /// Facet name to a description of what it keeps.
    pub facets: std::collections::BTreeMap<String, String>,
    pub blocks: Vec<BlockRecord>,
}

fn view_address(plan: &ViewPlan, graph: &Graph, dims: &[Dimension]) -> Result<String> {
    let projected = plan.materialize(graph)?;
    let policy = dataflow_policy(&plan.output_level())?;
    Ok(address(&projected, &policy, dims)?.to_hex())
}

/// Lift `code` (instructions only; pass the code without trailing data) and
/// address every block at each facet.
pub fn evm_dataflow_blocks(code: &[u8]) -> Result<DataflowBlocks> {
    let offsets_blind = ViewPlan::parse(MEMORY_OFFSETS_BLIND_VIEW).context("offsets view")?;
    let inputs_blind = ViewPlan::parse(INPUT_ORDER_BLIND_VIEW).context("inputs view")?;
    let both_blind =
        ViewPlan::parse(MEMORY_OFFSETS_AND_INPUT_ORDER_BLIND_VIEW).context("combined view")?;
    let policy = dataflow_policy(EVM_DATAFLOW_LEVEL)?;
    let sc = [Dimension::Structure, Dimension::Constants];
    let s = [Dimension::Structure];
    let insts = decode(code);
    let labeled = with_labels(code, &insts);
    let by_pc: std::collections::HashMap<u32, usize> =
        insts.iter().enumerate().map(|(i, x)| (x.pc, i)).collect();
    let exact_run = RunKeyer::new(code, RunKey::Exact);
    let blind_run = RunKeyer::new(code, RunKey::ConstantsBlind);
    let offsets_run = RunKeyer::new(code, RunKey::MemoryOffsetsBlind);

    let mut blocks = Vec::new();
    for block in lift_code(code)? {
        let first = by_pc[&block.start];
        let run = &labeled[first..first + block.instructions as usize];
        let mut addresses = std::collections::BTreeMap::new();
        addresses.insert("flat".into(), exact_run.address(code, run).to_hex());
        addresses.insert(
            "flat_constants_blind".into(),
            blind_run.address(code, run).to_hex(),
        );
        addresses.insert(
            "flat_memory_offsets_blind".into(),
            offsets_run.address(code, run).to_hex(),
        );
        addresses.insert(
            "dataflow".into(),
            address(&block.graph, &policy, &sc)?.to_hex(),
        );
        addresses.insert(
            "dataflow_constants_blind".into(),
            address(&block.graph, &policy, &s)?.to_hex(),
        );
        addresses.insert(
            "dataflow_memory_offsets_blind".into(),
            view_address(&offsets_blind, &block.graph, &sc)?,
        );
        addresses.insert(
            "dataflow_input_order_blind".into(),
            view_address(&inputs_blind, &block.graph, &sc)?,
        );
        addresses.insert(
            "dataflow_memory_offsets_and_input_order_blind".into(),
            view_address(&both_blind, &block.graph, &sc)?,
        );
        let b = block.bytes;
        blocks.push(BlockRecord {
            start: block.start,
            end: block.end,
            instructions: block.instructions,
            bytes: b.total,
            scheduling_bytes: b.scheduling,
            dup_bytes: b.dup,
            swap_bytes: b.swap,
            pop_bytes: b.pop,
            push_bytes: b.push,
            jumpdest_bytes: b.jumpdest,
            consumed: block.consumed,
            produced: block.produced,
            constants: block.constants,
            memory_offset_constants: block.memory_offset_constants,
            memory_offset_pushes: block.memory_offset_pushes,
            addresses,
        });
    }
    let facets = [
        (
            "flat",
            "evm-run/1, Structure + Constants: the exact bytes, labels relocated",
        ),
        (
            "flat_constants_blind",
            "evm-run/1, Structure: PUSH values forgotten",
        ),
        (
            "flat_memory_offsets_blind",
            "view evm-run.memory-offsets-blind/1, Structure + Constants",
        ),
        ("dataflow", "evm-dataflow/2, Structure + Constants"),
        ("dataflow_constants_blind", "evm-dataflow/2, Structure"),
        (
            "dataflow_memory_offsets_blind",
            "view evm-dataflow.memory-offsets-blind/2, Structure + Constants",
        ),
        (
            "dataflow_input_order_blind",
            "view evm-dataflow.input-order-blind/1, Structure + Constants",
        ),
        (
            "dataflow_memory_offsets_and_input_order_blind",
            "view evm-dataflow.memory-offsets-and-input-order-blind/1, Structure + Constants",
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    Ok(DataflowBlocks {
        schema: EVM_DATAFLOW_BLOCKS_SCHEMA.into(),
        code_bytes: code.len(),
        facets,
        blocks,
    })
}

/// Bytes of DUP, SWAP and POP.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SchedulingBytes {
    pub total: u64,
    pub dup: u64,
    pub swap: u64,
    pub pop: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionScheduling {
    pub name: String,
    pub bytes: u64,
    pub scheduling: SchedulingBytes,
}

/// One facet's census of whole blocks: blocks with equal addresses are one
/// class. `extra` is a class's bytes minus its smallest copy; `hidden` is the
/// part of `extra` that the exact flat facet does not already group (copies
/// whose bytes differ).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockFacetCensus {
    pub facet: String,
    pub min_block_bytes: u32,
    pub classes: usize,
    pub covered: u64,
    pub extra: u64,
    pub hidden: u64,
    /// Largest classes by `hidden`, then `extra`.
    pub top: Vec<BlockClass>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockClass {
    pub address: String,
    pub copies: usize,
    pub extra: u64,
    pub hidden: u64,
    pub flat_variants: usize,
    pub sizes: Vec<u32>,
    pub starts: Vec<u32>,
    /// Function name to copies in it.
    pub functions: std::collections::BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DataflowReport {
    pub schema: String,
    pub code_bytes: usize,
    pub blocks: usize,
    pub scheduling: SchedulingBytes,
    /// Scheduling bytes inside instructions with no source link, and all
    /// bytes with no source link, when attribution details were given.
    pub scheduling_in_no_source: Option<SchedulingBytes>,
    pub no_source_bytes: Option<u64>,
    pub by_function: Vec<FunctionScheduling>,
    pub facets: Vec<BlockFacetCensus>,
}

pub const EVM_DATAFLOW_REPORT_SCHEMA: &str = "riffcat-evm-dataflow-report/2";

fn add_scheduling(s: &mut SchedulingBytes, opcode: u8, len: u64) {
    match opcode {
        0x50 => s.pop += len,
        0x80..=0x8f => s.dup += len,
        0x90..=0x9f => s.swap += len,
        _ => return,
    }
    s.total += len;
}

/// Summarize lifted blocks: scheduling bytes (overall, per function, and
/// inside no-source instructions) and a whole-block census per facet.
/// `no_source` lists the pc ranges with no source link.
pub fn dataflow_report(
    blocks: &DataflowBlocks,
    code: &[u8],
    functions: &FunctionRegions,
    no_source: Option<&[(u32, u32)]>,
    min_block_bytes: &[u32],
    top: usize,
) -> DataflowReport {
    use std::collections::{BTreeMap, HashSet};
    let no_source_pcs: Option<HashSet<u32>> =
        no_source.map(|ranges| ranges.iter().map(|r| r.0).collect());
    let mut scheduling = SchedulingBytes::default();
    let mut in_no_source = SchedulingBytes::default();
    let mut per_function: BTreeMap<String, FunctionScheduling> = BTreeMap::new();
    let block_function: Vec<String> = blocks
        .blocks
        .iter()
        .map(|b| functions.label_at(b.start))
        .collect();
    for (b, name) in blocks.blocks.iter().zip(&block_function) {
        let entry = per_function
            .entry(name.clone())
            .or_insert_with(|| FunctionScheduling {
                name: name.clone(),
                bytes: 0,
                scheduling: SchedulingBytes::default(),
            });
        entry.bytes += u64::from(b.bytes);
        for inst in decode(&code[b.start as usize..b.end as usize]) {
            let len = u64::from(inst.len);
            add_scheduling(&mut scheduling, inst.opcode, len);
            add_scheduling(&mut entry.scheduling, inst.opcode, len);
            if no_source_pcs
                .as_ref()
                .is_some_and(|set| set.contains(&(b.start + inst.pc)))
            {
                add_scheduling(&mut in_no_source, inst.opcode, len);
            }
        }
    }
    let mut by_function: Vec<FunctionScheduling> = per_function.into_values().collect();
    by_function.sort_by(|a, b| {
        b.scheduling
            .total
            .cmp(&a.scheduling.total)
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut facets = Vec::new();
    for facet in blocks.facets.keys() {
        for &min in min_block_bytes {
            let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
            for (i, b) in blocks.blocks.iter().enumerate() {
                if b.bytes >= min && b.instructions >= 2 {
                    groups
                        .entry(b.addresses[facet].as_str())
                        .or_default()
                        .push(i);
                }
            }
            let extra_of = |members: &[usize]| -> u64 {
                let sum: u64 = members
                    .iter()
                    .map(|&i| u64::from(blocks.blocks[i].bytes))
                    .sum();
                let min = members
                    .iter()
                    .map(|&i| u64::from(blocks.blocks[i].bytes))
                    .min()
                    .unwrap_or(0);
                sum - min
            };
            let mut classes = Vec::new();
            for (address, members) in groups {
                if members.len() < 2 {
                    continue;
                }
                let mut flat: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
                for &i in &members {
                    flat.entry(blocks.blocks[i].addresses["flat"].as_str())
                        .or_default()
                        .push(i);
                }
                let extra = extra_of(&members);
                let flat_extra: u64 = flat
                    .values()
                    .filter(|m| m.len() > 1)
                    .map(|m| extra_of(m))
                    .sum();
                let mut functions = BTreeMap::new();
                for &i in &members {
                    *functions.entry(block_function[i].clone()).or_insert(0) += 1;
                }
                classes.push(BlockClass {
                    address: address.to_string(),
                    copies: members.len(),
                    extra,
                    hidden: extra - flat_extra,
                    flat_variants: flat.len(),
                    sizes: members.iter().map(|&i| blocks.blocks[i].bytes).collect(),
                    starts: members.iter().map(|&i| blocks.blocks[i].start).collect(),
                    functions,
                });
            }
            let covered = classes
                .iter()
                .flat_map(|c| c.sizes.iter())
                .map(|s| u64::from(*s))
                .sum();
            let extra = classes.iter().map(|c| c.extra).sum();
            let hidden = classes.iter().map(|c| c.hidden).sum();
            let count = classes.len();
            classes.sort_by(|a, b| {
                (b.hidden, b.extra, &a.address).cmp(&(a.hidden, a.extra, &b.address))
            });
            classes.truncate(top);
            facets.push(BlockFacetCensus {
                facet: facet.clone(),
                min_block_bytes: min,
                classes: count,
                covered,
                extra,
                hidden,
                top: classes,
            });
        }
    }
    DataflowReport {
        schema: EVM_DATAFLOW_REPORT_SCHEMA.into(),
        code_bytes: blocks.code_bytes,
        blocks: blocks.blocks.len(),
        scheduling,
        scheduling_in_no_source: no_source.map(|_| in_no_source),
        no_source_bytes: no_source.map(|r| r.iter().map(|(a, b)| u64::from(b - a)).sum()),
        by_function,
        facets,
    }
}

/// Plain-text rendering of the report's headline tables.
pub fn render_dataflow_report(report: &DataflowReport, top: usize) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let s = &report.scheduling;
    let _ = writeln!(
        out,
        "blocks {}, code bytes {}; DUP/SWAP/POP bytes {} (DUP {}, SWAP {}, POP {})",
        report.blocks, report.code_bytes, s.total, s.dup, s.swap, s.pop
    );
    if let (Some(n), Some(total)) = (&report.scheduling_in_no_source, report.no_source_bytes) {
        let _ = writeln!(
            out,
            "no-source bytes {total}; DUP/SWAP/POP among them {} (DUP {}, SWAP {}, POP {})",
            n.total, n.dup, n.swap, n.pop
        );
    }
    let _ = writeln!(out, "\nscheduling bytes / function bytes, top {top}:");
    for f in report.by_function.iter().take(top) {
        let _ = writeln!(
            out,
            "{:>7} / {:>7}  {}",
            f.scheduling.total, f.bytes, f.name
        );
    }
    let _ = writeln!(
        out,
        "\nfacet, min block bytes: classes covered extra hidden"
    );
    for f in &report.facets {
        let _ = writeln!(
            out,
            "{:48} {:>3}: {:>5} {:>7} {:>7} {:>7}",
            f.facet, f.min_block_bytes, f.classes, f.covered, f.extra, f.hidden
        );
    }
    out
}

/// Blocks of two artifacts that share a facet address: content-addressed
/// matches across compilers. A match is structural correspondence under the
/// facet, a heuristic lead, never an equivalence proof.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CrossFacet {
    pub facet: String,
    pub min_block_bytes: u32,
    pub shared_addresses: usize,
    pub left_blocks: usize,
    pub left_bytes: u64,
    pub right_blocks: usize,
    pub right_bytes: u64,
    /// Largest shared addresses: (address, left copies, right copies, bytes
    /// per left copy, first left start, first right start).
    pub top: Vec<(String, usize, usize, u32, u32, u32)>,
}

fn block_index<'a>(
    d: &'a DataflowBlocks,
    facet: &str,
    min: u32,
) -> Result<std::collections::BTreeMap<String, Vec<&'a BlockRecord>>> {
    let mut m: std::collections::BTreeMap<String, Vec<&'a BlockRecord>> = Default::default();
    for b in &d.blocks {
        if b.bytes >= min && b.instructions >= 2 {
            let address = b.addresses.get(facet).with_context(|| {
                format!("block at pc {} has no address for facet `{facet}`", b.start)
            })?;
            m.entry(address.clone()).or_default().push(b);
        }
    }
    Ok(m)
}

/// Blocks of two artifacts that share an address, per facet both list with
/// the same definition. A facet listed on both sides with different
/// definitions, or a block missing a listed facet's address, is an error.
pub fn compare_blocks(
    left: &DataflowBlocks,
    right: &DataflowBlocks,
    min_block_bytes: &[u32],
    top: usize,
) -> Result<Vec<CrossFacet>> {
    let mut out = Vec::new();
    for (facet, definition) in &left.facets {
        let Some(other) = right.facets.get(facet) else {
            continue;
        };
        anyhow::ensure!(
            definition == other,
            "facet `{facet}` means `{definition}` on the left and `{other}` on the right"
        );
        for &min in min_block_bytes {
            let (l, r) = (
                block_index(left, facet, min).context("left blocks")?,
                block_index(right, facet, min).context("right blocks")?,
            );
            let mut shared: Vec<(String, usize, usize, u32, u32, u32)> = Vec::new();
            let (mut lb, mut rb, mut lc, mut rc) = (0u64, 0u64, 0usize, 0usize);
            for (addr, ls) in &l {
                if let Some(rs) = r.get(addr) {
                    lb += ls.iter().map(|b| u64::from(b.bytes)).sum::<u64>();
                    rb += rs.iter().map(|b| u64::from(b.bytes)).sum::<u64>();
                    lc += ls.len();
                    rc += rs.len();
                    shared.push((
                        addr.clone(),
                        ls.len(),
                        rs.len(),
                        ls[0].bytes,
                        ls[0].start,
                        rs[0].start,
                    ));
                }
            }
            let count = shared.len();
            shared.sort_by(|a, b| {
                (u64::from(b.3) * b.1 as u64)
                    .cmp(&(u64::from(a.3) * a.1 as u64))
                    .then_with(|| a.0.cmp(&b.0))
            });
            shared.truncate(top);
            out.push(CrossFacet {
                facet: facet.clone(),
                min_block_bytes: min,
                shared_addresses: count,
                left_blocks: lc,
                left_bytes: lb,
                right_blocks: rc,
                right_bytes: rb,
                top: shared,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::regions::FunctionRegion;

    #[test]
    fn scheduling_twins_share_dataflow_but_not_flat_addresses() {
        // Two blocks computing mstore(0x80, calldataload(4)), scheduled
        // differently, each ending in STOP; then an unrelated block.
        let code = [
            0x60, 4, 0x35, 0x60, 0x80, 0x52, 0x00, // push-first
            0x5b, 0x60, 0x80, 0x60, 4, 0x35, 0x90, 0x52, 0x00, // swap
            0x5b, 0x60, 1, 0x60, 0, 0x55, 0x00,
        ];
        let out = evm_dataflow_blocks(&code).unwrap();
        assert_eq!(out.blocks.len(), 3);
        let a = &out.blocks[0].addresses;
        let b = &out.blocks[1].addresses;
        let c = &out.blocks[2].addresses;
        assert_eq!(a["dataflow"], b["dataflow"]);
        assert_ne!(a["flat"], b["flat"]);
        assert_ne!(a["dataflow"], c["dataflow"]);
        assert_eq!(out.blocks[1].scheduling_bytes, 1);
    }

    #[test]
    fn memory_offsets_blind_matches_other_struct_layouts_only() {
        // mstore(add(x, 0x20), 7) vs mstore(add(x, 0x40), 7) vs ... 8.
        let block = |off: u8, v: u8| vec![0x5b, 0x60, v, 0x81, 0x60, off, 0x01, 0x52, 0x00];
        let mut code = Vec::new();
        code.extend(block(0x20, 7));
        code.extend(block(0x40, 7));
        code.extend(block(0x20, 8));
        let out = evm_dataflow_blocks(&code).unwrap();
        let addr = |i: usize, k: &str| out.blocks[i].addresses[k].clone();
        assert_ne!(addr(0, "dataflow"), addr(1, "dataflow"));
        assert_eq!(
            addr(0, "dataflow_memory_offsets_blind"),
            addr(1, "dataflow_memory_offsets_blind")
        );
        assert_ne!(
            addr(0, "dataflow_memory_offsets_blind"),
            addr(2, "dataflow_memory_offsets_blind")
        );
        assert_eq!(
            addr(0, "dataflow_constants_blind"),
            addr(2, "dataflow_constants_blind")
        );
    }

    #[test]
    fn input_order_blind_forgets_entry_slots_but_not_outputs() {
        let one = |code: &[u8], facet: &str| {
            let blocks = evm_dataflow_blocks(code).unwrap();
            assert_eq!(blocks.blocks.len(), 1);
            blocks.blocks[0].addresses[facet].clone()
        };
        let facet = "dataflow_input_order_blind";
        // mstore(0, x - y) with the entry items read in either order.
        let xy = [0x81, 0x81, 0x03, 0x60, 0, 0x52, 0x50, 0x50, 0x00];
        let yx = [0x80, 0x82, 0x03, 0x60, 0, 0x52, 0x50, 0x50, 0x00];
        assert_ne!(one(&xy, "dataflow"), one(&yx, "dataflow"));
        assert_eq!(one(&xy, facet), one(&yx, facet));
        // mstore(0, x - x) is not a reordering of mstore(0, x - y).
        let xx = [0x80, 0x80, 0x03, 0x60, 0, 0x52, 0x50, 0x00];
        assert_ne!(one(&xx, facet), one(&xy, facet));
        // Outputs keep their order: (0x20, 0x40) versus (0x40, 0x20).
        let a = [0x60, 0x20, 0x60, 0x40, 0x61, 0x01, 0x00, 0x56];
        let b = [0x60, 0x40, 0x60, 0x20, 0x61, 0x01, 0x00, 0x56];
        assert_ne!(one(&a, facet), one(&b, facet));
    }

    #[test]
    fn cross_artifact_blocks_match_by_facet_address() {
        let a = evm_dataflow_blocks(&[0x60, 4, 0x35, 0x60, 0x80, 0x52, 0x00]).unwrap();
        let b = evm_dataflow_blocks(&[0x60, 0x80, 0x60, 4, 0x35, 0x90, 0x52, 0x00]).unwrap();
        let cmp = compare_blocks(&a, &b, &[2], 3).unwrap();
        let at = |f: &str| cmp.iter().find(|c| c.facet == f).unwrap();
        assert_eq!(at("dataflow").shared_addresses, 1);
        assert_eq!(at("flat").shared_addresses, 0);
    }

    #[test]
    fn comparing_blocks_without_a_listed_facet_is_an_error() {
        let mut left = evm_dataflow_blocks(&[0x60, 1, 0x60, 2, 0x01, 0x60, 0, 0x52, 0x00]).unwrap();
        let right = left.clone();
        left.blocks[0].addresses.remove("flat");
        let result = std::panic::catch_unwind(|| compare_blocks(&left, &right, &[0], 10))
            .expect("a block without a listed facet made compare_blocks panic");
        assert!(format!("{result:?}").starts_with("Err"));
        let mut other = right.clone();
        other
            .facets
            .insert("flat".into(), "some other definition".into());
        let result = compare_blocks(&right, &other, &[0], 10);
        assert!(format!("{result:?}").starts_with("Err"));
    }

    #[test]
    fn report_counts_scheduling_and_hidden_repeats() {
        let code = [
            0x60, 4, 0x35, 0x60, 0x80, 0x52, 0x00, // push-first
            0x5b, 0x60, 0x80, 0x60, 4, 0x35, 0x90, 0x52, 0x00, // swap
            0x5b, 0x60, 1, 0x60, 0, 0x55, 0x00,
        ];
        let blocks = evm_dataflow_blocks(&code).unwrap();
        let region = |name: &str, start, end| FunctionRegion {
            name: name.into(),
            start,
            end,
        };
        let functions =
            FunctionRegions::new(vec![region("f", 0, 7), region("g", 7, code.len() as u32)]);
        let no_source = [(13u32, 14u32)];
        let report = dataflow_report(&blocks, &code, &functions, Some(&no_source), &[4], 5);
        assert_eq!(report.scheduling.total, 1);
        assert_eq!(report.scheduling.swap, 1);
        assert_eq!(report.scheduling_in_no_source.as_ref().unwrap().swap, 1);
        assert_eq!(report.no_source_bytes, Some(1));
        let at = |facet: &str| report.facets.iter().find(|f| f.facet == facet).unwrap();
        assert_eq!(at("flat").classes, 0);
        let df = at("dataflow");
        assert_eq!((df.classes, df.covered, df.extra, df.hidden), (1, 16, 9, 9));
        assert_eq!(df.top[0].functions.len(), 2);
    }
}
