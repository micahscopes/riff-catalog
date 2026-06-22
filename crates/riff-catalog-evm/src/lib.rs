//! riff-catalog-evm: level "evm/1". One `evm.instruction` node per PC;
//! opcode -> Structure, PUSH immediates -> Constants. The dimension split
//! makes "same code, different embedded constants" visible at a glance:
//! two `PUSH1 x` differ at the Constants facet, match at Structure.
//!
//! The Solidity CBOR metadata trailer (`<code> <cbor> <2-byte len>`) is split
//! off into a single `evm.metadata` node with its bytes in Constants — it is
//! provenance (a hash of the source metadata), not code, so lowering it as
//! instructions would pollute the fingerprint. The payoff is that this maps
//! Sourcify's match levels onto the facet dial directly: the **Structure**
//! facet forgets the metadata value (a Sourcify *partial match* = two contracts
//! that are Structure-twins here), while a Constants-bearing facet keeps it
//! (an *exact/full match*). The dial they already run in production, generalized.

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

    // Split off the CBOR metadata trailer; only the code stream is lowered as
    // instructions (the trailer becomes one provenance node below).
    let (code_end, metadata) = split_metadata(bytecode);

    let mut pc = 0usize;
    let mut index = 0u32;
    while pc < code_end {
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
            let end = (pc + 1 + immediate_len).min(code_end);
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

    // The metadata trailer as one provenance node: its KIND is Structure (so
    // the code shape is unaffected), its bytes are Constants (so the Structure
    // facet forgets them — partial match — and a Constants facet keeps them).
    if let Some(trailer) = metadata {
        let node = NodeKey::entity(EntityKey::new(
            "evm.metadata",
            owner,
            format!("{}/metadata", which.as_str()),
        )?);
        graph.add_node(node.clone(), "evm.metadata")?;
        let mut hex = String::with_capacity(2 + trailer.len() * 2);
        hex.push_str("0x");
        for byte in trailer {
            hex.push_str(&format!("{byte:02x}"));
        }
        graph.add_field(&node, Dimension::Constants, "auxdata", hex)?;
        graph.add_child(&root, "metadata", index, &node)?;
    }

    Ok(LoweredBytecode {
        graph_key,
        graph,
        instruction_count: index as usize,
    })
}

/// Split off the Solidity CBOR metadata trailer: bytecode is laid out as
/// `<code> <cbor blob> <2-byte big-endian length of the cbor blob>`. Returns
/// the code length and the trailer bytes (cbor + length suffix) when a
/// plausible trailer is present, else the whole length and `None`.
fn split_metadata(bytecode: &[u8]) -> (usize, Option<&[u8]>) {
    let n = bytecode.len();
    if n < 3 {
        return (n, None);
    }
    let cbor_len = ((bytecode[n - 2] as usize) << 8) | bytecode[n - 1] as usize;
    let trailer_len = cbor_len + 2;
    if cbor_len == 0 || trailer_len > n {
        return (n, None);
    }
    let cbor_start = n - trailer_len;
    // solc emits a CBOR map of 1–3 entries here (ipfs/bzzr0 hash, solc version,
    // experimental flag): the first byte is a map header 0xa1..=0xa3.
    if !matches!(bytecode[cbor_start], 0xa1..=0xa3) {
        return (n, None);
    }
    (cbor_start, Some(&bytecode[cbor_start..n]))
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
    fn metadata_trailer_is_a_partial_match_facet() {
        // Same code (PUSH1 1, PUSH1 2, ADD), two different CBOR metadata
        // trailers: <cbor blob 0xa1..> <2-byte BE length>.
        let code = [0x60u8, 0x01, 0x60, 0x02, 0x01];
        let with = |aux: &[u8]| {
            let mut bc = code.to_vec();
            bc.extend_from_slice(aux);
            bc.extend_from_slice(&[0x00, aux.len() as u8]); // BE length of the blob
            structure_and_constants(&bc)
        };
        let a = with(&[0xa1, 0x01, 0xAA]);
        let b = with(&[0xa1, 0x01, 0xBB]);
        // Sourcify "partial match": same Structure, different metadata Constants.
        assert_eq!(a.0, b.0, "metadata value forgotten at the Structure facet");
        assert_ne!(a.1, b.1, "metadata distinguished at the Constants facet");
        // And the metadata bytes are NOT mis-parsed as code: the Structure here
        // equals the bare-code Structure plus one metadata node — different code
        // still differs.
        let other = with(&[0xa1, 0x01, 0xAA]); // same as `a`
        assert_eq!(a.0, other.0);
        let diff_code = structure_and_constants(&[0x60, 0x01, 0x01]); // PUSH1 1 ADD
        assert_ne!(a.0, diff_code.0);
    }

    #[test]
    fn truncated_push_is_tolerated() {
        // PUSH32 with only 2 bytes left — immediate clamps, no panic
        let lowered = lower_bytecode("evm:test", BytecodeKind::Runtime, &[0x7f, 0xaa, 0xbb]);
        assert_eq!(lowered.unwrap().instruction_count, 1);
    }
}
