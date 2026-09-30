# Test fixtures

- `fe-base/`: a synthetic Fe trace bundle (`trace.jsonl`, schema version 2),
  attribution details (`details.json`) and runtime (`runtime.bin`, raw bytes)
  for a contract `C` of three instructions: `PUSH1 0x80`, `MSTORE`, `STOP`.
  The trace carries the runtime's blake3 code hash. It is written in the
  shape of Fe's `fe dev trace emit` and `fe dev debug emit --attribution-details`
  outputs, not produced by Fe. Tests derive variants (other schema versions,
  no code hash, other artifacts) from it in code.
- `solc/`: `a.sol` and `b.sol`, and `out-true.json`, the standard-JSON output
  of solc 0.8.33 with viaIR and the optimizer (200 runs) for them, with ASTs,
  deployed bytecode, source maps and generated sources.
