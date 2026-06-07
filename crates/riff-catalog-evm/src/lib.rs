//! riff-catalog-evm: level "evm/1". One `evm.instruction` node per PC;
//! opcode -> Structure, PUSH immediates -> Constants. The dimension split
//! makes "same code, different embedded constants" visible at a glance:
//! two `PUSH1 x` differ at the Constants facet, match at Structure.

use riff_catalog_core::{Dimension, EntityKey, Graph, GraphKey, NodeKey};
use thiserror::Error;

/// The versioned level string for this lowering (invariant I10).
pub const EVM_LEVEL: &str = "evm/1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BytecodeKind {
    Creation,
    Runtime,
}

impl BytecodeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Creation => "creation",
            Self::Runtime => "runtime",
        }
    }
}

#[derive(Debug, Error)]
pub enum EvmLowerError {
    #[error(transparent)]
    Core(#[from] riff_catalog_core::CatalogError),
}

#[derive(Clone, Debug)]
pub struct LoweredBytecode {
    pub graph_key: GraphKey,
    pub graph: Graph,
    pub instruction_count: usize,
}

pub fn lower_bytecode(
    owner: &str,
    which: BytecodeKind,
    bytecode: &[u8],
) -> Result<LoweredBytecode, EvmLowerError> {
    let root_key = EntityKey::new("evm.code", owner, which.as_str())?;
    let graph_key = GraphKey::new(root_key.clone(), format!("evm-{}", which.as_str()))?;
    let mut graph = Graph::new(graph_key.clone());
    let root = NodeKey::entity(root_key);
    graph.add_node(root.clone(), "evm.code")?;

    let mut pc = 0usize;
    let mut index = 0u32;
    while pc < bytecode.len() {
        let opcode = bytecode[pc];
        let node = NodeKey::entity(EntityKey::new(
            "evm.instruction",
            owner,
            format!("{}/pc:{pc}", which.as_str()),
        )?);
        graph.add_node(node.clone(), "evm.instruction")?;
        graph.add_field(
            &node,
            Dimension::Structure,
            "opcode",
            format!("0x{opcode:02x}"),
        )?;
        let immediate_len = push_immediate_len(opcode);
        if immediate_len > 0 {
            let end = (pc + 1 + immediate_len).min(bytecode.len());
            let mut immediate = String::with_capacity(2 + immediate_len * 2);
            immediate.push_str("0x");
            for byte in &bytecode[pc + 1..end] {
                immediate.push_str(&format!("{byte:02x}"));
            }
            graph.add_field(&node, Dimension::Constants, "immediate", immediate)?;
        }
        graph.add_child(&root, "instruction", index, &node)?;
        pc += 1 + immediate_len;
        index += 1;
    }

    Ok(LoweredBytecode {
        graph_key,
        graph,
        instruction_count: index as usize,
    })
}

fn push_immediate_len(opcode: u8) -> usize {
    if (0x60..=0x7f).contains(&opcode) {
        (opcode - 0x5f) as usize
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riff_catalog_core::{CyclePolicy, DigestRequest, HashPolicy, ViewMode, digest_graph};

    fn structure_and_constants(
        bytecode: &[u8],
    ) -> (riff_catalog_core::Digest, riff_catalog_core::Digest) {
        let lowered = lower_bytecode("evm:test", BytecodeKind::Runtime, bytecode).unwrap();
        let policy =
            HashPolicy::new(EVM_LEVEL, ViewMode::AnonymousShape, CyclePolicy::Reject).unwrap();
        let hashes = digest_graph(
            &DigestRequest::all_dimensions(lowered.graph_key, policy),
            &lowered.graph,
        )
        .unwrap()
        .hashes;
        (
            *hashes.graph.get(Dimension::Structure).unwrap(),
            *hashes.graph.get(Dimension::Constants).unwrap(),
        )
    }

    #[test]
    fn push_immediates_split_into_constants() {
        // PUSH1 0x01 vs PUSH1 0x02: same structure, different constants
        let one = structure_and_constants(&[0x60, 0x01]);
        let two = structure_and_constants(&[0x60, 0x02]);
        assert_eq!(one.0, two.0);
        assert_ne!(one.1, two.1);
        // PUSH1 vs PUSH2 (same payload prefix): different structure
        let push2 = structure_and_constants(&[0x61, 0x01, 0x00]);
        assert_ne!(one.0, push2.0);
    }

    #[test]
    fn truncated_push_is_tolerated() {
        // PUSH32 with only 2 bytes left — immediate clamps, no panic
        let lowered = lower_bytecode("evm:test", BytecodeKind::Runtime, &[0x7f, 0xaa, 0xbb]);
        assert_eq!(lowered.unwrap().instruction_count, 1);
    }
}
