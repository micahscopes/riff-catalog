# Fe code-growth instrumentation: design and MB2 integration handoff

Status: local implementation, live compiler validation still pending when this
note was written (2026-09-05). This is not an upstream feature or a claim of
measured optimization savings. See [live-pilot.md](live-pilot.md) for commands.

## Purpose and boundary

Answer: which compiler boundary grew the code, which helper decisions contributed,
and did a controlled change improve the exact emitted output without breaking
the tested behavior?

The current patch is in Fe. It uses existing Sonatina APIs and adds no Sonatina
changes or riff-cat dependency to either compiler. A standalone toolkit consumes
versioned observations after compilation.

```text
Fe request and helper policy
  -> Sonatina transforms and existing analysis/clone records
  -> Fe observer writes events.jsonl and exact WGSL/SPIR-V
  -> riffcat-bloat imports, checks, seals, compares
  -> separate runtime oracle executes the saved WGSL
```

Two independently enabled features must remain distinguishable:

- `FE_BLOAT_CAPTURE_DIR`: observations and exact artifacts.
- `FE_BLOAT_FORCE_INLINE_HELPERS`: experimental policy intervention, not logging.

Absent both variables, the intended behavior is the existing compiler policy
without capture files. Observational neutrality still requires testing; it must
not be inferred from the name "observer".

## Current Fe patch

Paths below are relative to `/workspace/fe-worktrees/bloat-toolkit`.

| File | Responsibility |
| --- | --- |
| `crates/codegen/src/sonatina/bloat_capture.rs` | Request-local writer, graph/metric extraction, event and artifact encoding |
| `crates/codegen/src/sonatina/spirv_lower.rs` | Request lifecycle, transform boundaries, existing analysis/clone records, named policy intervention |
| `crates/codegen/src/sonatina/mod.rs` | Private observer module declaration |
| `crates/codegen/examples/bloat_capture_kernel.rs` | Same-source, fresh-process baseline and intervention harness |
| `crates/codegen/examples/bloat_gpu_oracle.rs` | Independent finite-domain checker for the pilot's exact WGSL |

There is also a separate two-line `InstanceIndex` compatibility repair in
`crates/codegen/src/web_bundle.rs`, needed by the clean Fe base against its
existing dependency pin. It is not instrumentation or a new optimization.

The observer wraps `compile_webgpu_request` and
`inline_spirv_calls_from_roots`. The Render pilot enters through
`compile_runtime_package_spirv_render`. Do not assume every older SPIR-V entry
API takes this route.

### Observation points

Full function/call-graph snapshots: before exact private-function merge, after
merge, after helper normalization, and final shader IR before backend emission.

Between those snapshots: root/module metrics after each inline frontier and
individual cleanup pass; new inline events; separate cumulative clone census.
Metric-only stages explicitly omit the graph rather than pretending it is empty
and complete. Final return-lane transforms are currently covered by the final
snapshot, not individually attributed structured events.

The existing backend helper analysis supplies callable variants, instruction
counts, resource access, physical parameter counts, and rejection reasons. Fe
then records baseline retention, selected retention, explicitly forced inlining,
and additional removals required to close helper dependencies.

This does not yet capture every Fe MIR or Naga internal pass, intermediate Naga
modules, WGSL AST structure, exact-merge alias maps, or rewritten instruction
descendants. Do not describe it as complete compiler-wide attribution.

### Event contract

Every JSONL record has schema `fe-bloat-event/1`, a request ID, and a contiguous
sequence number. Events are `capture_started`, `stage`, `exact_function_merge`,
`helper_analysis`, `helper_selection`, `inline_event`, `clone_census`, `artifacts`,
and a terminal `capture_completed` or `capture_failed`.

Each request gets a new directory. Artifact records identify the exact WGSL and
little-endian SPIR-V bytes, length, and SHA-256. The importer checks the producer
digest and length, independently computes BLAKE3, and validates stage references.
The checked-in synthetic wire fixture is
`crates/riff-catalog-bloat/tests/fixtures/structured-request/events.jsonl` in the
toolkit repository. It is an executable example of the contract, not a live run.

A missing terminal marker means incomplete. Failed captures are not successful
measurements. Request IDs cannot change midstream, and terminal records cannot
be followed by more events. The writer caps streams at 100,000 records / 64 MiB
and artifacts at 256 MiB each. Capture I/O errors fail an opted-in compile;
they are not silently swallowed. These output caps do not bound the in-memory
cost of collecting clone IDs or constructing graph snapshots.

### Meaning of IDs, counts, and hashes

- Function IDs are stage-local references, not structural content addresses.
  References to the normalized stage identify source functions for clone records;
  they do not assert that every later body is unchanged.
- Static reachable unions follow direct function calls and count each function
  once. Counts scan all layout blocks, not path-feasible execution. Unknown
  indirect calls make attribution incomplete.
- Inline events describe new compiler clone records at a frontier. Clone census
  rows repeatedly inspect accumulated records. Never sum census across stages.
- Original-ID survival means those exact cloned IDs remain inserted in the
  caller. It says nothing about rewritten descendants.
- Capture hashes commit to the serialized evidence, including provenance and
  paths. They are not semantic equivalence proofs, stable region identities,
  or permission to eliminate duplicated-looking code.

## Suggestions for reusable Sonatina instrumentation

This section is a proposal for the MB2 instrumentation owners, not implemented
Sonatina work or authorization to change their dependency pins.

### Keep responsibilities separate

| Keep in Fe | Candidate Sonatina responsibility | Keep outside both |
| --- | --- | --- |
| Source/request identity and entry selection | Transform boundary callbacks | Capture sealing, replay, comparison |
| Frontend helper-retention policy and experiments | Facts from the inliner, merge and return-lane transforms | Cross-run alignment and candidate matching |
| Capture directories, environment configuration, file ownership | Backend helper eligibility and specialization facts | Profitability interpretation |
| Source-to-IR provenance when available | Read-only IR snapshot/metric access | Independent behavior oracles |

Do not move `FE_BLOAT_FORCE_INLINE_HELPERS` into a generic observer API. A
measurement sink must not acquire the ability to select optimizations.

### Suggested incremental migration

1. Preserve the current wire fixture and importer checks as a consumer contract.
   First move how facts are obtained, not their meaning or the storage format.
2. Add an optional caller-owned observer at the Sonatina pass-driver boundary.
   Prefer borrowed read-only IR views and typed context containing request-local
   invocation/pass IDs. File I/O, environment reads, JSON, and riff-cat should
   remain outside this interface. Decide whether callbacks may fail the run;
   preserve explicit incomplete/error state either way.
3. Add typed events at the transforms that already know the facts. Start with
   rooted inlining: distinguish new clone events from later survival observations.
   Then expose exact-function-merge aliases and forwarded/dead return-lane changes
   if the transform actually provides the mappings. Missing mappings stay absent.
4. Let Fe adapt these callbacks into the current request writer. Add capability
   negotiation for counts, call-graph snapshots, and full clone-ID tracking so a
   size-only capture does not pay the cost of detailed attribution.
5. After equivalent captures and outputs are demonstrated, remove duplicated Fe
   transform traversal. Keep frontend policy observations in Fe. Only then extend
   the same sink to other frontends or backend specialization boundaries.

API sketch, illustrative rather than an agreed signature:

```rust,ignore
trait ObservationSink {
    fn before_pass(&mut self, context: &PassContext, ir: &Module);
    fn after_pass(&mut self, context: &PassContext, ir: &Module);
    fn transform_event(&mut self, context: &PassContext, event: TransformEvent<'_>);
}
```

An IR borrow is valid only during its callback. Sinks must copy selected facts,
not hold references into mutating IR. Parallel execution needs explicit parent
and invocation IDs; deterministic presentation must not depend on which worker
writes first. Do not require a global mutable observer or global event ordering
across unrelated requests.

### Migration risks and acceptance checks

The current Fe patch takes the existing trace-style path of invoking cleanup
passes separately when capture is enabled. A reusable pass-driver callback would
avoid duplicating that scheduling path. Test output identity with capture off/on,
including multi-entry, recursive/shared-helper, and high-growth kernels, before
claiming general neutrality. The tiny scalar pilot alone cannot establish it.

Also check:

- Zero snapshot construction and no full-clone tracking on the disabled path.
- Explicit capture levels with measured peak memory and compile-time overhead.
- Request isolation, write failure, truncation, and no-clobber behavior.
- Correct new-event versus cumulative-observation semantics after cleanup.
- New function IDs or replacement mappings if specialization creates functions
  after the normalized snapshot. Never invent identity to satisfy the importer.
- Backend legality preserved independently of the instrumentation configuration.
- Identical output-byte evidence and the same finite behavior oracle during
  migration. Broader equivalence or performance claims require broader checks.

## Current truth and coordination

Source context: session `01a06e1b-f1c4-7861-a0f3-86188a264dfa`; this note was
checked against the live local worktrees, not just earlier plans.

- Fe: `/workspace/fe-worktrees/bloat-toolkit`, branch `bloat-toolkit`, base
  `1de776e33a3d844cf723159d54c4b7f0d3b5ae8c`. The files listed above are in flight.
- Toolkit: `/workspace/riff-catalog-worktrees/bloat-toolkit`, branch
  `bloat-toolkit`, implementation commit `8c1ba34`, preceded by `63fad74`.
- Existing pins: Sonatina `54c6c63307eedfffaed4c538b1049ae81683b33b`, Naga fork
  `854ed576fa186d19b9f7d5a5bb5a1f2fa533dde1`. Neither was changed for this pilot.
- Preserve unrelated work in `/workspace/fe-worktrees/mb2` and
  `/workspace/riff-catalog`. Coordinate ownership before editing the same Fe
  instrumentation files. No pushes are authorized.

Done: structured importer, schema, accounting checks, regression fixtures, and
runbook. Full toolkit gate: `cargo nextest run --workspace`, 165 passed / 1 skipped.
Legacy capture replay retains its original address and unknown completion status.

Doing: first Fe examples build and live validation. No live size or behavior
result is claimed here yet. Next: compile baseline/variant, verify exact outputs,
execute the finite-domain oracle and negative control, run focused Fe regressions,
then attach measured results and final Fe revision.

Later: reusable Sonatina callbacks, broader MB2 corpus, backend-internal
attribution, structural region matching. Set aside for this slice: compiler-wide
canonicalization redesign and claims of profitability from repeated hashes alone.

## First action for the MB2 instrumentation owners

Read-only first:

```sh
git -C /workspace/fe-worktrees/bloat-toolkit status --short
git -C /workspace/fe-worktrees/bloat-toolkit diff -- crates/codegen/src/sonatina/spirv_lower.rs
```

Then read `bloat_capture.rs` and the wire fixture. Expect a Fe-owned adapter over
existing Sonatina facts, not a Sonatina API patch. Compare this boundary with
your current instrumentation plan and propose the first shared callback surface.
Before implementation, agree on file/worktree ownership and compatibility tests.
Do not change pins, merge into the dirty MB2 worktree, or push as part of review.
