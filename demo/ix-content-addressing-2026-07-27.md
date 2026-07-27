# How Argument Computer / Ix / Lurk actually use content addressing (2026-07-27)

Follow-up to `lean-lurk-priorart-2026-06-24.md`, which was written before the
hhhs "resolution as a second axis" turn and before Ix had this much code in it.
Read from source via `gh api` (argument.xyz and the Lurk blog 403 this sandbox's
egress proxy, same as in June). Ix repo state: pushed 2026-07-26, 84 stars, still
flagged pre-alpha "should not be used for any purpose".

## 1. The three layers, and what is addressed in each

### Lurk (the substrate)

Every expression is identified by a type-tagged cryptographic hash; compound data
is built from SNARK-friendly Poseidon hashes, so an s-expression is a Merkle tree
of tagged cons cells. Code is data, so programs are addressed on the same footing
as values. Lurk supports commitments: commit to a function now, open it on an
input later. The Lurk 0.5 headline was **functional memoization**, exploiting
program structure to avoid redundant computation.

The important reading for us: in Lurk the address is not primarily a public name.
It is the **interning key inside the evaluator**, and memoization is what it buys.

### Ix (the live heir: zkPCC for Lean 4)

- `Ix/Address.lean`: `Address` is a **32-byte Blake3 hash**. The doc comment says
  the address is the key for "the kernel's whnf/defeq/infer caches and the intern
  table". Addressing as resolution and memoization, stated outright.
- `Ix/Ixon.lean` opens with: "**Ixon: Alpha-invariant serialization format for
  Lean constants.**" That is their declared invariant. Binder names are
  computationally irrelevant, so the address forgets them. References to other
  constants are carried **by `Address`** (`Constant.refs : Array Address`),
  universes are positional (`univs : Array Univ`), and shared subterms are
  tabled (`sharing : Array Expr`). One facet, frozen by design.
- **Content and metadata are separate objects.** Hashed: `Constant { info,
  sharing, refs, univs }`. Not hashed, carried alongside: `ConstantMeta` with an
  `ExprMetaArena` whose nodes hold binder names, `mdata` KVMaps, `ref` names,
  projection struct names, and a `callSite` node whose `entries` are in **source
  order** with `kept (canonIdx, metaIdx)` / `collapsed (sharingIdx, metaIdx)`
  cases, plus "surgery" handling for elaborator-inserted and evaporated aux
  recursors. So: identity over the canonical form, and a parallel arena that
  records exactly how the source form differs, enough to reconstruct it.
- `Ix/Store.lean`: a real content-addressed store at `~/.ix/store` with hex
  fan-out directories; `write` returns `Address.blake3 bytes`.
- `Ix/Merkle.lean`: two modes. `merkleRootCanonical` over **lex-sorted, deduped**
  leaves gives "deterministic env identity" (our corpus root, same instinct);
  `merkleJoin` composes two existing roots in O(1). Leaves are
  `blake3(0x00 || addr)`, nodes `blake3(0x01 || l || r)`, RFC 6962 style. Odd
  levels are padded with a fixed 32-byte zero sentinel rather than duplicating
  the trailing leaf, explicitly to avoid CVE-2012-2459 root malleability.
- `Ix/Claim.lean`, the part that matters most to us:

  ```
  inductive Claim where
    | eval     (input output : Address) (assumptions : Option Address)
    | check    (const : Address)        (assumptions : Option Address)
    | checkEnv (root : Address)         (assumptions : Option Address)
    | reveal   (comm : Address) (info : RevealConstantInfo)
    | contains (tree : Address) (const : Address)
  ```

  A claim is a statement **about addresses**. A conditional claim carries
  `assumptions`, the **Merkle root of the set of things it depends on**:
  "`none` -> unconditional; `some root` -> conditional on every leaf in the
  merkle tree rooted at `root` being well-typed". `crates/ixon/assumption_tree.rs`
  keeps the actual leaves so "the root alone tells the verifier which set was
  assumed; the AssumptionTree carries the actual leaves so the verifier can
  inspect them (e.g. do I trust each of these axioms?)". `merkleJoin` unions two
  claims' assumption sets without re-sorting; `contains` discharges one leaf out
  of a conditional claim's set.
- `crates/aiur` (their zkVM): `querymap.rs` memoizes each (function, input) query
  with a multiplicity counter bumped on memo hits. Pay once per distinct query,
  reuse everywhere it recurs. Same economy as the store, one layer down.

### Argument Computer (the org)

One lineage, three names: Yatima Inc / Lurk Lab / now Argument Computer
(`argumentcomputer`). Current active surface: `ix`, `multi-stark`, their `sp1` and
`zisk` forks, `Blake3.lean`, `EVMYulLean` (executable EVM + Yul model in Lean 4).
Lurk-the-language repos are quieter; the reduction-machine work moved into
Aiur / multi-stark under Ix.

## 2. Alignment with our larger theory

Three of our four load-bearing pieces show up independently in Ix. One does not,
and it is the one we are proposing.

**(a) Canonicalize, then Merkle-hash.** Same primitive, as we already knew. We
must keep not claiming it.

**(b) Provenance is payload, not identity.** Their `Constant` / `ConstantMeta`
split, with a source-order `callSite` arena recording elaborator surgery, is the
same decision as our origin edges riding along while the address is computed on
the shape alone. Independent arrival at our fe origin-tracing design, in a
different language, for a different reason (they need round-trip to source; we
need the catalog to dedup through provenance). Strong external validation.

**(c) A fact rides an address, and its footprint is explicit.** This is the big
one. `assumptions : Option Address` plus `AssumptionTree` plus `merkleJoin` plus
`contains` is our anchors/cover discipline, made cryptographic and composable:
the set a claim depends on is itself content-addressed, unions are O(1), and
individual dependencies can be discharged later. Our formulation ("a fact rides
an anchor soundly only if the anchor keeps every dimension the fact depends on")
is the general case of their concrete one. Their soundness is cheap because their
single quotient, alpha-invariance, is provably irrelevant to typechecking. Ours
is the hard case: a facet can drop something a fact actually needed, which is
exactly the failure our anchors chapter shows on purpose.

**(d) Address as resolution, not identity.** Lurk's functional memoization, Ix's
address-keyed intern table and whnf/defeq/infer caches, and Aiur's query
multiplicities all say the same thing: the first job of an address is interning
and memoization inside a computation. That is the hhhs reading, confirmed by the
most sophisticated implementation of it in the wild.

**What remains ours: the ladder.** Ix has exactly one address per object, and
must: a zk claim has to be about a fixed object. There is no graded similarity,
no weighted containment, no per-dimension read, and no second address for the
same artifact anywhere in the design. Their `reveal` bitmask is disclosure
scoped, not identity scoped: it shows fewer fields of the same committed
constant; the address does not move. "One artifact, several useful addresses,
pick the narrowest one that preserves your fact" is still our slice.

## 3. Does it support "content addressing eases hooking in FV tools and
alternative compilation components"?

For FV tools: **yes, and Ix is the best available evidence.** Their entire
product is a proof that attaches to an address and travels, with its dependency
set explicit and composable. `checkEnv(root)` is "everything in this environment
typechecks"; `contains` and `merkleJoin` let independently produced claims be
combined and partially discharged without re-running anything. That is the
integration win stated in machine-checkable form, not as an aspiration.

For alternative compilation components: **partially, and the gap is instructive.**
Their addressing does decouple the object from the toolchain that produced it,
and the Ix README names our two neighbouring use cases directly: attach proofs
that compilation happened correctly (a Lean CompCert equivalent, against Thompson
trusting-trust supply chain attacks), and "proofs showing that bytecode was
generated from particular sources (currently a trusted block explorer feature)",
which is Sourcify's job named out loud. But their address is over the
**elaborated Lean object under alpha-invariance**. It is stable under renaming;
nothing about it is stable across a different elaborator, optimizer, or compiler.
Yatima's README went further and required identical toolchains. So Ix gives you
plumbing that is toolchain-agnostic while its addresses are not toolchain-stable.

The honest sentence, which is also the talk's sentence: content addressing makes
the plumbing cheap (a stable name, a store, dedup, memoization, composable
claims with explicit assumption sets). It does not by itself make two toolchains
agree. Agreement comes from choosing the invariant, and choosing one that forgets
something a fact needed makes the transported fact false. Swapping a compiler
component therefore needs a **coarser facet than alpha-invariance**, plus the
cover condition to say what survives. That is our contribution, and it is not in
their design.

## 4. Two things to steal, one thing to cite

1. **Check our corpus Merkle root against theirs.** They domain-separate leaves
   (`0x00`) from nodes (`0x01`) and pad odd levels with a zero sentinel rather
   than duplicating the trailing leaf, citing CVE-2012-2459. If our order
   independent corpus root duplicates a trailing leaf or does not domain
   separate, two distinct corpora can collide on a root. Worth an hour.
2. **Make a footprint an addressed set.** Our per-fact footprint is currently
   prose plus a facet index. Theirs is a Merkle root over the dependency set,
   with O(1) union and per-leaf discharge. Adopting that shape makes transport
   set inclusion, composition cheap, and partial discharge expressible. Candidate
   for the schema, downstream of freezing it.
3. **Cite Ix precisely on the prior-art slide.** Not "Ix addresses Lean
   declarations modulo cosmetic variation" but: Blake3 addresses over an
   alpha-invariant serialization, names and source order kept in a separate
   metadata arena, and claims that carry the Merkle root of their assumption set.
   Then the honest line: they built the transport layer for one facet; the ladder
   is what we are proposing. To this audience that reads as fluency, not rivalry.

## Source ledger

All read this session via `gh api` on `argumentcomputer/ix` at HEAD
(pushed 2026-07-26): `Ix/Address.lean`, `Ix/Ixon.lean`, `Ix/Store.lean`,
`Ix/Merkle.lean`, `Ix/Claim.lean`, `Ix/Environment.lean`,
`crates/ixon/src/assumption_tree.rs`, `crates/aiur/src/{lib,querymap}.rs`, README.
Org repo listing via `gh api orgs/argumentcomputer/repos`. Lurk characterization
from search extracts of argument.xyz/blog (prog-intro, perf-2024) and
filecoin.io "Introducing Lurk"; the canonical pages 403 this sandbox, so the
Lurk-layer claims above are extract-level, not full-text verified, same caveat
as the June doc.
