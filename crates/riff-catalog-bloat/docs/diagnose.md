# Diagnose a captured kernel

This tool locates recorded growth and preserves evidence. It does not decide
whether a shader is correct. Start with one failing kernel and exact saved output.

For emitted-code bloat, start with the [artifact census](artifact-census.md).
It complements instruction counts with exact WGSL function and projection-run
coverage, including per-function counts for repeated patterns.

## Try the real MB2 checkpoint now

The existing binary and captures in this VM need no compiler rebuild:

```sh
export TMPDIR=/workspace/tmp
RIFFCAT=/workspace/scratch/target-riffcat-bloat/debug/riffcat-bloat
CAPTURE=/workspace/scratch/mb2-observe-compat-20260905/complete.capture.json
"$RIFFCAT" replay "$CAPTURE"
"$RIFFCAT" replay /workspace/scratch/mb2-observe-compat-20260905/partial.capture.json
```

Expect complete versus incomplete, with exact shader artifacts verified in both.
The partial capture is a real budget-exhausted compiler run, not a broken shader.
Do not treat process success from `replay` as a complete-capture or behavior gate.

For a fresh consumer build, from the riff-cat checkout:

```sh
export TMPDIR=/workspace/tmp SCCACHE_DIR=/workspace/.sccache
export RUSTC_WRAPPER=sccache CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR=/workspace/scratch/target-riffcat-bloat
cargo build -p riff-catalog-bloat -j 1
```

## Find the recorded growth boundary

This compact view lists stage measurements without the verbose table:

```sh
"$RIFFCAT" replay "$CAPTURE" --json | jq '
  {completion, caveats, stages: [.stages[] |
    {id, computed_scopes, measurements: [.recorded_measurements[] |
      select(.name == "instructions" or .name == "emitted_artifact_bytes"
        or (.name | startswith("inliner_")))]}]}'
```

Compare like scopes and units. Module instructions, entry-reachable instructions
and emitted bytes can move in different directions. Stage IDs identify recorded
boundaries, not automatic cross-run semantic alignment. Repeated pass records can
share a stage label; their counts are observations, not additive growth events.

Inspect policy decisions and individual full-clone events next:

```sh
"$RIFFCAT" replay "$CAPTURE" --json | jq '
  {decisions, inline_events: (.inline_events | sort_by(.cloned_instructions) | reverse),
   clone_observations, caveats}'
```

Zero detailed clone rows does not mean zero inlining. Check aggregate inliner
counts too. Never sum cumulative survival observations across stages. Rewritten
descendants remain unknown. These captures do not yet attribute final WGSL bytes
to individual Sonatina operations or distinguish every legalization cause.

## Bring a production capture

MB2 owns capture generation. Ask for the request directory containing
`events.jsonl`, exact artifacts, source digest, Fe revision plus dirty patch digest,
actual Sonatina revision/overlay, target settings and the exact producing command.
Use `import-events --help` for required flags and the
[structured import example](../README.md#import-an-existing-fe-trace) for context.
Set `--compiler-id` to include the actual overlay, not only the manifest pin.
Import each request separately to a new output filename. Keep the request directory:
sealed captures reference it. After relocation, reimport instead of editing hashes.

Compare baseline and one controlled variant:

```sh
"$RIFFCAT" compare /workspace/scratch/baseline.capture.json \
  /workspace/scratch/variant.capture.json --verify-artifacts
```

Those two filenames are examples for your own imports. Inspect reported source,
compiler and settings differences before interpreting size differences as savings.
Equal stage names or facets do not prove equivalent behavior.

## For a correctness failure

Keep the smallest failing input, exact shader bytes and digest, expected output,
actual output, execution backend/device and failure logs alongside the capture.
Replay verifies artifact integrity, not shader semantics. Execute the exact saved
shader through the relevant independent oracle, not a newly regenerated shader.
Record validation, execution and oracle results separately.

The [live runbook](live-pilot.md) has the scalar oracle commands and a wrong-result
negative control. That oracle checks only `3*x+y`; it is not a correctness oracle
for Mandelbrot, Quilting or arbitrary kernels. A production diagnosis needs its
own expected result. Report size changes separately until that behavior gate passes.

Useful collaborator packet: raw request, sealed capture, producing provenance,
verified report, failing input and behavior evidence. Do not replace missing
lineage with guesses or allocate WGSL bytes proportionally to instruction counts.
