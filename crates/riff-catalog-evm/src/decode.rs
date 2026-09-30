//! Linear-sweep decoding of EVM bytecode and the facts every analysis here
//! derives from it: PUSH immediates, code labels and memory opcodes.
//!
//! ## Label detection is a stated heuristic
//!
//! Bytecode does not mark which PUSH values are code addresses. A PUSH1..PUSH4
//! with its full immediate is treated as a code label when its value is the
//! pc of a decoded JUMPDEST ([`jump_labels`]). A plain constant that happens
//! to equal a JUMPDEST pc is then a label too; each analysis says what that
//! costs it.

/// JUMPDEST.
pub const JUMPDEST: u8 = 0x5b;

/// Memory opcodes: MLOAD, MSTORE, MSTORE8, MSIZE, MCOPY.
pub const MEMORY_OPCODES: [u8; 5] = [0x51, 0x52, 0x53, 0x59, 0x5e];

/// One decoded instruction: its pc, total byte length and opcode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Instruction {
    pub pc: u32,
    pub len: u32,
    pub opcode: u8,
}

/// Immediate bytes of a PUSH opcode (0 for PUSH0 and every other opcode).
pub const fn push_len(opcode: u8) -> usize {
    if opcode >= 0x60 && opcode <= 0x7f {
        (opcode - 0x5f) as usize
    } else {
        0
    }
}

/// Linear-sweep decode. A truncated trailing PUSH is clamped to the code end.
pub fn decode(code: &[u8]) -> Vec<Instruction> {
    let mut out = Vec::new();
    let mut pc = 0usize;
    while pc < code.len() {
        let opcode = code[pc];
        let len = (1 + push_len(opcode)).min(code.len() - pc);
        out.push(Instruction {
            pc: pc as u32,
            len: len as u32,
            opcode,
        });
        pc += len;
    }
    out
}

/// The value of a PUSH1..PUSH4 with its full immediate, else `None`. These
/// are the only PUSHes that can hold a code address.
pub fn push_value(code: &[u8], inst: &Instruction) -> Option<u32> {
    let n = push_len(inst.opcode);
    if !(1..=4).contains(&n) || inst.len as usize != 1 + n {
        return None;
    }
    Some(
        code[inst.pc as usize + 1..(inst.pc + inst.len) as usize]
            .iter()
            .fold(0u32, |a, b| (a << 8) | u32::from(*b)),
    )
}

/// pcs of every decoded JUMPDEST in `code`.
pub fn jumpdests(code: &[u8]) -> std::collections::HashSet<u32> {
    decode(code)
        .into_iter()
        .filter(|i| i.opcode == JUMPDEST)
        .map(|i| i.pc)
        .collect()
}

/// Each instruction's code-label target, by the label rule: a PUSH1..PUSH4
/// with its full immediate whose value is the pc of a decoded JUMPDEST in
/// `code`.
pub fn jump_labels(code: &[u8], insts: &[Instruction]) -> Vec<Option<u32>> {
    let targets = jumpdests(code);
    insts
        .iter()
        .map(|inst| push_value(code, inst).filter(|v| targets.contains(v)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_clamps_truncated_push() {
        let insts = decode(&[0x61, 0x01]);
        assert_eq!(
            insts,
            vec![Instruction {
                pc: 0,
                len: 2,
                opcode: 0x61
            }]
        );
        assert_eq!(push_value(&[0x61, 0x01], &insts[0]), None);
    }

    #[test]
    fn labels_are_full_short_pushes_of_jumpdest_pcs() {
        // No PUSH value here is the pc of the only JUMPDEST (6).
        let code = [0x60, 0x04, 0x61, 0x00, 0x05, 0x56, 0x5b, 0x60, 0x05];
        let insts = decode(&code);
        assert!(jump_labels(&code, &insts).iter().all(Option::is_none));
        let code = [0x60, 0x03, 0x56, 0x5b, 0x61, 0x00, 0x03];
        let insts = decode(&code);
        assert_eq!(
            jump_labels(&code, &insts),
            vec![Some(3), None, None, Some(3)]
        );
        assert_eq!(push_len(0x5f), 0);
        assert_eq!(push_len(0x7f), 32);
    }
}
