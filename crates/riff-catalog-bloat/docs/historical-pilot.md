# Two runnable proofs

The first slice proves accounting and replay. It does not yet prove an
optimization saves code or runs faster.

## Start here in this VM

The implementation lives in an isolated worktree:

```sh
cd /workspace/riff-catalog-worktrees/bloat-toolkit
export TMPDIR=/workspace/tmp
export CARGO_TARGET_DIR=/workspace/scratch/target-riffcat-bloat
export RUSTC_WRAPPER=sccache
export SCCACHE_DIR=/workspace/.sccache
export CARGO_INCREMENTAL=0
```

The already-built executable is
`/workspace/scratch/target-riffcat-bloat/debug/riffcat-bloat`.
The `cargo run` commands below also build it when needed. No Fe build or GPU
is needed for these two proofs.

## Proof 1: shared helpers must not be counted twice

```sh
cargo run -p riff-catalog-bloat -- seal \
  --input crates/riff-catalog-bloat/tests/fixtures/general-shared-recursive.capture-body.json \
  --output /workspace/scratch/riffcat-general.capture.json
cargo run -p riff-catalog-bloat -- replay \
  /workspace/scratch/riffcat-general.capture.json
```

This checked-in fixture is synthetic. Two entry points call a shared helper,
which calls a recursive helper. A separate dead helper is never reached.

| What we count | Instructions |
| --- | ---: |
| All declared functions, including dead code | 137 |
| Entry A and its reachable helpers | 29 |
| Entry B and its reachable helpers | 31 |
| Union reachable from both entries | 36 |

The union is **36, not 60**: both entry closures include the same shared code.
Recursion terminates in the analysis because each function is counted once.
A second fixture stage has an unresolved indirect target. Its displayed count
is marked incomplete rather than presented as the full reachable footprint.

## Proof 2: replay a real Fe/Sonatina trace

The local historical input is:

`/workspace/scratch/mb2-round-interaction-inliner-census-2026-09-02.log`

It is 161886 bytes, with BLAKE3 digest
`79b357778f9e594fca8d062fd9157ed62c5f79689ca3f4cc964c57307e13c07a`.
The trace is not committed to this repository. On another host, substitute
your own capture using the [capture guide](../README.md#capture-fe-stderr).

```sh
cargo run -p riff-catalog-bloat -- import-fe \
  --trace /workspace/scratch/mb2-round-interaction-inliner-census-2026-09-02.log \
  --output /workspace/scratch/riffcat-historical.capture.json \
  --label historical-round-interaction \
  --source-id unknown \
  --compiler-id unknown
cargo run -p riff-catalog-bloat -- replay \
  /workspace/scratch/riffcat-historical.capture.json --json \
  > /workspace/scratch/riffcat-historical.report.json
```

Source/compiler revisions are unknown for this historical artifact. The
importer does not silently substitute the current checkout revisions.

The full log contains two compiler runs. The pilot recovered **129 stages and
852 helper observations**. These are cumulative census snapshots, not 852 new
clone events.

For the second run, independently checked against the original log:

| Observation | Count |
| --- | ---: |
| Module instructions before / after exact function merge | 7540 / 3771 |
| Root instructions at the last logged post-inline cleanup | 2870 |
| Helper-name groups at the final census | 25 |
| Final cumulative clone callsites | 74 |
| Final cumulative instructions originally cloned | 3507 |
| Original cloned instruction IDs still present | 2811 |

### Show the largest surviving helper groups

```sh
jq '.latest_helper_observations
    | map(select(.segment == 2))
    | sort_by(.surviving_original_ids) | reverse | .[:5]
    | .[] | {callee_name, surviving_original_ids, cloned_instructions_total,
              compiler_frontier, compiler_stage, line}' \
  /workspace/scratch/riffcat-historical.report.json
```

The largest group is `sparse_control_row_from_task`, with 742 surviving original
IDs from 856 cloned instructions over two callsites. This is an inspection
target, not a claim that 742 instructions can be removed.

### Prove deterministic replay

```sh
cargo run -p riff-catalog-bloat -- replay \
  /workspace/scratch/riffcat-historical.capture.json --json \
  | cmp /workspace/scratch/riffcat-historical.report.json -
```

Successful `cmp` is silent. Replay also rehashes the referenced trace and fails
if it has changed. Determinism is for the same captured data and tool/report
version, not a promise that future report versions serialize identically.

## What we learned

- Clone census rows repeat after passes. Summing them creates fictitious growth.
- The entry began as a three-instruction wrapper. Its expansion alone does not
  establish avoidable bloat.
- Original ID survival does not follow replacement instructions. The difference
  between 2870 root instructions and 2811 surviving IDs is not evidence of 59
  novel computations.
- Producer-reported WGSL size is 408158 bytes; the test reports 408152 bytes.
  The tool preserves both as separate logged observations. No exact emitted
  artifact was supplied in this pilot, so neither was verified against one.
- This log does not contain the call graph or stable clone caller identities.
  Those remain unavailable, even though the general capture format supports
  structured call graphs and inline events.

## Next useful slice

Emit structured observations from the current Fe/Sonatina compiler boundaries,
then measure one controlled helper-retention change. This will let us connect
an actual decision to measured output differences. Exact region matching and
behavior validation remain separate requirements, not implicit benefits of a
matching address.

For the schema, limitations, capture flags, and comparison workflow, see the
[usage guide](../README.md).
