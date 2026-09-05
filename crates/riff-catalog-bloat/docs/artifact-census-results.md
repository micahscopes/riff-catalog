# Artifact census: results and next gates

Delivered: exact regions and direct-projection runs, generic region manifests,
immutable replayable sidecars, capture binding, explicit comparison hints,
checked scalar-copy/accounting models and shared Rust/Lean vectors. Compiler
ownership stays with MB2. No compiler, core graph hash or optimization changes.

## Saved production artifact

Input: `/workspace/scratch/mb2-bloat-composition-20260905/`
`request-427773-1788626819680907141-compute/shader.wgsl` (`linear_ports`).
BLAKE3: `e358781a0fc79e7ac2b7eeb76c1cf5131a696b6af330119bf2924d5e18c00b0a`.

| Quantity | Measured |
| --- | ---: |
| Entire artifact | 1,011,478 bytes |
| `sparse_linear_copy_plan` | 690,388 bytes |
| Repeated direct-copy runs inside it | 51 |
| Statements per run | 210 |
| Those 10,710 statements' union coverage | 620,298 bytes |
| All simple projection moves inside it | 11,138 statements, 641,282 bytes |
| Matching runs in the whole shader | 53 |

The two extra runs belong to `sparse_linear_replayed_ports`, covering 23,366 bytes.
These measurements reproduce the manual finding, not a guarantee of removable
bytes or an automatic semantic diagnosis.

The 19 saved baseline artifacts sum to 12,936,420 bytes. Region union is 12,455,949
and outside-region bytes are 480,471. That partition is a construction invariant,
not independent evidence of correct region selection.

The original supplementary JS census was independently rerun on those files.
All 19 artifact sizes and all 95 sampled function sizes (its top five per shader)
agree exactly. Its broader move classification includes 480 additional lines:
391 literal-RHS assignments and 89 nonliteral-index accesses intentionally excluded
by the strict projection policy. The independent line comparison found no other
discrepancy in that sample. The dominant helper's broader move count agrees exactly.

Evidence: `/workspace/scratch/riffcat-census-pilot-final-20260905/`, including
per-stage sidecars/reports, `corpus-summary.json`, `independent-js.jsonl`,
`crosscheck.jsonl`, `crosscheck-summary.json`, and `crosscheck-gaps.json`.
The read-only supplementary scripts are
`/workspace/scratch/mb2-composition-text-census-20260905.js` and
`/workspace/scratch/riffcat-census-crosscheck-gaps-20260905.js`.

## Verification and limitations

- Real complete and budget-exhausted MB2 captures census and replay with artifact
  verification. Partial status remains explicit. Comparisons retain interventions
  and provenance separately from source/compiler/settings alignment.
- The saved scalar intervention compares 518 to 422 WGSL bytes. `fs_main` shrinks
  by 20 bytes, `mix_words` becomes unpaired/absent, and `vs_fullscreen` stays equal.
  This reuses the earlier behavior-tested pilot's artifacts; no new GPU execution
  or runtime result is claimed.
- Three warm debug runs on linear_ports, JSON serialized without sidecar writes:
  0.38, 0.38, 0.37 seconds; peak RSS 19,228, 19,152, 19,204 KiB. The proposed
  10x-artifact RSS target was not met. These are one-machine, one-artifact observations,
  not universal bounds or compiler timings.
- Artifact/token/region limits and a selected-region-byte work budget bound input
  processing. Sidecar size is checked before atomic no-replacement publication.
  Failed-write, relocation, clobber, overlap, tamper and incomplete-context cases
  have regressions.
- Full `cargo nextest run --workspace`: 211 passed, one intentionally skipped
  golden-file regeneration test. Log:
  `/workspace/scratch/riffcat-census-final-nextest-20260905.log`.
- `lake build` checks the proof module. The interpreter bridge checks five shared
  coverage vectors and a byte-cost counterexample. Rust also checks exhaustive
  two-interval cases through length eight and 2,000 deterministic randomized
  many-interval cases against masks. Logs:
  `/workspace/scratch/riffcat-census-lean-20260905.log` and
  `/workspace/scratch/riffcat-census-bridge-final-20260905.log`.

## Go / no-go

| Question | Boundary |
| --- | --- |
| Which regions occupy bytes? | Go with digest-bound spans and union accounting; selection remains adapter evidence. |
| Does a direct-copy pattern repeat? | Go under its lexical policy, with scopes and explicit unassigned counts. |
| Does a pattern digest determine byte cost? | No: root-name changes preserve the pattern but change byte length. Checked counterexample. |
| Do equal initial values justify aliasing locations? | No: the scalar-store counterexample changes later results. |
| Can scalar-copy renamings compose? | Yes in the checked model, under injectivity and transported state. Not a WGSL/compiler proof. |
| Can repeated stores be removed or a speedup claimed? | Not from this census. Require representation correctness and independent behavior/runtime evidence. |

The mask proofs are not a proof of the Rust interval sweep. Shared vectors and
property tests provide an executable bridge, not a universal refinement theorem.
The lexical matcher has no claimed binding-aware equivalence proof.

## Next inputs and separate slices

1. The requested `scratch/wgsl-codegen-review-20260905/riff-cat-feedback.md` was
   absent from accessible workspace/repository scratch directories. Its additional
   requests have not been reviewed or implemented.
2. Obtain the post-fix and intermediate-regression 19-shader corpora and provenance.
   Only the baseline was available: the reported 2,706,004-byte final corpus and
   15,066,556-byte intermediate corpus were not reproduced here. Compare named
   regions without inventing source alignment.
3. Obtain a current Quilting failing kernel, expected output and execution evidence.
   Older unrelated WGSL files are not a controlled current comparison.
4. Add separate literal-store and interleaved-temporary policies when demanded by
   those inputs. Do not relax root erasure in place: preserve cross-statement
   dependencies and add no-go counterexamples first.
5. Gate binding-aware body/specialization comparison, full interval-refinement
   proofs and lower token-memory use as separate slices. Causal IR/Naga/source
   attribution still requires producer-supplied lineage.
