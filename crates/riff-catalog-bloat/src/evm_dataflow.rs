//! Per-block facet addresses of EVM bytecode, flat (`evm-run/1`) and lifted
//! (`evm-dataflow/1`), for censuses that compare them.
//!
//! Every address is a core facet address. The facets that erase one field
//! class (memory offsets, stack port order) are `riffcat-view/1` plans over
//! `evm-dataflow/1`, so their policy level names the plan.

use anyhow::{Context, Result};
use riff_catalog_core::{Dimension, Graph};
use riff_catalog_evm::dataflow::{EVM_DATAFLOW_LEVEL, address, dataflow_policy, lift_code};
use riff_catalog_evm::runs::{
    RunKey, RunKeyer, decode, run_address, run_facet, run_graph, with_labels,
};
use riff_catalog_view::ViewPlan;
use serde::{Deserialize, Serialize};

pub const EVM_DATAFLOW_BLOCKS_SCHEMA: &str = "riffcat-evm-dataflow-blocks/1";

/// Forgets which constants are memory or calldata offsets, keeps the rest.
pub const MEMORY_OFFSETS_BLIND_VIEW: &str = r#"
language "riffcat-view/1"
view "evm-dataflow.memory-offsets-blind/1"
input "evm-dataflow/1"
root node-kind "evm.block"
traverse children
retain structure, constants
erase constants.memory_offset
"#;

/// Forgets the stack positions of a block's inputs and outputs.
pub const PORT_ORDER_BLIND_VIEW: &str = r#"
language "riffcat-view/1"
view "evm-dataflow.port-order-blind/1"
input "evm-dataflow/1"
root node-kind "evm.block"
traverse children
retain structure, constants
erase structure.slot
"#;

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
    let ports_blind = ViewPlan::parse(PORT_ORDER_BLIND_VIEW).context("ports view")?;
    let both_text = MEMORY_OFFSETS_BLIND_VIEW
        .replace(
            "memory-offsets-blind/1",
            "memory-offsets-and-port-order-blind/1",
        )
        .replace(
            "erase constants.memory_offset",
            "erase constants.memory_offset, structure.slot",
        );
    let both_blind = ViewPlan::parse(&both_text).context("combined view")?;
    let policy = dataflow_policy(EVM_DATAFLOW_LEVEL)?;
    let sc = [Dimension::Structure, Dimension::Constants];
    let s = [Dimension::Structure];
    let insts = decode(code);
    let labeled = with_labels(code, &insts);
    let by_pc: std::collections::HashMap<u32, usize> =
        insts.iter().enumerate().map(|(i, x)| (x.pc, i)).collect();
    let (exact_run, blind_run) = (run_facet(false), run_facet(true));
    let offsets_run = RunKeyer::new(code, RunKey::MemoryOffsetsBlind);

    let mut blocks = Vec::new();
    for block in lift_code(code)? {
        let first = by_pc[&block.start];
        let run = &labeled[first..first + block.instructions as usize];
        let flat = run_graph(code, run);
        let mut addresses = std::collections::BTreeMap::new();
        addresses.insert("flat".into(), run_address(&flat, &exact_run).to_hex());
        addresses.insert(
            "flat_constants_blind".into(),
            run_address(&flat, &blind_run).to_hex(),
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
            "dataflow_port_order_blind".into(),
            view_address(&ports_blind, &block.graph, &sc)?,
        );
        addresses.insert(
            "dataflow_memory_offsets_and_port_order_blind".into(),
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
        ("dataflow", "evm-dataflow/1, Structure + Constants"),
        ("dataflow_constants_blind", "evm-dataflow/1, Structure"),
        (
            "dataflow_memory_offsets_blind",
            "view evm-dataflow.memory-offsets-blind/1, Structure + Constants",
        ),
        (
            "dataflow_port_order_blind",
            "view evm-dataflow.port-order-blind/1, Structure + Constants",
        ),
        (
            "dataflow_memory_offsets_and_port_order_blind",
            "view evm-dataflow.memory-offsets-and-port-order-blind/1, Structure + Constants",
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
