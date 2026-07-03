# riffcat into fe origin-tracing and instrumentation: the riffcat-side draft (2026-07-03)

Scope: what riffcat and the riffcat demo contribute to a new draft that folds
riffcat back into fe's origin-overhaul (origin tracing + instrumentation). The fe
side is sketched only, on purpose (per direction). Everything here is a riffcat
workspace change or a demo chapter; nothing here requires editing fe.

Grounding: file:line citations are read directly from the current tree
(`/workspace/riff-catalog`, branch `main`) and from fe branch
`origin-overhaul-phased` (tip `e0e5419c9`, present locally). Builds referenced
are not re-run here.

---

## 0. What already shipped since the 2026-06-24 review (re-anchor)

The prior review (`demo/fe-origin-overhaul-and-riffcat-library-2026-06-24.md`)
proposed a sequence whose first two steps are now DONE on the riffcat side. Do
not re-plan them:

- **The wire contract is a crate.** `riff-catalog-schema` exists: the pure-data
  interchange types with no hashing. `EntityKey { kind, owner, local }`
  (`key.rs:12`, doc says "Mirrors fe's `OriginExportKey` shape so fe can map
  1:1"), `NodeKey::{Entity, Derived}` (`key.rs:59`), `GraphKey` (`key.rs:94`),
  `Graph`/`Node`/`Edge`/`EdgeRole`/`Field`/`GraphSink` (`graph.rs`),
  `Dimension` (closed 5-set incl. `TraceEvents`, `dimension.rs:10`), `Value`,
  `Digest`, `Name`. `EdgeRole::Origin` is a defined variant (`graph.rs:86`). The
  engine depends on this crate and re-exports it, so `riff_catalog_core::{Graph,
  EntityKey, ...}` is unchanged.
- **A solc-free facade with ingest and similarity exists.** `riff-catalog`
  (`crates/riff-catalog/src/lib.rs`) re-exports core and adds
  `ingest_graph(&Graph, level) -> GraphHashes` (`:17`, "the entry a producer
  such as fe would call", no solc), plus `containment` (`:37`) and `similarity`
  (Jaccard over per-node subtree digests, `:53`). This is exactly the
  producer/consumer boundary the 06-24 doc's steps 2 and 3 called for.
- **The engine already treats provenance as payload, not shape.** The structural
  fold excludes origin edges: `graph_digest.rs:3` ("Flat edges (every role
  except Origin) fold through EVERY dimension") and `:56`
  (`.filter(|edge| edge.role != EdgeRole::Origin)`). This is the single most
  important property for the fe pairing and it is already true and tested.

So the missing pieces are narrow: freeze the contract with fixtures, add a path
that turns an fe origin bundle into riff graphs, decide the `trace_events`
(instrumentation) payload, and a demo chapter that shows the whole loop with the
engine doing the work.

Current version counters: core `SCHEMA_VERSION = 2` (`lib.rs:57`), claims
`CLAIMS_SCHEMA_VERSION = 2`, fe `TRACE_SCHEMA_VERSION = 1` (trace-facts). Still
three counters; see section 4.

---

## 1. What the new draft is

A new branch in the riffcat workspace that makes riffcat ready to be fe's first
non-solc producer and consumer of origin/trace data, plus one demo chapter that
presents the story. Three riffcat-side workstreams (A, B, C) and one demo
workstream (D). The fe side (section 4) is a sketch only.

Naming for the branch: `origin-tracing-integration` (or similar). It is a draft;
land it as clean SSOT-respecting increments, never a churn pile.

---

## 2. Workstream A: freeze the wire contract (load-bearing, zero algorithm risk)

The types are stable; what is missing is a frozen, byte-checked external form so
fe and riffcat cannot drift silently. This is read-only on the engine.

1. **Golden serialized fixtures.** Check in canonical JSON (and the JSONL record
   forms) for each wire type: `EntityKey`, `NodeKey::{Entity,Derived}`,
   `GraphKey`, a small `Graph` that includes at least one `EdgeRole::Origin`
   edge and one field per `Dimension`, and a `FingerprintRecord` (see below).
   Assert byte-identical round-trip in a serde-stability test in
   `riff-catalog-schema`. The same fixture files are the ones fe checks against
   on its side (the enforcement mechanism the 06-24 doc section C.2 point 5
   recommends: golden files asserted byte-identical on both sides).
2. **serde aliases for fe's field names.** fe's `OriginExportKey` uses
   `owner_key` / `local_key`; riffcat's `EntityKey` uses `owner` / `local`
   (`key.rs:14`). Add `#[serde(alias = "owner_key")]` / `#[serde(alias =
   "local_key")]` so fe-emitted JSON parses into `EntityKey` for one release
   while fe renames. This is the cheapest bridge and burns down cleanly (delete
   the aliases once fe renames). Keep `deny_unknown_fields` behavior in mind: if
   the fe struct carries extra fields, decide alias-vs-newtype per field.
3. **Bless a `FingerprintRecord`.** `{ graph_key, policy_id, schema_version,
   digests: BTreeMap<Dimension, Digest>, node_count }`. This already exists in
   spirit as riffcat's corpus `Record::Digest` and as fe's `ShapeGraphHashFact`
   plus owner metadata. Define ONE shared struct in `riff-catalog-schema`
   (data-only, no hashing) so both sides emit the same record. Add it to the
   golden fixtures.
4. **The `ArtifactRef` URI grammar is already stable**
   (`riffcat:<schema>:<policy-hex>:<dimension>:<digest-hex>`). Bless it in the
   fixtures as the atom of the protocol (a bare hash never travels without
   schema version, policy id, dimension). It lives in core today; the frozen
   grammar and its fixtures can sit alongside the schema fixtures.

Deliverable A: fixtures + serde-stability tests + serde aliases + the
`FingerprintRecord` type. No engine behavior changes.

---

## 3. Workstream B: ingest an fe origin bundle without solc

fe emits two artifacts (06-24 doc section B.2): the origin/provenance graph
(`OriginNodeFact` + `OriginEdgeFact` with a 13-variant `OriginEdgeLabel` and a
per-phase introducer) and riff-shaped shape digests. The origin graph maps
directly to riff:

- `OriginNodeFact { key }` becomes a riff `Node` keyed by the corresponding
  `NodeKey` (its `EntityKey` mirrors `OriginExportKey` 1:1 after the alias
  bridge).
- `OriginEdgeFact { from, to, label, introduced_by }` becomes a riff `Edge` with
  `EdgeRole::Origin`, carrying the `label` and `introduced_by` as fields on the
  edge (provenance payload).

Add a reader: `ingest_trace_bundle(jsonl: &str) -> Result<Vec<Graph>, _>` in the
facade (or a small `riff-catalog-ingest-trace` crate if we want the facade to
stay dependency-thin, matching how solc ingest is meant to live in its own
crate). Then `ingest_graph` digests each graph unchanged.

The property to lock with a test, because it is the whole point of the pairing:
**adding the origin/provenance edges does not move the structural facet
address.** Ingest the same graph twice, once with `EdgeRole::Origin` edges and
once without, and assert the `Structure` (and every non-`TraceEvents`) facet
address is identical, while the origin edges remain queryable as topology. This
is already guaranteed by `graph_digest.rs:56`; the test makes it a contract the
fe integration can rely on and the demo can show.

Deliverable B: `ingest_trace_bundle` + the "provenance rides along, does not
perturb shape" test. Mechanical, no new algorithm.

---

## 4. Workstream C: the instrumentation dimension (spec-or-strike TraceEvents)

This is the "instrumentation" half of the ask and the one real decision. Today
`Dimension::TraceEvents` is defined (`dimension.rs:15`) and folded into every
facet, but it has NO producer on either side. Feeding it is a `SCHEMA_VERSION`
bump because riff folds every dimension over the same Merkle skeleton: adding
records to a node changes that node's digest at every facet that includes the
dimension. This is the single biggest correctness trap in the whole
integration: a producer that silently writes `TraceEvents` re-bases every cached
fingerprint.

Decision for the draft:

1. **Define the payload.** Ordered multiset of `(event_kind, origin-key-digest)`
   records attached to function nodes (the shape the consolidated spec 3.3.4
   proposes). `event_kind` is a closed enum (like `Dimension` and `EdgeRole`, an
   open string is drift bait). `origin-key-digest` is the digest of the
   `EntityKey` the event points at, so instrumentation references provenance by
   content, not by raw key.
2. **Gate it behind a version.** Implement it behind `SCHEMA_VERSION = 3` (or,
   if we do not want to move the live address yet, behind a distinct policy
   level string that fences it exactly like `yulCFGJson` is fenced). Never let a
   producer emit `TraceEvents` under the current `SCHEMA_VERSION = 2`. State the
   re-base cost in the API docs.
3. **Ship it OFF by default.** The v1 posture is: the dimension exists, the
   producer path is fenced, and turning it on is an explicit, versioned choice.
   This keeps every existing facet address stable while the instrumentation
   story is designed.

Deliverable C: the `event_kind` enum + a `trace_events` field producer in the
schema/graph builder, fenced behind a version bump, with a test that proves an
empty `trace_events` slot leaves the `SCHEMA_VERSION = 2` address unchanged and
a fed slot only moves the address under `SCHEMA_VERSION = 3`. This is the piece
to land LAST and to hold if the version story is not settled.

---

## 5. Workstream D: the demo chapter (compute, do not bake)

One new chapter that makes the fe pairing legible by running the engine on an
origin graph. It is a synthetic stand-in for fe's origin facts (a small lowering:
one source expression to HIR to MIR to a post-opt form, with `EdgeRole::Origin`
edges linking each lowered node back to its source), built in JS and handed to
the engine. Nothing about the answer is baked; the chapter calls a wasm binding
and renders what comes back.

What the chapter shows, in order:

1. **Provenance rides along.** Compute the structural facet address of the
   lowering with the origin edges present and with them stripped. Same address.
   The point: origin/provenance is queryable payload, it does not move the
   shape fingerprint. This is the property from workstream B, shown live.
2. **The origin edges explain divergence.** Two lowerings of the same source
   that differ only by an optimization pass have high structural containment
   (the fork-detection metric, `containment` in the facade), and the origin
   edges point at exactly the nodes that differ. Recognition-led: the engine
   re-finds "these are the same shape except here", and provenance says where.
3. **(optional, gated) The honest cost of instrumentation.** Fold a
   `trace_events` payload in and show the address move under the version bump.
   The demo's own "locality runs out" honesty register: turning on
   instrumentation re-bases the address, and that is a deliberate versioned
   choice, not a silent one.

New wasm binding needed (the demo cannot reach the facade otherwise):
`ingest_origin_graph(graph_json: &str) -> String` that parses a
`riff-catalog-schema` `Graph`, runs `ingest_graph`, and returns digests + facet
addresses + per-node hashes as JSON. A companion `containment(a_json, b_json,
dimension)` binding for step 2. These are thin wrappers over the facade, the same
template `fingerprint_riff` / `node_digests` already follow. Keep the chord and
music bindings untouched.

Deliverable D: the `ingest_origin_graph` (+ `containment`) wasm bindings and the
demo chapter, rendered from the engine. Register A vocabulary, no em-dashes,
recognition-led framing.

---

## 6. What stays sketched on the fe side (deliberately shallow)

Not planned in detail here. The shape is: fe depends on `riff-catalog-schema`
(to speak the wire form) and `riff-catalog-core` (one hasher), deletes its
`shape-address` prototype (a second, known-leaky hasher), re-points its shape
facts (`ShapePolicyFact` / `ShapeNodeHashFact` / `ShapeComponentHashFact` /
`ShapeGraphHashFact`) at riff's `DimensionDigests` / `PolicyId` / `GraphKey`,
and emits `FingerprintRecord`s plus the origin graph against the frozen fixtures.
The serde-alias bridge (workstream A2) covers the `owner_key` / `local_key`
rename for one release. `riff_catalog_core` stays salsa-free; fe owns the
`#[salsa::tracked]` wrappers. Three schema-version counters (core 2, claims 2,
fe trace 1) get reconciled or explicitly layered as part of fe's side, not here.

---

## 7. Sequencing and risk

1. **A (freeze the contract)** first. Read-only on the engine, zero algorithm
   risk, unblocks everything. Golden fixtures are the cheapest drift enforcement.
2. **B (trace-bundle ingest)** next. Mechanical mapping onto existing types; the
   Origin-excluded-from-structural-digest property it depends on is already
   true and tested.
3. **D (demo chapter)** on top of A and B, so the story is presentable early.
4. **C (TraceEvents)** last, and hold it if the version story is unsettled. It
   is the only step that moves live addresses.

Single biggest hazard: never emit `TraceEvents` under `SCHEMA_VERSION = 2`. The
whole draft is safe as long as A, B, D touch only the current version and C is
fenced behind a bump. The v1 scope cuts the engine already carries (1-WL
incompleteness, per-function unit graphs treat out-of-graph callees as opaque)
are honesty obligations on the API docs, not blockers.

---

## 8. Concrete deliverables of this draft (riffcat + demo only)

- (A) golden serialized fixtures + serde-stability tests + `owner_key`/
  `local_key` serde aliases + a shared `FingerprintRecord` type, all in
  `riff-catalog-schema`.
- (B) `ingest_trace_bundle` (facade or a small ingest-trace crate) + the
  "provenance does not perturb the structural address" test.
- (C) a fenced `trace_events` producer behind a `SCHEMA_VERSION` bump, held
  until the version story is settled, with the empty-slot-stability test.
- (D) `ingest_origin_graph` (+ `containment`) wasm bindings and one demo chapter
  that runs the engine on a synthetic fe-style origin graph.
- this doc.
