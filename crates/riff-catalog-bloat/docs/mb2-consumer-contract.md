# MB2 observation consumer agreement

Status: ownership confirmed; the first real MB2 complete and partial captures
pass consumer compatibility regressions. Protocol: agent-handoff/v1.

Checkpoint: `/workspace/scratch/mb2-observe-compat-20260905/README.md`.
Exact request fixtures and producer provenance are in `tests/fixtures/mb2-observe`;
`tests/mb2_compat.rs` tests import, verified CLI replay, deterministic output and
tampering of both WGSL and SPIR-V for complete and budget-exhausted requests.
No consumer wire-schema change was needed. The delivery sequence below records
the original agreement; the first capture exchange is now complete.

## Ownership and outcome

MB2 owns the compiler recorder, its typed API, driver/request configuration,
encoding, Fe hooks, Sonatina coordination, and shared-mb2 integration and validation.
Riff-cat will not implement a competing recorder or edit Fe/Sonatina files.
The existing Fe pilot patch is a source of hooks and tests, not a requirement to
preserve its recorder internals. Adapt it to the compiler-owned boundary.

Riff-cat owns external ingestion, artifact verification, evidence accounting,
reports, graph projections, and explicit cross-run comparison claims. Neither
compiler identity, legality nor cache decisions depend on those claims.
No Ethdebug dependency or integration is in scope. Nothing is to be pushed.

## Source and current state

The governing integration contract is
`/workspace/fe-worktrees/mb2/docs/dev/compiler-observation-integration.md`,
committed as `6cc22779a2627d66f2c1bf50fd5724611ec292bc` and checked on 2026-09-05.
Its typed, caller-owned recorder and acceptance gates take precedence over the
pilot's internal design. It does not yet specify a new concrete wire encoding.

The Fe candidate is `908116b4b0714a36fb77bb53808d2762c2f6c4e8` in
`/workspace/fe-worktrees/bloat-toolkit`. Do not transplant its separate
InstanceIndex prerequisite over shared raster support or replace current
`spirv_lower.rs` wholesale. Preserve MB2's newer repeated-loop retention policy.
The shared MB2 tree has unrelated tracked and untracked changes; they are not
owned by this work.

Riff-cat consumer work is isolated in
`/workspace/riff-catalog-worktrees/observe-consumer`, branch `observe-consumer`,
based on `eebacf4c89649bacce310505ceabda477803d40b`. The working tree was clean
before this documentation change. The completed pilot remains in the separate
`bloat-toolkit` worktree. See [verified results](live-results.md) for its evidence;
those results do not establish compatibility with a future MB2 recorder.

## Consumer boundary

1. Preserve a projection accepted by `src/fe_events.rs` during migration.
   JSONL is an interchange format, not a requirement on the recorder's Rust API.
   A new version is coordinated with a real fixture, not independently invented
   by riff-cat. Keep archived captures replayable.
2. Keep request, stage and entity scopes explicit. Matching names or arena
   numbers do not establish cross-stage or cross-run identity. Producer
   derivations and consumer alignment claims remain different evidence.
3. Keep exact artifact bytes and digests separate from producer measurements,
   including backend versus outer WebBundle WGSL. Verify referenced bytes before
   presenting replay as verified or passing snapshots to graph adapters.
4. Preserve partial completion, unsupported records and coverage gaps visibly.
   Missing attribution is not zero expansion. Full clone events, fast-path
   counts, cumulative original-ID survival and unknown descendants stay separate.
5. Future graph comparisons must identify snapshot format, producer version,
   graphicalization policy and facet. Artifact identity, scoped compiler entity
   identity and facet equality are separate objects. Parser incompatibility is
   reported, not repaired by silently changing dependency pins or erasing facts.
6. Facet matches are scoped structural comparison results. They do not alone
   prove semantic equivalence, rewritten lineage, or causally attributed savings.

The compiler contract requests best-effort ordinary recording plus explicit strict
mode. The pilot's opted-in writer currently fails compilation on recording errors.
MB2 owns that behavioral migration. If a sink fails, failure reporting needs a
channel that does not depend on writing another record to that same failed sink.
Riff-cat must never infer successful completion merely from a readable prefix.

## Delivery sequence and evidence

- Done: the pilot structured importer, artifact-verified replay and contained
  experiments exist. The integration contract and this ownership split are recorded.
- Doing: riff-cat consumer compatibility preparation; no replacement recorder.
- Next, MB2: adapt the candidate behind the typed boundary and supply one small
  complete capture plus a partial/failure capture, with exact artifacts, producing
  revision/settings and the import command or versioned schema description.
- Next, riff-cat: import those fixtures, add compatibility regressions and verify
  deterministic replay, artifact tampering rejection, scoped references and honest
  incomplete coverage. Then build the stage waterfall and helper expansion ranking.
- Later: verified RMIR/Sonatina snapshot projections and production attribution,
  including Naga/WGSL ranges when producer evidence supports them.
- Set aside: a competing bundle schema, universal provenance substrate, and
  compiler decisions driven by riff-cat addresses.

Compatibility is verified for this checkpoint's existing JSONL projection only.
Compiler neutrality, disabled-path cost, no-clobber/isolation, finite-domain
behavior checks and optimized-build overhead measurements remain MB2 gates from
the governing contract, not claims established by importer tests.

## First action for MB2

Read the integration contract at `6cc22779a` and inspect the candidate diff against
current shared MB2. Recorder ownership is no longer a dependency on riff-cat:
proceed with the small typed compiler-owned boundary and compatibility projection.
Expected first exchange is a real complete and partial capture for importer tests.
Coordinate before breaking the projection or changing identity/coverage semantics;
do not wait for a new riff-cat recorder implementation.
