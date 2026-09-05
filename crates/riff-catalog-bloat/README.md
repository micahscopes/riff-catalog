# riff-catalog-bloat

`riffcat-bloat` is a small, compiler-independent ledger for code-growth
evidence. It seals a versioned JSON capture, validates its stage DAG and
stage-local function IDs, computes module, root-body, and reachable-union
instruction totals, and replays deterministic JSON or a short table.

Start with the [diagnosis quickstart](docs/diagnose.md) for existing MB2 captures,
growth inspection and the evidence needed for a correctness investigation.
Use the [artifact census](docs/artifact-census.md) to rank exact emitted function
sizes and repeated direct-copy sequences, save evidence and compare outputs.

The toolkit has no Sonatina dependency. It imports neutral structured compiler
events, and a compatibility adapter imports older Fe stderr traces as
explicitly weaker evidence.

See [the historical Fe pilot](docs/historical-pilot.md) for a replayed real-log
walkthrough and its evidence limits.

For compiler integration, see the [Fe design and MB2 handoff](docs/fe-instrumentation-design.md),
including proposed reusable Sonatina observation hooks and migration checks.
The [MB2 consumer agreement](docs/mb2-consumer-contract.md) records current
recorder ownership and the compatibility boundary for that integration.
The [live runbook](docs/live-pilot.md) and [verified results](docs/live-results.md)
cover the contained retained-helper versus inline experiment.

## Try the synthetic general capture

The general capture below is synthetic. Real typed-recorder compatibility
fixtures live in `tests/fixtures/mb2-observe`, with their own provenance README.

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

## Import structured Fe events

The instrumented Fe worktree used by this pilot can write a bounded JSONL request capture when
`FE_BLOAT_CAPTURE_DIR` is set. It writes nothing when the variable is absent.
Each request gets its own directory containing `events.jsonl` and exact emitted
artifacts. The final `capture_completed` record is required for a complete
capture. A missing marker remains explicitly incomplete, and records after a
completion or failure marker are rejected.

```sh
cargo run -p riff-catalog-bloat -- import-events \
  --events /workspace/scratch/fe-bloat/request-0000-main/events.jsonl \
  --output /workspace/scratch/fe-baseline.capture.json \
  --label scalar-helper-baseline \
  --source-id 'blake3:SOURCE_DIGEST' \
  --compiler-id 'fe:REVISION' \
  --producer-revision 'FE_REVISION' \
  --command 'cargo run -p fe-codegen --example bloat_capture_kernel -- ...' \
  --setting target=webgpu

cargo run -p riff-catalog-bloat -- replay \
  /workspace/scratch/fe-baseline.capture.json --json
```

The importer checks event ordering, one request ID, stage and predecessor
references, intervention name resolution, completion state, and the exact byte
length, producer SHA-256, and independent BLAKE3 of every linked artifact.
Artifact paths must remain inside the request directory. Graph totals are a
static direct-function closure over captured functions and all instructions in
their layout blocks. They are not path-feasible CFG instruction totals.

The importer bounds structured streams to 100,000 records and 64 MiB and linked
artifacts to 256 MiB. Import rejection does not imply compilation failed.
The original pilot producer failed compilation on recording errors; the MB2
typed recorder supports ordinary best-effort capture and explicit strict mode.
Budget exhaustion can leave valid shader artifacts but an incomplete capture.

Every rooted frontier records aggregate inliner call and instruction counts.
Structured caller/callee inline events and clone census rows are emitted only
for the full-inliner path because Sonatina's trivial remove, rewrite, and splice
paths do not expose clone-ID records. Zero detailed rows therefore does not
mean zero inlining. Use the aggregate `inliner_calls_*` measurements to decide
whether a frontier changed before interpreting detailed rows.

Schema `riff-catalog-bloat/2` adds explicit completion, intervention,
structured clone observations, and decisions. Existing version 1 capture files
remain readable and immutable. To add version 2 facts, import or seal into a
new filename. Do not overwrite or relabel a version 1 capture.

For a controlled Fe variant, `FE_BLOAT_FORCE_INLINE_HELPERS=mix_words` accepts
only an exact, unambiguous backend-callable helper that baseline policy would
retain. Unknown, ambiguous, rejected, empty, and duplicate selections fail.
Dependency-closure consequences are recorded separately, so comparison does
not claim that only the named helper changed.

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
matching, Naga/WGSL structural adapters, and profitability analysis remain
future work. This pilot's named-helper policy is a controlled intervention, not
a profitability claim. Its finite GPU oracle can test the selected fixture and
driver, but it is not a general behavior-equivalence proof.
