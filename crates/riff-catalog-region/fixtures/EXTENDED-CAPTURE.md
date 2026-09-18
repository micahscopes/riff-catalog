# Extended real-solc fixture provenance

`extended-regions.yul` was captured with optimizer disabled and experimental
`yulCFGJson` enabled. `extended-cfg.json` is exactly the CFG field from the saved
capture, reformatted as JSON, not a handwritten substitute.

- Compiler: 0.8.37-develop.2026.9.2+commit.2edb74d3.mod.Linux.g++.
- Binary SHA-256: b00ce952669bfddea399a44fa8c95e97dd3d00d26b19b02b8131879995511ec0.
- Source SHA-256: 7650da5033190135fdf6b09f38056d25a9bdab3c6ff194023c9396cf7cf7a048.
- CFG file SHA-256: 3598896eed938974ce1de0f5b06f2462894a9980810c60086fbb41ed31878dfb.
- Full input/output and provenance: `/workspace/scratch/riffcat-yul-demo-20260917/extended.bundle.json`.

Reproduce with `riffcat --solc <binary> yul-capture extended-regions.yul
--object ExtendedRegions --output <new-bundle.json>`. The CFG lives at `.cfg`.
The tests use this checked-in capture, so they require no compiler process.

Observed: whole nested function has 47 incidence vertices and exact comparison
survives block transport reordering. Selecting Block1, Block2 and Block6 in
`exits` gives a cyclic 30-vertex region with two distinct outgoing boundary
targets. Swapping their conditional role changes content. Reordered stores in
`effects` versus `effects_swapped` differ structurally, without claiming different
observable behavior. The three extended regression tests pass.

Following an environment reset, these tests were compiled with the installed
matching rustc through sccache against the already-built region and serde_json
libraries in `/workspace/sourcify-similarity-target`. This is focused evidence,
not a substitute for the still-pending full Cargo/nextest rebuild gate.
