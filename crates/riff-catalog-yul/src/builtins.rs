//! The EVM-dialect builtin table. A builtin call is *semantics* (an opcode),
//! so it lands in the Structure dimension; a user-function call is a name.

/// Sorted for binary search; keep alphabetical when adding.
const EVM_BUILTINS: &[&str] = &[
    "add",
    "addmod",
    "address",
    "and",
    "balance",
    "basefee",
    "blobbasefee",
    "blobhash",
    "blockhash",
    "byte",
    "call",
    "callcode",
    "calldatacopy",
    "calldataload",
    "calldatasize",
    "caller",
    "callvalue",
    "chainid",
    "codecopy",
    "codesize",
    "coinbase",
    "create",
    "create2",
    "datacopy",
    "dataoffset",
    "datasize",
    "delegatecall",
    "difficulty",
    "div",
    "eq",
    "exp",
    "extcodecopy",
    "extcodehash",
    "extcodesize",
    "gas",
    "gaslimit",
    "gasprice",
    "gt",
    "invalid",
    "iszero",
    "keccak256",
    "linkersymbol",
    "loadimmutable",
    "log0",
    "log1",
    "log2",
    "log3",
    "log4",
    "lt",
    "mcopy",
    "memoryguard",
    "mload",
    "mod",
    "msize",
    "mstore",
    "mstore8",
    "mul",
    "mulmod",
    "not",
    "number",
    "or",
    "origin",
    "pop",
    "prevrandao",
    "return",
    "returndatacopy",
    "returndatasize",
    "revert",
    "sar",
    "sdiv",
    "selfbalance",
    "selfdestruct",
    "setimmutable",
    "sgt",
    "shl",
    "shr",
    "signextend",
    "sload",
    "slt",
    "smod",
    "sstore",
    "stop",
    "sub",
    "timestamp",
    "tload",
    "tstore",
    "xor",
];

pub fn is_evm_builtin(name: &str) -> bool {
    name.starts_with("verbatim_") || EVM_BUILTINS.binary_search(&name).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted() {
        let mut sorted = EVM_BUILTINS.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, EVM_BUILTINS);
    }

    #[test]
    fn classification() {
        assert!(is_evm_builtin("add"));
        assert!(is_evm_builtin("sstore"));
        assert!(is_evm_builtin("datasize"));
        assert!(is_evm_builtin("verbatim_1i_1o"));
        assert!(!is_evm_builtin("abi_decode_t_uint256"));
        assert!(!is_evm_builtin("fun_transfer"));
    }
}
