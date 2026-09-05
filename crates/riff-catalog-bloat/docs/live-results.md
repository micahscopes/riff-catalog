# Live pilot results, 2026-09-05

The measurement loop works end to end on a small real Fe fixture. Explicitly
inlining its one scalar helper reduces emitted WGSL by 96 bytes. Both saved
shaders pass an independent finite-domain execution check. This is a contained
result, not a general recommendation to inline helpers.

## The experiment

Existing fixture: `crates/codegen/tests/fixtures/spirv/scalar_helper_call_render.fe`.
It computes `3*x+y` through `mix_words`. One compiler build and source identity,
fresh processes, baseline retention versus `FE_BLOAT_FORCE_INLINE_HELPERS=mix_words`.
The event stream records that the helper was backend-callable and normally
retained. Only that helper was explicitly removed from retention; no additional
dependency-closure removals were recorded.

| Measurement | Retained baseline | Force-inline variant |
| --- | ---: | ---: |
| Exact WGSL bytes | 518 | 422 |
| Exact SPIR-V bytes | 1,300 | 1,176 |
| Final module instructions | 5 | 6 |
| Final entry-body instructions | 2 | 3 |
| Final entry-reachable instructions | 5 | 3 |
| Fast-path calls spliced, first frontier | 0 | 1 |
| Full-inliner clone-record events | 0 | 0 |
| Independently checked pixels | 2,313 passed | 2,313 passed |

Capture off versus on produced byte-identical baseline WGSL. The preliminary
and final runs also emitted identical shader digests. The WGSL diff removes the
helper definition and call, substituting its arithmetic directly into `fs_main`.

## What we learned

1. **Scope can reverse the apparent answer.** Module and entry-body counts grow,
   but entry-reachable code and output bytes shrink. The unreferenced helper
   still exists in the module. Treating total IR size as shipped size would
   misclassify this result.
2. **Zero clone records did not mean no inlining.** The first run exposed that
   Sonatina's trivial inliner does not emit full-inliner clone records. We added
   its existing aggregate counters without changing the inlining path. The final
   capture reports one fast-path splice and explicitly limits detailed coverage.
3. **The hook boundary matters.** An additional existing MVT5 fixture emitted a
   complete, replayable 779-byte WGSL capture, but had no direct calls even at the
   first pre-merge snapshot. It does not show which earlier transformation
   removed them. Broader attribution needs earlier hooks, not stronger guesses.

The first finding is substantiation of a familiar compiler concern, not a claim
that compiler authors did not know it. The second directly improved this tool's
instrumentation. The third informs where MB2 instrumentation should expand next.

## Positive full-clone coverage on the MB2 kernel

The existing `production_sparse_round_interaction_fits_the_private_heap` test
also passed with capture enabled and no policy intervention. Both requests
completed: `paint` and `write_round_locals`. The latter supplied the live coverage
that the small fixtures could not:

- 59 stages, including 7 after-inline frontiers.
- 16 detailed full-inliner event rows, representing 24 callsites and 1,246 cloned
  instructions. These sums independently match the aggregate frontier counters.
- 15 fast-path splices, recorded separately.
- 546 cumulative clone-census rows. These are repeated observations, not another
  546 inlining operations or an additive instruction total.
- Exact backend WGSL: 91,681 bytes; exact SPIR-V: 66,056 bytes.

Both request captures imported and replayed with artifact verification. The
fixture's outer WebBundle test parsed/validated its own WGSL and checked its size
bound. It reported 91,675 WGSL bytes, six fewer than the observer's backend
artifact. Keep those boundaries separate; this observer does not capture the
later WebBundle string as an exact artifact. No GPU execution oracle was run for
this additional kernel, and no optimization savings are claimed from this
baseline-only coverage run. Do not compare its size with the older historical
trace as though they were a controlled before/after pair.

Evidence: `/workspace/scratch/riffcat-bloat-round-interaction-capture-2026-09-05`,
plus `/workspace/scratch/fe-bloat-round-interaction-capture-2026-09-05.log`.

## Behavior and compilation costs

The exact saved WGSL was parsed, validated, and executed on software Vulkan:
llvmpipe, Mesa 26.0.5, LLVM 21.1.8. Every pixel in a 257 by 9 domain matched the
independent Rust expression `(3*x+y).to_le_bytes()`. Both output images have
SHA-256 `f5c974e4b6ffc8fdaac2184d5f12179ce7e7cf0c145bbafed9fb4ae3df2c7f3a`.

A valid all-zero shader failed at pixel `(1,0)`, as intended. This checks the
oracle can reject wrong behavior rather than merely accepting valid WGSL.
There was no GPU skip. This is not physical-GPU performance evidence or a proof
over all inputs. SPIR-V bytes were measured, not independently executed.

Final one-shot wall observations:

| Scope | Capture off | Baseline capture | Variant capture |
| --- | ---: | ---: | ---: |
| Frontend package construction | 28.920 s | 30.876 s | 29.189 s |
| Shader lowering/backend/observer | 29.137 ms | 48.283 ms | 44.239 ms |

Other verification work was active on this VM. These are cost observations, not
a benchmark of policy speed or observer overhead. They do not include the
driver's later GPU shader compilation. The cold Rust examples build took
25m43s; that tool-build cost is separate from these per-kernel timings.

## Verified evidence

Authoritative local run directory:
`/workspace/scratch/riffcat-bloat-live-pilot-2026-09-05-verified`.

- `compile-summary.json`: mode-specific times and exact WGSL digests.
- `baseline-capture/` and `force-inline-mix-words-capture/`: raw producer events,
  exact WGSL and SPIR-V. Producer SHA-256 and independent BLAKE3 verified on import.
- `*.capture.json`, `*.report.json`, `comparison.json`: sealed observations and
  replayed/compared evidence. Repeated baseline JSON replay passed bytewise `cmp`.
- `gpu-oracle.json`: actual adapter and per-shader execution results.
- `negative-control.stderr`: expected numerical rejection, process exit 1.
- `invalid-helper.capture.json` and `.report.txt`: a real failed compile remains
  failed through import and plain-text replay, process exit 1 at compilation.
- `provenance.json`: exact source-tree, patch, binary, toolchain and dependency IDs.

The comparison reports matching source, compiler, and declared settings. It
also reports the intentional policy variable and capture-directory differences.
Its generic conclusion does not automatically certify single-policy causality;
the controlled harness and saved outputs supply additional experiment context.

The source tree is `8e5a712a8274ec8803964301ad54442c198e4654`, now committed as Fe
`908116b4b0714a36fb77bb53808d2762c2f6c4e8`. The binary was built before this local
commit from the same source tree; its digest is recorded separately. The earlier
compatibility-only commit is `b53449c77840dadedf8a676881639be5f401d5b8`.

| Verification gate | Result |
| --- | --- |
| Toolkit `cargo nextest run --workspace` | 165 passed, 1 intentionally skipped golden-file regeneration utility |
| Fe `--lib bloat_policy_tests` | 2 passed |
| Existing authored scalar-helper regression | 1 passed |
| Existing authored nested MVT5 regression, with capture | 1 passed; imported and artifact-verified |
| Existing MB2 round-interaction regression, with capture | 1 passed; both requests imported/replayed; full-clone sums agree with aggregate counters |
| Actual saved-shader oracle | Both passed; negative control rejected |
| Altered-artifact, truncation, stage-reference regressions | Passed in toolkit suite |

This is not a full Fe workspace test run. The scalar pilot exercised the fast
inliner, and the additional MB2 kernel exercised detailed full-inliner recording.
Rewritten-descendant tracking and earlier frontend attribution remain outside
this instrumentation's coverage.

## Try and extend

```sh
/workspace/scratch/target-riffcat-bloat/debug/riffcat-bloat replay \
  /workspace/scratch/riffcat-bloat-live-pilot-2026-09-05-verified/force-inline-mix-words.capture.json
```

See [the runbook](live-pilot.md) for a fresh experiment and the
[Fe/MB2 design handoff](fe-instrumentation-design.md) for proposed reusable
Sonatina callbacks. Next useful work is a controlled intervention on a high-growth,
backend-callable MB2 helper, with an appropriate behavior check. No
production retention policy was changed, and nothing was pushed.
