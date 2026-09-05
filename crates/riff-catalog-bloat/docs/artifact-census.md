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
