//! Level "evm-dataflow/1": EVM bytecode split into basic blocks, each block
//! lifted by symbolic stack execution into a dataflow graph. DUP, SWAP and
//! POP do not become nodes; they only decide which value feeds which operand.
//!
//! ## One block graph
//!
//! - `evm.block` root. Structure: `consumed` (entry stack items the block
//!   removes), `produced` (items it leaves in their place) and
//!   `falls_through`. Children: `effect` (ordinal = program order) for every
//!   effectful operation, and `out` (all ordinal 0) for every produced item.
//! - `evm.out` node per produced item. Structure `slot` (0 = top at exit);
//!   child `value`.
//! - `evm.input` node per entry item that is read. Structure `slot` (0 = top
//!   at entry).
//! - `evm.op` node per operation. Structure `opcode`, and `effect_index` for
//!   effectful operations; children `arg` (ordinal 0 = the operand popped
//!   first).
//! - `evm.const` node per distinct PUSH value. Structure: `constant_port`,
//!   numbered by first use, so the facets that forget values still keep
//!   which constants are equal. Constants: `memory_offset`
//!   when every use of the value is a memory or calldata address (directly,
//!   or through ADDs; see [`MEMORY_ADDRESS_OPERANDS`]), else `value`. The
//!   bytes have leading zeros removed, so `PUSH1 0x20` and `PUSH2 0x0020`
//!   are the same constant.
//! - `evm.label` node per distinct code label (a PUSH1..PUSH4 whose value is a
//!   JUMPDEST pc, the same heuristic as [`crate::runs`]). Structure `port`,
//!   numbered by first use; the target itself is not a field.
//!
//! Pure operations and constants are hash-consed inside a block: the same
//! pure operation on the same operands is one node, whether the code
//! recomputed it or kept it with DUP. Effectful operations are never merged,
//! and their order is kept. Values that nothing uses are not in the graph.
//!
//! Anonymous-shape digests of these graphs compare the computation of two
//! blocks, not their bytes. Core's Merkle fold identifies a child by its
//! content, so two distinct nodes with equal content would be
//! interchangeable: a consumer of one would hash like a consumer of the
//! other. The lowering therefore gives every node a distinct Structure
//! identity (inputs by slot, labels and constants by first-use port, effects
//! by index, and pure operations are hash-consed), which makes the digest
//! determine the DAG, sharing included, at every facet that keeps Structure.
//! A match is structural correspondence, not an equivalence proof.

use std::collections::{BTreeMap, HashMap, HashSet};

use riff_catalog_core::{
    CatalogError, CyclePolicy, Digest, DigestRequest, Dimension, EntityKey, Facet, Graph, GraphKey,
    HashPolicy, NodeKey, ViewMode, digest_graph,
};

use crate::runs::{Instruction, decode};

/// The versioned level string for this lowering (invariant I10).
pub const EVM_DATAFLOW_LEVEL: &str = "evm-dataflow/1";

/// Operands that are memory or calldata addresses: (opcode, operand index,
/// 0 = popped first). MLOAD, MSTORE, MSTORE8 and CALLDATALOAD read or write
/// at their first operand; CALLDATACOPY's first two operands are the memory
/// destination and the calldata source.
pub const MEMORY_ADDRESS_OPERANDS: &[(u8, usize)] = &[
    (0x51, 0),
    (0x52, 0),
    (0x53, 0),
    (0x35, 0),
    (0x37, 0),
    (0x37, 1),
];

const JUMPDEST: u8 = 0x5b;
const ADD: u8 = 0x01;

/// Stack effect and kind of one opcode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpKind {
    /// No side effect and no state read that another operation could change.
    Pure,
    /// Kept in order as an `effect` child.
    Effect,
    /// Ends the block (and is an effect).
    Terminator,
}

fn op_info(opcode: u8) -> (usize, usize, OpKind) {
    use OpKind::*;
    match opcode {
        0x00 => (0, 0, Terminator),
        0x01..=0x07 | 0x0a | 0x0b => (2, 1, Pure),
        0x08 | 0x09 => (3, 1, Pure),
        0x10..=0x14 | 0x16..=0x18 | 0x1a..=0x1d => (2, 1, Pure),
        0x15 | 0x19 => (1, 1, Pure),
        0x20 => (2, 1, Effect),
        0x30 | 0x32 | 0x33 | 0x34 | 0x36 | 0x38 | 0x3a => (0, 1, Pure),
        0x35 => (1, 1, Pure),
        0x31 | 0x3b | 0x3f => (1, 1, Effect),
        0x37 | 0x39 | 0x3e => (3, 0, Effect),
        0x3c => (4, 0, Effect),
        0x3d => (0, 1, Effect),
        0x40 | 0x49 => (1, 1, Pure),
        0x41..=0x46 | 0x48 | 0x4a => (0, 1, Pure),
        0x47 => (0, 1, Effect),
        0x51 | 0x54 | 0x5c => (1, 1, Effect),
        0x52 | 0x53 | 0x55 | 0x5d => (2, 0, Effect),
        0x56 => (1, 0, Terminator),
        0x57 => (2, 0, Terminator),
        0x58 | 0x59 | 0x5a => (0, 1, Effect),
        0x5e => (3, 0, Effect),
        0xa0..=0xa4 => (2 + (opcode - 0xa0) as usize, 0, Effect),
        0xf0 => (3, 1, Effect),
        0xf1 | 0xf2 => (7, 1, Effect),
        0xf3 | 0xfd => (2, 0, Terminator),
        0xf4 | 0xfa => (6, 1, Effect),
        0xf5 => (4, 1, Effect),
        0xff => (1, 0, Terminator),
        // INVALID and undefined opcodes halt.
        _ => (0, 0, Terminator),
    }
}

fn push_len(opcode: u8) -> usize {
    if (0x5f..=0x7f).contains(&opcode) {
        (opcode - 0x5f) as usize
    } else {
        0
    }
}

fn is_scheduling(opcode: u8) -> bool {
    opcode == 0x50 || (0x80..=0x9f).contains(&opcode)
}

/// A value in the symbolic execution.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Val {
    Input(usize),
    Const(Vec<u8>),
    Label(u32),
    Op(usize),
}

#[derive(Clone, Debug)]
struct Op {
    opcode: u8,
    args: Vec<Val>,
    kind: OpKind,
}

/// Bytes of one block, by role.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BlockBytes {
    pub total: u32,
    /// DUP, SWAP and POP.
    pub scheduling: u32,
    pub dup: u32,
    pub swap: u32,
    pub pop: u32,
    pub push: u32,
    pub jumpdest: u32,
}

/// One lifted basic block.
#[derive(Clone, Debug)]
pub struct Block {
    pub start: u32,
    pub end: u32,
    pub instructions: u32,
    pub bytes: BlockBytes,
    pub consumed: usize,
    pub produced: usize,
    /// Distinct constants classed as memory offsets, and all distinct constants.
    pub memory_offset_constants: usize,
    pub constants: usize,
    /// pcs of PUSH instructions whose value is classed as a memory offset.
    pub memory_offset_pushes: Vec<u32>,
    pub graph: Graph,
}

/// Split `code` (up to `code_end`) into basic blocks. A block starts at pc 0,
/// at every JUMPDEST and after every terminator (JUMPI included).
pub fn basic_blocks(code: &[u8]) -> Vec<Vec<Instruction>> {
    let mut blocks: Vec<Vec<Instruction>> = Vec::new();
    let mut current: Vec<Instruction> = Vec::new();
    for inst in decode(code) {
        if inst.opcode == JUMPDEST && !current.is_empty() {
            blocks.push(std::mem::take(&mut current));
        }
        current.push(inst);
        let (_, _, kind) = op_info(inst.opcode);
        let is_push_or_stack = push_len(inst.opcode) > 0
            || inst.opcode == 0x5f
            || is_scheduling(inst.opcode)
            || inst.opcode == JUMPDEST;
        if kind == OpKind::Terminator && !is_push_or_stack {
            blocks.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    blocks
}

/// Lift every block of `code`. `jumpdests` should come from the same code.
pub fn lift_code(code: &[u8]) -> Result<Vec<Block>, CatalogError> {
    let jumpdests: HashSet<u32> = decode(code)
        .into_iter()
        .filter(|i| i.opcode == JUMPDEST)
        .map(|i| i.pc)
        .collect();
    basic_blocks(code)
        .iter()
        .map(|insts| lift_block(code, insts, &jumpdests))
        .collect()
}

/// Lift one block by symbolic stack execution.
pub fn lift_block(
    code: &[u8],
    insts: &[Instruction],
    jumpdests: &HashSet<u32>,
) -> Result<Block, CatalogError> {
    let mut stack: Vec<Val> = Vec::new();
    let mut inputs = 0usize;
    let mut ops: Vec<Op> = Vec::new();
    let mut pure_ids: HashMap<(u8, Vec<Val>), usize> = HashMap::new();
    let mut effects: Vec<usize> = Vec::new();
    let mut bytes = BlockBytes::default();
    let mut const_pushes: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut falls_through = true;

    // Make the stack at least `n` deep by adding entry items at the bottom.
    fn ensure(stack: &mut Vec<Val>, inputs: &mut usize, n: usize) {
        while stack.len() < n {
            stack.insert(0, Val::Input(*inputs));
            *inputs += 1;
        }
    }

    for inst in insts {
        let opcode = inst.opcode;
        let raw = &code[inst.pc as usize..(inst.pc + inst.len) as usize];
        bytes.total += inst.len;
        match opcode {
            JUMPDEST => bytes.jumpdest += inst.len,
            0x5f..=0x7f => {
                bytes.push += inst.len;
                let imm = &raw[1..];
                let n = push_len(opcode);
                let value = (1..=4)
                    .contains(&n)
                    .then(|| imm.iter().fold(0u64, |a, b| (a << 8) | u64::from(*b)))
                    .filter(|v| imm.len() == n && *v <= u64::from(u32::MAX))
                    .map(|v| v as u32)
                    .filter(|v| jumpdests.contains(v));
                if let Some(target) = value {
                    stack.push(Val::Label(target));
                } else {
                    let first = imm.iter().position(|b| *b != 0).unwrap_or(imm.len());
                    let trimmed = imm[first..].to_vec();
                    const_pushes.push((inst.pc, trimmed.clone()));
                    stack.push(Val::Const(trimmed));
                }
            }
            0x80..=0x8f => {
                bytes.dup += inst.len;
                let n = (opcode - 0x7f) as usize;
                ensure(&mut stack, &mut inputs, n);
                let v = stack[stack.len() - n].clone();
                stack.push(v);
            }
            0x90..=0x9f => {
                bytes.swap += inst.len;
                let n = (opcode - 0x8f) as usize;
                ensure(&mut stack, &mut inputs, n + 1);
                let top = stack.len() - 1;
                stack.swap(top, top - n);
            }
            0x50 => {
                bytes.pop += inst.len;
                ensure(&mut stack, &mut inputs, 1);
                stack.pop();
            }
            _ => {
                let (pops, pushes, kind) = op_info(opcode);
                ensure(&mut stack, &mut inputs, pops);
                let mut args = Vec::with_capacity(pops);
                for _ in 0..pops {
                    args.push(stack.pop().expect("ensured"));
                }
                let id = if kind == OpKind::Pure {
                    let next = ops.len();
                    let id = *pure_ids.entry((opcode, args.clone())).or_insert(next);
                    if id == next {
                        ops.push(Op { opcode, args, kind });
                    }
                    id
                } else {
                    ops.push(Op { opcode, args, kind });
                    effects.push(ops.len() - 1);
                    ops.len() - 1
                };
                if pushes == 1 {
                    stack.push(Val::Op(id));
                }
                if kind == OpKind::Terminator {
                    falls_through = matches!(opcode, 0x57);
                }
            }
        }
    }
    bytes.scheduling = bytes.dup + bytes.swap + bytes.pop;

    // Entry items left in place at the bottom are not consumed.
    let mut consumed = inputs;
    let mut kept = 0usize;
    while kept < stack.len() && consumed > 0 && stack[kept] == Val::Input(consumed - 1) {
        kept += 1;
        consumed -= 1;
    }
    let outputs: Vec<Val> = stack[kept..].iter().rev().cloned().collect();

    // Live values: reachable from effects and outputs.
    let mut live = vec![false; ops.len()];
    let mut work: Vec<usize> = effects.clone();
    work.extend(outputs.iter().filter_map(|v| match v {
        Val::Op(id) => Some(*id),
        _ => None,
    }));
    while let Some(id) = work.pop() {
        if std::mem::replace(&mut live[id], true) {
            continue;
        }
        for a in &ops[id].args {
            if let Val::Op(child) = a {
                work.push(*child);
            }
        }
    }

    // Memory-offset classification: a constant is an address part when it is
    // an address operand, or an operand of an ADD that is one (recursively).
    let mut address_ops = vec![false; ops.len()];
    let mut address_uses: HashMap<Vec<u8>, (usize, usize)> = HashMap::new(); // (address, all)
    // Walk ops in reverse creation order so an ADD is marked before its operands.
    for id in (0..ops.len()).rev() {
        if !live[id] {
            continue;
        }
        let op = &ops[id];
        for (k, a) in op.args.iter().enumerate() {
            let is_address = MEMORY_ADDRESS_OPERANDS.contains(&(op.opcode, k))
                || (op.opcode == ADD && address_ops[id]);
            match a {
                Val::Op(child) if is_address && ops[*child].opcode == ADD => {
                    address_ops[*child] = true;
                }
                Val::Const(c) => {
                    let e = address_uses.entry(c.clone()).or_insert((0, 0));
                    e.1 += 1;
                    if is_address {
                        e.0 += 1;
                    }
                }
                _ => {}
            }
        }
    }
    for v in &outputs {
        if let Val::Const(c) = v {
            address_uses.entry(c.clone()).or_insert((0, 0)).1 += 1;
        }
    }
    let is_memory_offset = |c: &[u8]| {
        address_uses
            .get(c)
            .is_some_and(|(address, all)| *address > 0 && address == all)
    };

    // Build the core graph.
    let owner = EntityKey::new("evm.block", "block", insts[0].pc.to_string())?;
    let mut graph = Graph::new(GraphKey::new(owner.clone(), "block")?);
    let root = NodeKey::entity(owner.clone());
    graph.add_node(root.clone(), "evm.block")?;
    graph.add_field(&root, Dimension::Structure, "consumed", consumed as u64)?;
    graph.add_field(
        &root,
        Dimension::Structure,
        "produced",
        outputs.len() as u64,
    )?;
    graph.add_field(&root, Dimension::Structure, "falls_through", falls_through)?;

    let effect_index: HashMap<usize, usize> =
        effects.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let mut keys: HashMap<Val, NodeKey> = HashMap::new();
    let mut label_ports: BTreeMap<u32, u64> = BTreeMap::new();
    let mut constants = 0usize;
    let mut memory_offset_constants = 0usize;
    let mut next_key = 0usize;
    // Iterative post-order materialization of a value and its operands.
    let mut materialize = |graph: &mut Graph, v: &Val| -> Result<NodeKey, CatalogError> {
        let mut frames: Vec<(Val, bool)> = vec![(v.clone(), false)];
        while let Some((v, expanded)) = frames.pop() {
            if keys.contains_key(&v) {
                continue;
            }
            if let (Val::Op(id), false) = (&v, expanded) {
                frames.push((v.clone(), true));
                for a in ops[*id].args.iter().rev() {
                    if !keys.contains_key(a) {
                        frames.push((a.clone(), false));
                    }
                }
                continue;
            }
            let key = NodeKey::derived(owner.clone(), format!("v{next_key}"))?;
            next_key += 1;
            match &v {
                Val::Input(slot) => {
                    graph.add_node(key.clone(), "evm.input")?;
                    graph.add_field(&key, Dimension::Structure, "slot", *slot as u64)?;
                }
                Val::Const(c) => {
                    graph.add_node(key.clone(), "evm.const")?;
                    graph.add_field(
                        &key,
                        Dimension::Structure,
                        "constant_port",
                        constants as u64,
                    )?;
                    constants += 1;
                    let class = if is_memory_offset(c) {
                        memory_offset_constants += 1;
                        "memory_offset"
                    } else {
                        "value"
                    };
                    graph.add_field(&key, Dimension::Constants, class, c.clone())?;
                }
                Val::Label(target) => {
                    graph.add_node(key.clone(), "evm.label")?;
                    let next = label_ports.len() as u64;
                    let port = *label_ports.entry(*target).or_insert(next);
                    graph.add_field(&key, Dimension::Structure, "port", port)?;
                }
                Val::Op(id) => {
                    let op = &ops[*id];
                    graph.add_node(key.clone(), "evm.op")?;
                    graph.add_field(&key, Dimension::Structure, "opcode", u64::from(op.opcode))?;
                    if let Some(index) = effect_index.get(id) {
                        graph.add_field(
                            &key,
                            Dimension::Structure,
                            "effect_index",
                            *index as u64,
                        )?;
                    }
                    debug_assert!(op.kind != OpKind::Pure || live[*id]);
                    for (k, a) in op.args.iter().enumerate() {
                        let child = keys[a].clone();
                        graph.add_child(&key, "arg", k as u32, &child)?;
                    }
                }
            }
            keys.insert(v, key);
        }
        Ok(keys[v].clone())
    };
    for (ordinal, id) in effects.iter().enumerate() {
        let key = materialize(&mut graph, &Val::Op(*id))?;
        graph.add_child(&root, "effect", ordinal as u32, &key)?;
    }
    for (slot, v) in outputs.iter().enumerate() {
        let value = materialize(&mut graph, v)?;
        let out = NodeKey::derived(owner.clone(), format!("out{slot}"))?;
        graph.add_node(out.clone(), "evm.out")?;
        graph.add_field(&out, Dimension::Structure, "slot", slot as u64)?;
        graph.add_child(&out, "value", 0, &value)?;
        graph.add_child(&root, "out", 0, &out)?;
    }

    let memory_offset_pushes = const_pushes
        .iter()
        .filter(|(_, c)| is_memory_offset(c))
        .map(|(pc, _)| *pc)
        .collect();
    let last = insts.last().expect("blocks are non-empty");
    Ok(Block {
        start: insts[0].pc,
        end: last.pc + last.len,
        instructions: insts.len() as u32,
        bytes,
        consumed,
        produced: outputs.len(),
        memory_offset_constants,
        constants,
        memory_offset_pushes,
        graph,
    })
}

/// The hash policy for `evm-dataflow/1` graphs (or a view level over them).
pub fn dataflow_policy(level: &str) -> Result<HashPolicy, CatalogError> {
    HashPolicy::new(level, ViewMode::AnonymousShape, CyclePolicy::Reject)
}

/// Facet address of `graph` under `policy` at `dimensions`.
pub fn address(
    graph: &Graph,
    policy: &HashPolicy,
    dimensions: &[Dimension],
) -> Result<Digest, CatalogError> {
    let facet = Facet::new(policy.policy_id(), dimensions.iter().copied())?;
    let request = DigestRequest::new(
        graph.graph_key.clone(),
        policy.clone(),
        facet.dimensions.clone(),
    )?;
    Ok(digest_graph(&request, graph)?
        .hashes
        .facet_address(&facet)?
        .address_digest())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exact(code: &[u8]) -> Digest {
        let blocks = lift_code(code).unwrap();
        assert_eq!(blocks.len(), 1, "{blocks:?}");
        let policy = dataflow_policy(EVM_DATAFLOW_LEVEL).unwrap();
        address(
            &blocks[0].graph,
            &policy,
            &[Dimension::Structure, Dimension::Constants],
        )
        .unwrap()
    }

    const PUSH1: u8 = 0x60;
    const DUP1: u8 = 0x80;
    const DUP2: u8 = 0x81;
    const SWAP1: u8 = 0x90;
    const POP: u8 = 0x50;
    const MLOAD: u8 = 0x51;
    const MSTORE: u8 = 0x52;
    const CALLDATALOAD: u8 = 0x35;
    const SUB: u8 = 0x03;
    const STOP: u8 = 0x00;

    #[test]
    fn stack_scheduling_is_not_structure() {
        // mstore(0x80, calldataload(4)) written two ways: recompute-free with a
        // swap, and with a dup and pop.
        let a = [PUSH1, 4, CALLDATALOAD, PUSH1, 0x80, MSTORE, STOP];
        let b = [PUSH1, 0x80, PUSH1, 4, CALLDATALOAD, SWAP1, MSTORE, STOP];
        let c = [PUSH1, 4, CALLDATALOAD, DUP1, POP, PUSH1, 0x80, MSTORE, STOP];
        assert_eq!(exact(&a), exact(&b));
        assert_eq!(exact(&a), exact(&c));
        // A pure value computed twice is the value kept with DUP.
        let d = [
            PUSH1,
            4,
            CALLDATALOAD,
            PUSH1,
            4,
            CALLDATALOAD,
            SUB,
            PUSH1,
            0,
            MSTORE,
            STOP,
        ];
        let e = [PUSH1, 4, CALLDATALOAD, DUP1, SUB, PUSH1, 0, MSTORE, STOP];
        assert_eq!(exact(&d), exact(&e));
    }

    #[test]
    fn operand_order_and_constants_matter() {
        let a = [PUSH1, 1, PUSH1, 2, SUB, PUSH1, 0, MSTORE, STOP];
        let b = [PUSH1, 2, PUSH1, 1, SUB, PUSH1, 0, MSTORE, STOP];
        let c = [PUSH1, 1, PUSH1, 3, SUB, PUSH1, 0, MSTORE, STOP];
        assert_ne!(exact(&a), exact(&b));
        assert_ne!(exact(&a), exact(&c));
    }

    #[test]
    fn effects_keep_their_order() {
        // mload(0) before mstore(0, 1) vs after it.
        let a = [PUSH1, 0, MLOAD, PUSH1, 1, PUSH1, 0, MSTORE, STOP];
        let b = [PUSH1, 1, PUSH1, 0, MSTORE, PUSH1, 0, MLOAD, STOP];
        assert_ne!(exact(&a), exact(&b));
    }

    #[test]
    fn stack_ports_and_passthrough() {
        // Entry [x, y]: swap them and add; y below is untouched.
        let blocks = lift_code(&[DUP2, DUP2, 0x01, SWAP1, POP]).unwrap();
        let block = &blocks[0];
        assert_eq!(block.bytes.scheduling, 4);
        // Stack at exit: [y, x+y]; y is passthrough, x consumed.
        assert_eq!(block.consumed, 1);
        assert_eq!(block.produced, 1);
    }

    #[test]
    fn memory_offsets_are_their_own_constant_class() {
        // mstore(add(input, 0x20), 7): 0x20 is an offset, 7 a value.
        let blocks = lift_code(&[PUSH1, 7, DUP2, PUSH1, 0x20, 0x01, MSTORE, STOP]).unwrap();
        let block = &blocks[0];
        assert_eq!(block.memory_offset_constants, 1);
        assert_eq!(block.constants, 2);
        assert_eq!(block.memory_offset_pushes, vec![3]);
    }

    #[test]
    fn blocks_split_at_jumpdest_and_terminators() {
        let code = [PUSH1, 5, 0x56, 0x5b, 0x5b, STOP, 0x5b, PUSH1, 1];
        let blocks = basic_blocks(&code);
        let starts: Vec<u32> = blocks.iter().map(|b| b[0].pc).collect();
        assert_eq!(starts, vec![0, 3, 4, 6]);
        // The PUSH1 5 is a label (pc 5 is not a JUMPDEST here, so a constant).
        let lifted = lift_code(&code).unwrap();
        assert!(
            !lifted[0]
                .graph
                .nodes
                .values()
                .any(|n| n.kind.as_str() == "evm.label")
        );
    }
}
