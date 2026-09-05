# riff-catalog-bloat

`riffcat-bloat` is a small, compiler-independent ledger for code-growth
evidence. It seals a versioned JSON capture, validates its stage DAG and
stage-local function IDs, computes module, root-body, and reachable-union
instruction totals, and replays deterministic JSON or a short table.

This first slice does not change optimization behavior. It has no Sonatina
dependency. An adapter imports existing Fe stderr traces as explicitly weaker
compatibility evidence.

See [the historical Fe pilot](docs/historical-pilot.md) for a replayed real-log
walkthrough and its evidence limits.

## Try the synthetic general capture

All checked-in fixtures are synthetic illustrations. They are not compiler
measurements.

```sh
export TMPDIR=/workspace/tmp
export CARGO_TARGET_DIR=/workspace/scratch/target-riffcat-bloat
export RUSTC_WRAPPER=sccache
export SCCACHE_DIR=/workspace/.sccache
export CARGO_INCREMENTAL=0

cargo run -p riff-catalog-bloat -- seal \
  --input crates/riff-catalog-bloat/tests/fixtures/general-shared-recursive.capture-body.json \
  --output /workspace/scratch/riffcat-general.capture.json

cargo run -p riff-catalog-bloat -- report \
  /workspace/scratch/riffcat-general.capture.json

cargo run -p riff-catalog-bloat -- replay \
  /workspace/scratch/riffcat-general.capture.json --json
```

`seal` accepts a `Capture` body without a `capture_id`. It domain-separates and
hashes the exact serialized body, validates it, and writes the completed file
with create-new semantics. Repeating an identical write is allowed. A different
capture is never allowed to replace an existing file.

The synthetic graph has two entries sharing one helper, a recursive helper,
and dead code. Reachable-union accounting visits each function once, terminates
on the recursive edge, and excludes the dead helper. A second stage includes an
unknown indirect target, so its attribution is visibly incomplete.

## Import an existing Fe trace

The parser is intentionally bounded and tied to the exact current messages in
`crates/codegen/src/sonatina/spirv_lower.rs`. It requires an exact-function-merge
marker to segment compilations. Recognized but unsupported `fe spirv`, `fe naga`,
and `sonatina` lines are retained as unknown lines rather than discarded.

```sh
cargo run -p riff-catalog-bloat -- import-fe \
  --trace /workspace/scratch/fe-inline.stderr \
  --wgsl /workspace/scratch/kernel.wgsl \
  --output /workspace/scratch/fe-inline.capture.json \
  --label kernel-inline-capture \
  --source-id 'blake3:SOURCE_DIGEST' \
  --compiler-id 'fe:REVISION;sonatina:REVISION;naga:REVISION' \
  --producer-revision 'FE_REVISION' \
  --command 'cargo test --release -p fe-codegen --test TEST -- --nocapture' \
  --setting profile=release \
  --setting target=webgpu \
  --env FE_SPIRV_INLINE_TRACE=1 \
  --env FE_SPIRV_INLINE_TRACE_CLONES=1

cargo run -p riff-catalog-bloat -- replay \
  /workspace/scratch/fe-inline.capture.json
```

The trace and optional WGSL are referenced by canonical absolute path, exact
byte length, and a streamed BLAKE3 digest. `replay` verifies every referenced
artifact before reporting. `report --verify-artifacts` and
`compare --verify-artifacts` opt into the same check. Tampering fails closed.

The imported Fe clone rows are cumulative observations of the compiler's clone
record collection. The report shows the latest observation per segment and
helper for convenience, but retains every stage row in JSON. Never sum these
rows across frontiers or cleanup passes. `surviving_original_ids` literally
counts cloned instruction IDs still inserted in the caller. It does not count
instructions rewritten from them or any descendants.

The Fe trace carries selected root-body totals, not a reachable call graph. The
importer therefore marks call-graph attribution incomplete, leaves caller IDs
absent, and does not invent general inline events. A pre snapshot written by
the current Fe hook is after exact function merge and before helper graph
normalization.

If an actual WGSL file is supplied, its exact artifact byte count remains
separate from any producer or test-reported byte count in stderr. For example,
`sonatina spirv: emitted wgsl, bytes=...` and a later test harness byte count are
distinct compatibility measurements, not interchangeable artifact facts.

## Capture Fe stderr

The currently useful Fe environment is:

```sh
TMPDIR=/workspace/tmp \
CARGO_TARGET_DIR=/workspace/scratch/target-fe-bloat \
CARGO_BUILD_JOBS=1 \
RUSTC_WRAPPER=sccache \
SCCACHE_DIR=/workspace/.sccache \
CARGO_INCREMENTAL=0 \
FE_SPIRV_INLINE_TRACE=1 \
FE_SPIRV_INLINE_TRACE_CLONES=1 \
FE_SPIRV_INLINE_SNAPSHOT_DIR=/workspace/scratch/fe-bloat-sona \
FE_RUNTIME_IR_SNAPSHOT_DIR=/workspace/scratch/fe-bloat-rmir \
MB2_ALLOW_GPU_SKIP=1 \
cargo test --locked --release -p fe-codegen --features spirv-backend \
  --test TEST_NAME -- --nocapture \
  2> /workspace/scratch/fe-inline.stderr
```

`MB2_ALLOW_GPU_SKIP=1` means a host without a usable GPU may skip its runtime
oracle. A skipped oracle is not behavior-validation evidence. Record the knob
and the actual test outcome in capture provenance.

## Compare

```sh
cargo run -p riff-catalog-bloat -- compare \
  /workspace/scratch/capture-a.json \
  /workspace/scratch/capture-b.json \
  --json --verify-artifacts
```

Comparison lists declared source, compiler, settings, environment, and
measurement differences. The first implementation aligns measurements by
exact stage ID, name, and scope. Fe imports use source line IDs for observation
stages, so cross-version trace alignment is deliberately limited. Equal or
single-setting-different captures still do not establish that one compiler
policy caused an outcome.

## Claims and nonclaims

The toolkit can establish, subject to its typed evidence:

- exact artifact bytes and digests;
- compiler-reported or compatibility-trace measurements without conflating the
  evidence sources;
- direct-graph reachable unions when the producer declares the graph complete;
- stage-local inline event facts with caller, callee, callsites, total cloned
  instructions, and optional literal original-ID survival.

It does not establish that repeated code is removable, that inlining growth is
waste, that a missing surviving original ID has no rewritten descendant, or
that an optimization is correct or profitable. A small entry wrapper growing
after inlining is not by itself avoidable bloat.

WL/facet equality is candidate evidence only. Exact region matching still
needs a declared boundary, ordered operand and alias wiring, effects, and an
appropriate checker. Port/result-lane matching, effect matching, exact region
matching, behavior oracles, Naga/WGSL structural adapters, and profitable
compiler interventions remain future work.
