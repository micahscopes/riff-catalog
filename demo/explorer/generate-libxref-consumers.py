#!/usr/bin/env python3
"""Generate consumer contracts that each vendor one library primitive.

Each (library, primitive) gets several genuinely-different wrapper contracts
(distinct names, distinct extra method) that all call the same library
function. The library chunk is byte-identical across a library's consumers
(=> one names-blind fingerprint class, the cross-reference) but differs across
libraries (=> the supply-chain contrast). Verbatim OZ/Solady/Solmate source.
"""
import os

LIBS = "/tmp/libs"
OUT = "/tmp/libxref/src"
os.makedirs(OUT, exist_ok=True)

# (tag, lib source file, primitive shape, call expression)
ENTRIES = [
    ("oz_muldiv",      "oz_Math.sol",                     "muldiv",          "Math.mulDiv(a, b, c)"),
    ("solady_muldiv",  "solady_FixedPointMathLib.sol",    "muldiv",          "FixedPointMathLib.fullMulDiv(a, b, c)"),
    ("solmate_muldiv", "solmate_FixedPointMathLib.sol",   "muldiv",          "FixedPointMathLib.mulDivDown(a, b, c)"),
    ("oz_sqrt",        "oz_Math.sol",                     "unary",           "Math.sqrt(a)"),
    ("solady_sqrt",    "solady_FixedPointMathLib.sol",    "unary",           "FixedPointMathLib.sqrt(a)"),
    ("solmate_sqrt",   "solmate_FixedPointMathLib.sol",   "unary",           "FixedPointMathLib.sqrt(a)"),
    ("solady_mulwad",  "solady_FixedPointMathLib.sol",    "binary",          "FixedPointMathLib.mulWad(a, b)"),
    ("solmate_mulwad", "solmate_FixedPointMathLib.sol",   "binary",          "FixedPointMathLib.mulWadDown(a, b)"),
    ("oz_ecdsa",       "oz_ECDSA_flat.sol",               "ecdsa",           "ECDSA.recover(h, sig)"),
    ("solady_ecdsa",   "solady_ECDSA.sol",                "ecdsa",           "ECDSA.recover(h, sig)"),
    ("oz_merkle",      "oz_MerkleProof.sol",              "merkle_memory",   "MerkleProof.verify(proof, root, leaf)"),
    ("solady_merkle",  "solady_MerkleProofLib.sol",       "merkle_memory",   "MerkleProofLib.verify(proof, root, leaf)"),
    ("solmate_merkle", "solmate_MerkleProofLib.sol",      "merkle_calldata", "MerkleProofLib.verify(proof, root, leaf)"),
]

# Different "applications" so each consumer is a genuinely distinct contract,
# not a copy — only the library chunk is shared between them.
APPS = ["Vault", "Airdrop", "Lottery"]

SHAPES = {
    "muldiv":          ("uint256 a, uint256 b, uint256 c",                 "uint256", "external pure"),
    "unary":           ("uint256 a",                                       "uint256", "external pure"),
    "binary":          ("uint256 a, uint256 b",                            "uint256", "external pure"),
    "ecdsa":           ("bytes32 h, bytes memory sig",                     "address", "external view"),
    "merkle_memory":   ("bytes32[] memory proof, bytes32 root, bytes32 leaf", "bool",  "external pure"),
    "merkle_calldata": ("bytes32[] calldata proof, bytes32 root, bytes32 leaf", "bool", "external pure"),
}

for tag, libfile, shape, call in ENTRIES:
    src = open(os.path.join(LIBS, libfile)).read()
    params, ret, mods = SHAPES[shape]
    for i, app in enumerate(APPS):
        name = f"{app}_{tag}_{i}"
        # a unique extra method so the surrounding contract genuinely differs
        extra = f"    function appId() external pure returns (uint256) {{ return {1000 + i * 13 + len(tag)}; }}\n"
        wrapper = (
            f"\ncontract {name} {{\n"
            f"    function run({params}) {mods} returns ({ret}) {{\n"
            f"        return {call};\n"
            f"    }}\n"
            f"{extra}"
            f"}}\n"
        )
        path = os.path.join(OUT, f"{name}.sol")
        open(path, "w").write(src + wrapper)

files = sorted(os.listdir(OUT))
print(f"generated {len(files)} consumers:")
for f in files:
    print("  ", f)
