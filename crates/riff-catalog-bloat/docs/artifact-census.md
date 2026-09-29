# Artifact census

Find the largest emitted functions and repeated direct-copy sequences. Counts
come from exact saved bytes, not instruction-count estimates or predicted savings.
See [results and next gates](artifact-census-results.md) for the production pilot.

## Start here

From the main riff-cat checkout:

```sh
export TMPDIR=/workspace/tmp SCCACHE_DIR=/workspace/.sccache
export RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR=/workspace/scratch/target-riffcat-mainline
cargo build -p riff-catalog-bloat -j 1
RIFFCAT=$CARGO_TARGET_DIR/debug/riffcat-bloat
```

On any saved WGSL file:

```sh
"$RIFFCAT" census /path/to/shader.wgsl --top 5
```

For a verified compiler capture, preserving provenance and completion status:

```sh
"$RIFFCAT" census-capture \
  /workspace/scratch/mb2-observe-compat-20260905/partial.capture.json --top 5
```

The latter succeeds with an explicitly incomplete capture. It verifies all capture
artifacts, then checks that the bytes analyzed still match the selected artifact.
It does not promote partial capture into complete compiler attribution.

## Save, replay, compare

Use fresh output paths. Saved sidecars never overwrite different evidence:

```sh
"$RIFFCAT" census /path/to/before.wgsl \
  --output /workspace/scratch/before.census.json
"$RIFFCAT" census /path/to/after.wgsl \
  --output /workspace/scratch/after.census.json
"$RIFFCAT" census-replay /workspace/scratch/before.census.json
"$RIFFCAT" census-compare /workspace/scratch/before.census.json \
  /workspace/scratch/after.census.json --top 10
```

`--output` also works with `census-capture`. Add `--json` to any command for full
regions, per-function pattern counts, provenance and comparison rows. Replay
recomputes the census from its inputs; comparison replays both sides first.
Keep those inputs: sidecars reference canonical absolute paths. After relocation,
regenerate a sidecar instead of rewriting its sealed contents. The sidecar ID
commits the source references and report, and is not a signature of authorship.

Functions pair only by unique names under the same adapter. This is a visible
alignment hint, not cross-stage identity. Renames, ambiguous names and absent
functions stay unpaired. Pattern comparison uses policy, kind and digest. An
absent repeated group remains unknown, since it may now occur only once.

## What is measured

- Function spans begin at `fn` and end after the matching closing brace.
  Preceding attributes, declarations and whitespace remain outside these regions.
- Projection runs are consecutive simple assignments or untyped `let` statements
  with one common left root and right root. Member paths and constant indices are
  preserved; root names are erased and same-root versus distinct-root is retained.
  Calls, arithmetic, compound assignments, dynamic indices and literal RHS values
  do not match. Interleaved load temporaries generally break a run.
- Run spans include leading indentation and trailing whitespace/newline when the
  statement is alone on a line. Internal comments belong to the byte span but are
  omitted from the lexical key. Comments between statements break run grouping.
- Each pattern reports exact union coverage plus counts within containing functions.
  Different patterns/policies can overlap. Never sum them as removable bytes.
- `region_union_bytes + outside_region_bytes = artifact_bytes`. The outside
  number is a measurement boundary, not an estimate of unexplained bloat.

The WGSL adapter is lexical, not a validator or binding-aware alpha-equivalence
checker. Unsupported syntax can be rejected or remain outside pattern coverage.
It accepts ASCII identifiers and decimal constant indices (plain or `u`-suffixed).
Inputs are bounded to 64 MiB, two million tokens and 100,000 regions. Large valid
programs can exceed these limits and be rejected. Counts do not prove behavior,
aliasing safety, dead code, performance or safe removal.
Total selected-region bytes are capped at eight times artifact size and 256 MiB.
Sidecars must also fit 64 MiB before publication. Atomic publication requires
same-directory hard-link support; unsupported filesystems fail explicitly.

## Adapter interface for other artifacts

Use `census ARTIFACT --regions MANIFEST.json`. No compiler dependency is required.
The manifest schema is `riffcat-regions/1`:

```json
{
  "schema": "riffcat-regions/1",
  "artifact_blake3": "REPLACE_WITH_EXACT_ARTIFACT_BLAKE3",
  "adapter": "my-format-regions/1",
  "regions": [
    {"id":"part-0","kind":"section","name":"example","start":0,"end":16}
  ]
}
```

Offsets are half-open byte offsets, including for non-text artifacts. Digest,
bounds and IDs are checked; zero-length regions and duplicate ranges within one
kind are rejected. Overlap is allowed and measured by union. Adapter labels are
producer declarations, not verified semantics. Exact-byte grouping stays within
each kind and compares actual slices; projection grouping compares serialized
keys. Published hashes name those groups but do not determine group membership.
Region kinds must be nonempty; empty display names are allowed but not paired by
name during comparison. Manifest-driven patterns have explicit unassigned counts
rather than inferred function containment. WGSL projection regions also expose
their statement counts, including runs that occur only once.

These addresses are not riff-cat graph facet addresses. A textual group does not
supply the graph-policy witnesses needed for a semantic claim. Current capture
schemas require whole-artifact byte measurements, so subregions stay in the
sidecar until a future explicitly versioned scope extension is warranted.

## EVM bytecode runs

A manifest may declare `"evm_runs": {"min_run_bytes": 32}`. The artifact is
then read as EVM bytecode, and repeated instruction runs are reported inside
each `function` region (runs never cross a function region) as `evm_run`
regions and pattern groups (`riff_catalog_evm::runs`). Each copy is lowered
into a core graph at level `evm-run/1` (`run_graph`), and a class is a core
facet address of that graph: Structure plus Constants for the exact key
(policy name `evm-run/1 facet structure+constants`). Two runs match when:

- opcodes and non-label PUSH immediates are equal, in order;
- a jump label that targets code inside the run targets the same offset from
  the run start, so relocated copies still match;
- labels that target code outside the run become ports numbered by first use.
  The per-copy targets are bindings, and `evm_ports` counts ports and how many
  of them differ between copies.

With `"constants_as_ports": true` (CLI `--constants-as-ports`), the facet is
Structure only (`evm-run/1 facet structure`): PUSH values are forgotten, but
which constants are equal to each other is Structure, so they behave like
ports numbered by first use.
That groups copies that differ only in constants (the same error path with
another selector): code a parameter could share, not identical code.
`evm_ports.constant_ports` and `varying_constant_ports` count them.
`"memory_offsets_as_ports": true` (CLI `--memory-offsets-as-ports`) keeps
every constant except those `riff_catalog_evm::dataflow` classes as memory or
calldata offsets (a constant whose every use in its basic block is an MLOAD,
MSTORE, MSTORE8, CALLDATALOAD or CALLDATACOPY address, directly or through
ADDs). Those are lowered as the Constants field class `memory_offset` and
erased by the `riffcat-view/1` plan `evm-run.memory-offsets-blind/1`, so the
key groups the same code for different struct layouts (policy name
`evm-run/1 view memory-offsets-blind facet structure+constants`). It replaces
an earlier cap on the number of differing constants, which worked around long
runs of fixed-offset memory moves matching each other with every constant
different. Runs shorter than
`min_run_instructions` (default 2) are not reported: a single PUSH32 is not
a shape.

DUP, SWAP and POP are instructions, so equal runs also have equal internal
dataflow wiring, given the same stack at entry. A label is a PUSH1..PUSH4 whose
value is a JUMPDEST pc. That is a heuristic; a constant misread as a label can
only split a class (inside) or show up as a port binding (outside).

Candidates come from maximal repeats (suffix array and LCP intervals) and a
fast token key; each candidate group is then partitioned by the core facet
address, which decides the classes (a test checks the two partitions agree). The selection is greedy by covered bytes on
still-unclaimed bytes, so selected groups never overlap each other and their
`covered_bytes` add up. It is not an optimal cover. A partially claimed
occurrence is dropped, not trimmed. A group is structural correspondence, not
proof that the copies behave alike or that sharing them is safe or smaller.

## Fe EVM contracts from the compiler trace

`fe-trace-bytes` builds the manifest above from Fe's own outputs and reports
where every runtime byte goes:

```sh
fe dev trace emit INGOT -O 1 --out trace.jsonl
fe dev debug emit --format ethdebug --from trace.jsonl --out ethdebug.json \
  --attribution-details attribution.json
"$RIFFCAT" fe-trace-bytes --trace trace.jsonl --attribution attribution.json \
  --contract NAME --artifact NAME.runtime.bin --census-dir OUT --json-out report.json
"$RIFFCAT" census OUT/runtime.bin --regions OUT/regions.json   # replayable run census
```

The trace must describe the given artifact exactly: its length, the trace's
code hash and every PUSH immediate are checked first. Attribution is Fe's
`PrimarySourceV1` decision from the details file, not re-derived here. Tables:

- by Fe classification, confidence and reason; by emitted function (final code
  layout, from `bytecode.pc -> evm.vcode.inst`); by primary source body and
  file; by recv arm, with and without the functions only one arm reaches.
  Each of these adds up to the artifact length. Bytes after the code that the
  trace's code hash covers but no instruction describes (constant data) are a
  row of their own.
- by source body anywhere among a byte's origins. These overlap by design.

Emitted functions by position: a function's region runs from its first to
its last linked byte. The unlinked gap directly before it joins it when the
gap starts with a JUMPDEST and its other JUMPDESTs are reached only from the
gap or the function (the entry JUMPDEST and argument set-up carry no vcode
link). Other gaps stay "between functions". On Seaport this agrees, byte for
byte, with the compiler's own pc map for 192 of 196 functions of a
near-identical build.

Recv arms: an arm left as its own function owns that function. An arm inlined
into the dispatcher owns the post-optimization blocks dominated by its region
entry, the nearest common dominator of the blocks whose instructions come only
from that arm. The assignment is dropped, with a note, if the region would
contain a block that comes only from another arm. Shared blocks stay in the
"no single recv arm" row. Blocks are labeled with the recv-arm bodies found
among their instructions' origins (Fe's attribution). Functions each arm
reaches come from calls inferred from label pushes whose value is a
function's first byte, walked with the capture model's `reachable_union`;
computed jumps are not seen, so the graph is declared incomplete.

Runs are joined back per occurrence: emitted function, recv arm, the source
body with the most primary-attributed bytes, and how many instructions have the
same primary source in every copy (the same Fe source emitted more than once).

## Formal boundary and regression checks

```sh
cargo test -p riff-catalog-bloat -j 1
cd lean
lake build
lake exe census_bridge ../crates/riff-catalog-bloat/tests/fixtures/census-bridge.json
```

`Riffcat/ArtifactCensus.lean` proves finite-mask accounting, scalar-copy renaming
under injectivity and state correspondence, and composition of those renamings.
Checked counterexamples show that alias collapse can change results even from
initially agreeing states, and that exact byte cost cannot factor through the
name-erasing text pattern. Rust and Lean check shared coverage/text-cost vectors.
Rust also exhaustively compares two-interval union with a bitmap for lengths 0..8.
It checks 2,000 deterministic randomized many-interval cases as well.

These are model proofs and executable bridge checks, not a proof of the Rust
sort-and-sweep implementation or of WGSL lowering. They justify narrow accounting
and interpretation rules, not compiler transformations. Before changing codegen,
run the relevant independent behavior oracle on the exact saved artifacts.

## EVM facets, stage tracing and excess ledgers

These commands work on any EVM runtime with a `riffcat-regions/1` manifest
(from `fe-trace-bytes --census-dir` for Fe, `solc-functions` for solc).

- `evm-dataflow` lifts basic blocks into `evm-dataflow/1` graphs
  (`riff_catalog_evm::dataflow`) and addresses each block at seven facets:
  flat exact, flat constants blind, flat memory-offsets blind, dataflow
  exact, dataflow constants blind, and the `riffcat-view/1` plans that erase
  the field classes `constants.memory_offset` and `structure.slot`. The
  report counts DUP/SWAP/POP bytes and whole-block classes per facet.
  `evm-dataflow-compare` lists blocks two artifacts share at each facet.
- `fe-trace-stages` reads the trace's origin graph by stage
  (`riff_catalog_ingest_trace::stages`) and follows named selections of
  emitted bytes back to HIR: nodes per stage, edge phases, where provenance
  ends, post-opt operations, MIR forms and instances, and a mechanism per
  instruction. Chains from HIR constructs to their bytes are content
  addressed at level `fe-stage-chain/1`.
- `sonatina-functions` groups a Sonatina module's functions by facet
  (exact, types blind, types and constants blind).
- `evm-byte-causes` puts every byte in one bucket: named causes in the
  order given, then role buckets by opcode. `evm-byte-causes-compare`
  subtracts two ledgers and checks that the differences add up to the size
  difference. Role buckets describe what leftover bytes do; they are not
  causes, and the apportioned estimate it prints is an estimate.

A shared address is structural correspondence at a facet, never a proof
that two pieces of code behave alike.
