# fe origin-overhaul and riffcat-as-a-library (2026-06-24)

Static, read-only review. No builds were run. All citations are file paths,
commit hashes, and line ranges read directly from source or via `git show
<branch>:<path>`.

Repos:
- fe compiler: `/workspace/fe`, branch `origin-overhaul-phased` (tip
  `e0e5419c9`, 2026-06-10).
- riffcat: `/workspace/riff-catalog` (local-only Rust workspace).

Context docs reused rather than re-derived:
- `/workspace/tracing-architecture-consolidated-draft-2026-06-10.md` (655-line
  consolidated spec; its line citations into both repos were spot-checked and
  match).
- `/workspace/riff-catalog/demo/riffcat-similarity-experiments-2026-06-23.md`
  (the weighted-containment / Jaccard / containment work).

---

## A. What origin-overhaul-phased adds to fe, and how stale it is

### A.1 Staleness

- merge-base with `master`: `b5c62cdcf3af7fd28bf33b5c51912e852e5cca39`
  ("Write bytecode artifacts without trailing newlines (#1452)", 2026-05-17).
- `git log <base>..origin-overhaul-phased --oneline | wc -l` = **453 commits**.
- In this clone, `git log <base>..master | wc -l` = **0**: the local `master`
  is pinned at the base, so the branch is 453 ahead with no measured drift
  *in this checkout*. That is a clone artifact, not a claim that upstream
  master has not moved. Real-world rebase distance is unknown from here; the
  backup branch name itself records a prior rebase onto argot/master
  (`backup/origin-overhaul-phased-pre-argot-master-rebase-20260530-124130`).
- Calendar staleness: branch tip 2026-06-10, today 2026-06-24, so ~2 weeks
  idle. Base is 2026-05-17.

### A.2 What it adds

New crates introduced on the branch (all absent at base; confirmed via
`git diff <base>...origin-overhaul-phased --stat --diff-filter=A -- '*Cargo.toml'`):

| crate | role |
|---|---|
| `crates/shape-address` (+ `shape-address-macros`) | the structural hasher prototype riffcat descends from |
| `crates/trace-facts` (+ `trace-facts-macros`) | the typed origin/provenance/trace fact schema + JSONL bundle |
| `crates/trace-query` | reports, audits, joins, the introspection service / workbench backend |
| `crates/debug-export` | DWARF / ethdebug / DebugBundle emission |
| `crates/introspection-config` | shared trace/introspection config |
| `crates/solc-runner` | Solidity compile support for benches/comparison |

Whole-branch diffstat: `612 files changed, 136817 insertions(+), 91329
deletions(-)`.

Foundational origin commits (oldest first), the load-bearing ones:
- `f860597aa` "Add typed origin identity core" -> `crates/common/src/origin.rs`
  (the `OriginExportKey` type), plus `crates/hir/src/origin.rs`,
  `crates/mir/src/origin.rs`.
- `5ef3b3cdc` "Add stable origin export keys".
- `7bca1ef14` "Add typed origin fact projections".
- `28bbfde8f` "Add shape address graph model" through `b8840517a` "Add SCC
  shape graph hashing" / `1b6e29d39` "Add shape hash trace facts": the
  shape-address hasher and its fact emission.
- A long tail (the bulk of the 453) is the trace **workbench**: web demo, LSP
  trace sessions, missing-link / attribution audits, live trace projection,
  PC-map joins. This is consumer/UI work on top of the origin substrate, not
  the substrate itself.

Key data shapes on the fe side (read directly):

- `OriginExportKey { kind, owner_key, local_key }` with private fields,
  validated, U+001F-joined canonical storage key, serde `deny_unknown_fields`:
  `crates/common/src/origin.rs:62-160`. Doc comment at `:57-61` states "Stable
  key for an origin node that leaves the compiler", `kind` owned by the phase
  crate.
- `OriginNodeFact { key: OriginExportKey }` and
  `OriginEdgeFact { from, to, label: OriginEdgeLabel, introduced_by:
  Option<CompilerPhase> }`: `crates/trace-facts/src/fact.rs:231-300`.
- `OriginEdgeLabel` (13 variants: LoweredFrom, EmittedFrom, SyntheticFor,
  IntegerLegalizationFor, Load/Store/Spill/ReloadOf, Inlined/CallsiteOf,
  PreservedSnapshotIdentity, BackendPrepared, Unmapped) and
  `OriginEdgeTraversalClass` (ExactAttribution, Structural, Contextual,
  Synthetic, SnapshotAlias, Unmapped) with `classify_origin_edge`:
  `fact.rs:349-420`.
- Shape facts that re-emit riffcat-style digests over the wire:
  `ShapePolicyFact`, `ShapeNodeHashFact { node, graph, policy, local, tree,
  component }`, `ShapeComponentHashFact`, `ShapeGraphHashFact`, and
  `shape_hash_facts(...)`: `fact.rs:1806-1960`.
- JSONL bundle: `TraceBundle { metadata, facts }`, `TraceMetadata`,
  `TraceJsonlRecord::{Metadata, Fact}`, `JsonlTraceSink`, and
  `TRACE_SCHEMA_VERSION = 1`: `crates/trace-facts/src/jsonl.rs:7-230`.
- The prototype hasher `crates/shape-address/src/lib.rs`: `ShapeDimension`
  (Structure/Names/Constants/Types/TraceEvents, `:12-40`),
  `ShapeDigestAlgorithm`, `ShapeViewMode`, `ShapeCyclePolicy`,
  `ShapeNodeKey::{Entity, Derived}`, `ShapeGraphKey`, `ShapeHashPolicy`,
  `ShapeEdgeRole` (Graph/Control/Data/Reference/Call/Dependency/**Origin**,
  `:526-552`), `ShapeGraph`, `shape_digest(...)`, `SHAPE_SCHEMA_VERSION = 1`.

---

## B. Connection to riffcat: producer/consumer is already half-built

This is not a hypothetical pairing. riffcat's own README
(`/workspace/riff-catalog/README.md:82-90`) says it is the "Successor to the
`shape-address` prototype in the fe repo" and that "the graph model is built to
take the fe compiler's origin keys at the boundary" (`:15-18`). The branch is
the other half.

### B.1 The two sides are near-isomorphic

| concept | fe (origin-overhaul-phased) | riffcat (riff-catalog-core) |
|---|---|---|
| entity identity | `OriginExportKey { kind, owner_key, local_key }` `common/src/origin.rs:62` | `EntityKey { kind, owner, local }` `key.rs:13` (doc: "Mirrors fe's `OriginExportKey` shape so fe can map 1:1") |
| node key | `ShapeNodeKey::{Entity, Derived}` `shape-address/src/lib.rs:156` | `NodeKey::{Entity, Derived}` `key.rs:59` |
| graph key | `ShapeGraphKey` `shape-address:198` | `GraphKey` `key.rs:94` |
| dimensions | `ShapeDimension` 5-set incl. `TraceEvents` `shape-address:12` | `Dimension` 5-set incl. `TraceEvents` `dimension.rs:10` |
| policy | `ShapeHashPolicy` / `ShapePolicyFact` `shape-address:221`, `fact.rs:1806` | `HashPolicy` `policy.rs:90` |
| per-node digests | `ShapeNodeHashFact { local, tree, component }` `fact.rs:1852` | `NodeHashes { local, tree, component }` `hash/mod.rs:84-93` |
| component digests | `ShapeComponentHashFact` `fact.rs:1883` | `ComponentHash` `hash/mod.rs:95-105` |
| graph digest | `ShapeGraphHashFact` `fact.rs:1911` | `GraphHashes.graph: DimensionDigests` `hash/mod.rs:108-114` |
| edge roles incl. provenance | `ShapeEdgeRole::Origin` `shape-address:534` | `EdgeRole::Origin` `graph.rs:86` |

The field names differ (`owner_key`/`local_key` vs `owner`/`local`) but the
structure is the same; both sides serde the same shapes.

### B.2 Does fe produce what riffcat's `trace_events` dimension wants?

Two distinct artifacts on the fe side, and they map to two different riffcat
mechanisms. This is the subtle part.

1. **The origin/provenance graph** (`OriginNodeFact` + `OriginEdgeFact`). This
   is an *attribution* graph: which postopt instruction came from which MIR
   stmt came from which HIR expr came from which source span, with edge labels
   and a per-phase introducer. In riffcat terms this is a `Graph` whose
   provenance edges carry `EdgeRole::Origin`. riffcat **deliberately excludes
   `EdgeRole::Origin` from the structural digest**
   (`hash/graph_digest.rs:53-56`: `.filter(|edge| edge.role != EdgeRole::Origin)`):
   origin edges are provenance payload that must not perturb the shape
   fingerprint. So fe's origin graph is consumed by riffcat as *graph topology
   and identity*, not as the thing the structural facet hashes.

2. **The `trace_events` dimension** is a different, currently-empty slot.
   `Dimension::TraceEvents` is defined (`dimension.rs:15`) and is part of every
   facet (`Facet::full`, `Facet::names_blind`), but it has **no producer in any
   riffcat lowering crate**. A grep across `crates/` for `TraceEvents` outside
   `dimension.rs` returns only tests
   (`riff-catalog-core/tests/golden.rs:93,129`, `tests/hashing.rs:85`,
   `riff-catalog-solidity/tests/stability.rs:167`). The consolidated spec flags
   exactly this: section 3.3.4 "`TraceEvents`: spec or strike, before any
   producer ships" notes it is "Unfed on both sides today" and proposes the
   payload "likely: ordered multiset of `(event_kind, origin-key-digest)`
   records on function nodes."

So the honest answer to "does fe produce the trace_events artifact riffcat is
meant to fingerprint": **not yet, on either side.** What fe *does* produce is
the origin/provenance graph and (via `shape_hash_facts`) riffcat-shaped
per-node/component/graph digests. The natural producer/consumer relationship is
real and the identity types already line up 1:1; the `trace_events` dimension
specifically is an unfilled hole both repos carry, and feeding it is a
`SCHEMA_VERSION` bump because riff folds every dimension over the same skeleton
(adding records to a node changes that node's digest at every facet that
includes the dimension).

### B.3 What "consume" means concretely today

riffcat's actual ingestion entry points (`crates/riff-catalog-cli/src/ingest.rs`)
all go through solc: `.sol`, `.yul`, irAst JSON, sourcify. There is **no path
that ingests an fe `TraceBundle` JSONL**, and no `from_trace_facts` lowering.
The corpus persistence (`crates/riff-catalog-cli/src/corpus.rs:26-72`) is a
JSONL `Record` enum (Artifact / Graph / Digest / Claim / Attestation) that is
*similar in spirit* to fe's `TraceJsonlRecord` but is a different schema. The
two JSONL formats are cousins, not the same wire format.

---

## C. riffcat as a library: API, schema/protocol, fe as first producer

Grounding facts first:
- `riff-catalog-core` is **already a clean, salsa-free library crate** (deps:
  blake3, serde, thiserror only; `crates/riff-catalog-core/Cargo.toml`). Its
  `lib.rs` re-exports a coherent surface: `Graph`/`GraphSink`/`Node`/`Edge`,
  `digest_graph`, `GraphHashes`/`NodeHashes`/`DimensionDigests`,
  `Facet`/`FacetAddress`/`ArtifactRef`, `DigestIndex`/`FacetIndex`/`LookupRequest`,
  `EntityKey`/`GraphKey`/`NodeKey`, `HashPolicy`, `SCHEMA_VERSION = 2`.
- `riff-catalog-claims` is also a library (claims, attestations, claim-gated
  lookup; `CLAIMS_SCHEMA_VERSION = 2`).
- The lowering crates (`-solidity`, `-yul`, `-evm`) each expose `lower_*`
  functions and a versioned `*_LEVEL` string. They are libraries too.
- What is **CLI-only** today: `ingest` (compile + persist), the corpus JSONL
  loader, and every query (`bucket`, `overlap`/Jaccard, `diff`, `conformance`,
  `claim`/`attest`). These live in `crates/riff-catalog-cli/src/` and are *not*
  reusable except by shelling out to the binary.
- The only non-CLI consumer surface is `riff-catalog-wasm`
  (`crates/riff-catalog-wasm/src/lib.rs`): two functions, `fingerprint_yul`
  and `fingerprint_source`, each taking solc JSON and returning a JSON array of
  `{unit, name, digests, facets}`. It is a thin wrapper over
  `lower_* -> digest_graph`, explicitly "byte-identical to one computed
  natively". This is the closest thing to a stable library boundary that
  exists, and it is the template for the rest.

So: the *engine* is already a library; what is missing is (1) a single facade,
(2) library-level ingest and query that today only the CLI has, and (3) a
published on-the-wire schema distinct from the in-memory serde types.

### C.1 The public library API riffcat should expose

A `riff-catalog` (or `riffcat`) facade crate, re-exporting core + claims and
adding the two boundaries the CLI currently monopolizes:

- **Ingest (engine-side, solc-free).** `ingest_graph(graph: Graph) ->
  GraphFingerprint` and `ingest_graphs(impl IntoIterator<Item = Graph>)`. A
  producer that already has a `Graph` (fe does, via its shape lowerings) should
  never need solc. The solc-driven ingest stays in a `riff-catalog-ingest-solc`
  crate (lift `ingest.rs` out of the CLI) so the engine has no solc dependency.
- **Fingerprint.** Already exists: `digest_graph(&DigestRequest, &Graph) ->
  DigestResult` (`hash/mod.rs:147`). Re-export verbatim; this is the SSOT.
- **Facet projection.** Already exists: `GraphHashes::facet_address(&Facet) ->
  FacetAddress` (`hash/mod.rs:119`) and the named constructors `Facet::full /
  names_blind / structure_only` (`reference.rs:101-126`). Re-export; add a
  `Facet::new(policy_id, dimensions)` convenience (exists, `reference.rs:86`).
- **Per-node Merkle digests.** Already computed and returned in
  `GraphHashes.nodes: BTreeMap<NodeKey, NodeHashes>` with `local` / `tree` /
  `component` (`hash/mod.rs:84-114`). No new code; just promote it as a
  documented, supported return rather than a workbench-internal detail. This is
  the substrate the similarity work needs (per the similarity-experiments doc:
  the node-digest multiset).
- **Similarity / weighted-containment.** This does **not** exist as a library
  function. Today `overlap` computes only contract-level Jaccard over shared
  whole-graph class digests inside the CLI
  (`queries.rs:219-267`). The function-level fuzzy multiset metrics (Jaccard,
  directional containment, size-weighted containment over the per-node `tree`
  digest multiset) are described and prototyped only in `sim.py` per
  `riffcat-similarity-experiments-2026-06-23.md` (sections A.2, B.1, C.1-C.3),
  and the doc explicitly notes "(a) FUNCTION-LEVEL FUZZY ... NOT exposed by the
  CLI" (`:170`). Promote these to library functions in core (they generalize
  unchanged over the node-digest multiset, and over CondenseScc the multiset is
  member colors + component digest, per `:104-106`):
  `jaccard(a: &GraphHashes, b: &GraphHashes, facet)`,
  `containment(a, b, facet)` (state direction; `C(A in B) != C(B in A)`),
  `weighted_containment(a, b, facet)`. Reporting must surface the leaf floor
  separately (the doc: "A 'similarity' that is just the leaf floor is noise",
  `:130`).
- **Index / lookup.** Already exists as library types: `DigestIndex`,
  `FacetIndex`, `LookupRequest`/`LookupResult` (`index.rs`), and the
  claim-gated layer `ClaimGatedIndex`/`FacetClosure` in `-claims`. Re-export.

The CLI then becomes a thin shell over this facade (and `ingest-solc`), which is
the same refactor the wasm crate already demonstrates is possible without
touching core.

### C.2 A stable on-the-wire schema / protocol

The pieces of a protocol exist but are not assembled into one named, versioned
contract:

- **Self-describing reference**: `ArtifactRef` already serializes to/parses from
  the URI `riffcat:<schema>:<policy-hex>:<dimension>:<digest-hex>`
  (`reference.rs:17-72`). This is the atom of the protocol. Invariant I1/I7: a
  bare hash never travels without schema version, policy id, dimension.
- **Versioning**: `SCHEMA_VERSION = 2` in core (`lib.rs:57`),
  `CLAIMS_SCHEMA_VERSION = 2` in claims, and fe's `TRACE_SCHEMA_VERSION = 1` in
  trace-facts. Three independent version counters today; a unified protocol
  needs them reconciled or explicitly layered.
- **Records**: riffcat's `corpus::Record` (Artifact/Graph/Digest/Claim/
  Attestation, `corpus.rs:26-72`) and fe's `TraceJsonlRecord` (Metadata/Fact,
  `jsonl.rs:168`) are both JSONL-one-record-per-line, both serde-derived, but
  are different enums. Neither is documented as a frozen external schema; both
  are "whatever serde emits."

Proposal: a `riff-catalog-schema` doc-and-fixtures layer that freezes:
1. The `ArtifactRef` URI grammar (already stable; just bless it).
2. A `FingerprintRecord` = `{ graph_key, policy_id, schema_version,
   digests: BTreeMap<Dimension, Digest>, node_count }`: this is *already*
   exactly riffcat's `Record::Digest` (`corpus.rs:52-62`) and exactly fe's
   `ShapeGraphHashFact` plus owner metadata. Freeze one shared form.
3. Optionally a `NodeFingerprintRecord` mirroring `ShapeNodeHashFact` /
   `NodeHashes` for the similarity path.
4. The origin-graph wire form: `OriginNodeFact` + `OriginEdgeFact` +
   `OriginEdgeLabel`, which become riffcat `Node`s + `EdgeRole::Origin` edges
   on ingest.
5. The kind-string registry (the consolidated spec, section 3.4, lists the full
   current namespace as a table and argues it is load-bearing because
   `TracePhase::from_key` is prefix-driven and an unregistered kind silently
   misclassifies).

Enforcement mechanism that costs least: golden fixture files of serialized
records checked into both repos and asserted byte-identical on each side
(riffcat already freezes its encoding with golden tests per `README.md:88-90`).
A shared `fe-origin-key` microcrate (consolidated spec 3.4) is the stronger
option since riffcat *already replicates fe's key validation verbatim*
(`key.rs` Name validation vs `origin.rs:184-200`).

### C.3 fe origin-overhaul as the first producer against that schema

The consolidated spec already lays this out as a decision (section 3.3, "One
hasher: riff-catalog-core") and it is the right call:

- fe's `shape-address` crate should **not** ship to fe master; instead fe
  depends on `riff-catalog-core` (a `fe-riff` adapter crate) and re-points its
  shape facts (`ShapePolicyFact`/`ShapeNodeHashFact`/`ShapeComponentHashFact`/
  `ShapeGraphHashFact`, `fact.rs:1806-1960`) at riff's `DimensionDigests` /
  `PolicyId` / `GraphKey`, which are drop-in shape-compatible. This deletes a
  second, known-leaky hasher (the prototype used key-ordered SCC hashing; riff
  fixed three anonymity leaks with WL refinement, per `README.md:84-90`).
- fe's hir/mir/codegen shape lowerings become `riff_catalog_core::Graph`
  builders. They already are graph builders against `ShapeGraph`, which has the
  same `add_node`/`add_field`/`add_child`/`add_edge` surface
  (`shape-address/src/lib.rs:596+` vs riffcat `graph.rs:152-209` /
  `GraphSink` trait `:243-266`), so the port is mechanical and local.
- fe then emits, against the frozen schema: (1) per-unit `FingerprintRecord`s
  (it already does, as shape facts), and (2) the origin graph as riff `Graph`s
  with `EdgeRole::Origin` provenance edges. riffcat ingests both with no solc
  in the loop.
- `riff_catalog_core` is kept salsa-free; fe owns the `#[salsa::tracked]`
  wrappers and memoizes only the small `DimensionDigests`, building the fat
  `GraphHashes` transiently (consolidated spec 3.3.3).

This makes fe the first non-solc producer and exercises exactly the ingest /
schema boundary section C.1 and C.2 propose.

---

## D. Risk and sequencing

**Minimal first step (load-bearing, unblocks everything):** freeze the shared
identity + fingerprint-record schema. Concretely: lift `EntityKey`/
`OriginExportKey` field names into one form (the spec recommends riff's
`owner`/`local`; fe accepts the old names via serde alias for one release),
extract a zero-dep key microcrate, and check in golden serialized fixtures on
both sides. This is read-only on the engine and is the precondition for fe
ever producing against riffcat without forking the hasher. Cheap, high
leverage, no algorithm risk.

**Second (mechanical, no behavior change):** add the solc-free
`ingest_graph(Graph)` entry and lift the solc driver into its own crate; split
a `riff-catalog` facade that re-exports core + claims + the new ingest. The
wasm crate already proves core lowers and digests unchanged off the CLI path,
so this is plumbing, not redesign.

**Third (real design work, defer until the schema is frozen):** promote the
similarity metrics (Jaccard / directional containment / weighted containment
over the per-node `tree`-digest multiset) from the `sim.py` prototype into core
library functions, with the leaf-floor reported separately and direction always
stated. This is the only genuinely new algorithmic surface; everything else is
re-exposing what `digest_graph` already returns.

**Defers / explicitly out of scope until decided:**
- `TraceEvents` dimension. It is unfed on both sides and folding any payload
  into it is a `SCHEMA_VERSION` bump that moves every existing facet address.
  Spec it (ordered multiset of `(event_kind, origin-key-digest)` on function
  nodes) or strike it before any producer ships. Do not let it ride as
  vocabulary without semantics. **This is the single biggest correctness trap**:
  shipping a producer that writes TraceEvents silently re-bases every cached
  fingerprint.
- Reconciling the three schema version counters (riff core 2, claims 2, fe
  trace 1) into one layered protocol version.
- Whether the Sonatina-shape digests are lowered fe-side or in-tree (spec 3.3.1
  leaves this an owner decision; fe-side is the zero-Sonatina-coupling default).

**Risk notes:** the engine carries deliberate v1 scope cuts that any library
consumer inherits and must be told about (1-WL incompleteness; per-function
unit graphs treat out-of-graph callees as opaque; `yulCFGJson` is experimental
and fenced by its level string) per `README.md:92-98`. None of these block the
library boundary; they are honesty obligations on the API docs. The branch
itself is ~2 weeks idle and 453 commits of mostly-workbench churn on top of a
small origin substrate; if it must rebase onto a moved upstream master, the
origin core (`common/src/origin.rs`, the fact schema) is the durable part and
the workbench/web-demo tail is the part most likely to conflict.
