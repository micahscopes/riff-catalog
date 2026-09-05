# Cubical riffcat: a Cubical Agda prototype

A Cubical Agda prototype of riffcat's core. It typechecks green, and it demonstrates
the genuinely-cubical layer the Lean port cannot reach: the facet as a set-quotient
higher inductive type (HIT), the anchors theorem as the HIT's recursion principle,
transport along a quotient path that COMPUTES, and the music set-class instance.

This is a PROTOTYPE, not the byte-for-byte port. The Lean development
(`../lean/`) remains the BLAKE3 byte-oracle. Per the established hash-split, this
cubical witness proves the quotient / transport / h-level laws over an ABSTRACT hash;
it does not implement BLAKE3 (a third hand-rolled hash would add no assurance about
BLAKE3 and would be huge). This is the capstone witness on the cubical-specific laws.

## What it shows (the cubical-specific layer)

1. The facet is a real set-quotient HIT, `A / ~_F`, via `Cubical.HITs.SetQuotients`.
   The content address is its normalization, exhibiting an effective / split quotient
   (decide a class by computing and comparing addresses, never touching the path
   constructor).
2. The anchors theorem IS the SetQuotients recursion principle: a fact out of
   `A / ~_F` is exactly a function on `A` that respects `~_F` (`rec` / `elim`). This is
   the quotient's universal property.
3. Transport along a quotient path COMPUTES. `eq/ a b r : [ a ] = [ b ]` is a genuine
   path, and riding an anchored fact along it reduces, by `refl`, to the supplied
   respect-proof. In Lean 4, `Quot.sound` is an opaque proof with no such reduction
   rule, so the same transport is inert. This is the heart of why cubical and not Lean,
   and it is a CHECKED lemma, not prose.
4. The music instance grounds the abstract HIT: pitch-class sets, a transposition and
   inversion group action, the set class as a quotient, the prime form as the
   normalization, and major and minor triads landing in the same set class (Forte
   3-11, prime form `[0, 3, 7]`), as a checked example that computes.
5. Composition is facet-safe exactly when the operation cannot observe distinctions
   the input facets forgot. `Riffcat.Composition` constructs the descended query and
   linker with `rec`/`rec2`; both commuting squares compute by `refl`. Its converse
   proves the stability evidence is necessary, not decoration. A concrete two-port
   example then shows a coarse facet losing a link-critical label and a residual
   restoring enough information for the linker to descend.
6. The useful kinds of linking form a small ladder. Preservation gives safe descent
   and reuse. Reflection says the output kept every distinction in the selected
   input observation. Both together give an exact change key. Exact reconstruction
   is stronger again: it needs a residual and a round-trip law.
7. Addressed documents can be modeled as bundles of named parts. A consumer declares
   which parts it observes, so a link between two parts does not acquire accidental
   dependencies on the rest of the document. Facet-safe links and serial queries
   compose.
8. Salsa 0.28.2 supplies one operational reading: dependency equality permits reuse;
   changed dependencies may require a rerun; equal rerun output is backdated; only
   changed output forces downstream regeneration.
9. Cache invalidation is proof-relevant and dimension-indexed, rather than one global
   dirty bit.  Per-facet change evidence forms an algebra, and compiler passes
   transport that evidence while preserving empty changes and merging.  Causal cache
   receipts add horizon-relative liveness, atomic support bundles, transitive
   retraction, and checked hard revision filters.

## The modules

- `Riffcat/Fold.agda` (Deliverable 1): the dimension-tagged structure as an inductive
  type `Term D`, and the catamorphism `fold` (the acyclic per-dimension digest as the
  unique algebra map) over an ABSTRACT `Digest`. `addr = fold hashAlg` is the engine's
  `node_tree_digest` modeled exactly as a catamorphism. The defining equation
  `addr (node l cs) = hashRecord l (map (addr) children)` holds by `refl`
  (`addr-node`, `addr-node-map`); the substitution principle is `addr-cong-children`.
- `Riffcat/Facet.agda` (Deliverable 2): the facet congruence `FacetRel D` (equal
  addresses), the quotient `Facet D = Term D / FacetRel D`, the ANCHORS THEOREM as
  `anchor = rec` with its computation rule `anchor-β`, the normalization `addrFacet`
  exhibiting the EFFECTIVE / SPLIT quotient (`sameAddr->sameClass`,
  `sameClass->sameAddr`), and FACET REFINEMENT (`Refinement.coarsen↠`: a finer
  quotient surjects onto a coarser one, the quotient-of-a-quotient factorization).
- `Riffcat/Transport.agda` (Deliverable 3): the CUBICAL PAYOFF. `anchor-rides` and
  `addr-rides` are the abstract laws (riding a fact / the address along a facet path
  reduces to the respect-proof, by `refl`). The `Concrete` submodule is a fully
  concrete instance (parity on the naturals) where `parity-rides : cong parityFact
  (eq/ 0 2 r) ≡ refl` holds by `refl`, and the endpoints reduce to actual booleans.
  Nothing is postulated in this module.
- `Riffcat/Composition.agda` (Deliverable 4): PROOFS AS DOCUMENTATION for faceted
  queries and linking. `QueryTransport` states the Salsa-style reuse contract;
  `FacetLink` proves that independently faceting two inputs and then linking agrees
  with linking first and observing the output; `FacetLinkNecessary` proves the
  converse obligation. `Remembering` models a residual as the small saved distinction
  a later operation still needs. `PortExample` is fully concrete: forgetting a Bool
  port label breaks linking congruence, while remembering it restores a commuting
  linker. The construction and path-action laws compute by `refl`.
- `Riffcat/LinkingKinds.agda` (Deliverable 5): a checked map of the design space.
  `Preserves`, `Reflects`, and `SameKernel` state exactly what follows from a
  transformation; their composition laws are proved. `LosslessWithResidual`
  separates exact reconstruction from approximate guessing. `Parts` models a
  document as named, independently addressed parts and proves selected-part linking
  and serial query composition.
- `Riffcat/Salsa.agda` (Deliverable 6): the public red-green contract of Salsa
  0.28.2, presented as one operational interpretation of the general laws. It
  distinguishes reuse, backdating, and output change. Its concrete accumulator
  example proves that the main value can stay equal while an auxiliary value changes,
  motivating independently observable output ports for diagnostics and indices.
- `Riffcat/Invalidation.agda` (Deliverable 7): replaces a global dirty bit with a
  dimension-indexed change algebra.  Each facet chooses its own evidence type and
  associative merge; whole change sets merge pointwise.  `InvalidationTransport`
  preserves no-change and merge, and its checked composition law lets a pass move,
  discard, enrich, or combine facet effects.  `InvalidationPolicy` is the explicit
  home for richer cache logic and requires a proof for every `keep` decision.  The
  concrete reason-bag instance shows a rename invalidating Names while Structure and
  Types remain unchanged.  Dependent transport moves change evidence coherently along
  facet-identification paths.  A separate concurrent-algebra boundary adds
  commutativity and idempotence when replica arrival order and duplicate delivery must
  not matter; reason lists alone deliberately do not claim that stronger law.
- `Riffcat/Cache.agda` (Deliverable 8): a small causal cache and co-transaction
  correspondence.  It distinguishes an immutable artifact from its revisable
  receipt, models `(observed-at, judged-from)` liveness, proves all-or-nothing bundle
  liveness and transitive retraction, and makes hard revision filters carry their
  skip-safety proof.  Complete-support cache keys must prove that key equality
  determines verdict equality.  Green reuse, backdating, replacement, and retraction
  remain distinct evidence types.
- `Riffcat/Music.agda` (Deliverable 9): pitch-class sets as 12-bit characteristic
  vectors, transposition as cyclic rotation (the Z/12 action), inversion as the mirror
  (the dihedral action), the orbit relation as the congruence, `SetClass = Pcs / ~SC`,
  and `primeForm` as the normalization, under the SAME rule as the Rust engine: the
  most compact rotation, i.e. minimal as a 12-bit integer with pitch class 11 the most
  significant bit (Rahn's rule, the published catalog's). `major-minor-same-setclass :
  Cmajor ~SC Aminor` holds by `refl`; all of C major, A minor, C minor, and G major
  compute their prime form to exactly `[0, 3, 7]`. The non-collapse
  `major-augmented-different-setclass : ¬ (Cmajor ~SC Caug)` is a checked inequality
  (the augmented triad's prime form is `[0, 4, 8]`), and `Aminor7-prime` pins the
  packing rule on the case that separates the conventions: the minor seventh (Forte
  4-26) computes the compact `[0, 3, 5, 8]`, not the lex-least `[0, 2, 5, 9]`, the
  exact case the engine's `prime_form` fix pinned. The explicit inversion generator
  `invertI` checks the demo's A/B story at the Tn level: major and minor are distinct
  transposition classes (3-11B vs 3-11A) and `invertI` maps one onto the other.
- `Riffcat.agda`: the top module. It re-exports the nine pieces and gathers the
  headline checked results (`transport-computes`, `music-major-equals-minor`,
  `music-major-not-augmented`). Typechecking this module green is a single green light
  over the whole prototype.

## What is proved vs postulated

PROVED (typechecked, many by `refl`, i.e. they hold by computation):

- The catamorphism's defining equation and the map-shape of it (`addr-node`,
  `addr-node-map`), the substitution principle (`addr-cong-children`).
- The facet relation is a propositionally-valued equivalence relation
  (`FacetRel-prop`, `FacetRel-equiv`), the quotient is a set (`isSetFacet`).
- The anchors theorem (`anchor = rec`) and its computation rule (`anchor-β`, by
  `refl`).
- The effective / split quotient maps and their round-trip (`addrFacet`,
  `addrFacet-β`, `sameAddr->sameClass`, `sameClass->sameAddr`).
- Facet refinement: the finer quotient surjects onto the coarser one
  (`Refinement.coarsen-surjective`, `coarsen↠`), instantiated for real by the music
  tower (`setClassFromTransClass↠`).
- The transport-computes lemmas, abstract and concrete (`anchor-rides`, `addr-rides`,
  `Concrete.parity-rides`), all by `refl`.
- Facet-sensitive query and link composition (`QueryTransport`, `FacetLink`), the
  necessity of link congruence (`FacetLinkNecessary`), residual refinement
  (`Remembering`), and the concrete saved-port-label composition
  (`PortExample.saved-link-commutes`).
- Preservation/reflection/same-kernel composition, residual round trips, selected
  part linking, and bundle-query composition (`Riffcat.LinkingKinds`).
- Salsa-style green/backdated/changed outcomes and the checked main-versus-auxiliary
  counterexample (`Riffcat.Salsa`).
- Dimension-indexed invalidation algebra laws; identity and composed invalidation
  transports; and the checked rename-only example (`Riffcat.Invalidation`).
- Atomic support-bundle liveness, transitive retraction, sound hard-bound skipping,
  complete-support verdict reuse, and the artifact-versus-warrant example
  (`Riffcat.Cache`).
- The music instance: prime form computes to `[0, 3, 7]` for every major/minor triad,
  major equals minor as set classes, the major/augmented non-collapse, the compact
  4-26 prime form `[0, 3, 5, 8]` (the packing-rule case shared with the engine), and
  the explicit-inversion A/B checks at the Tn level.

POSTULATED (the abstract hash, and only that):

- `Digest : Type`, `isSetDigest : isSet Digest`, and
  `hashRecord : {D} -> Local -> List (Frame D x Digest) -> Digest` in `Riffcat/Fold`.
  This is the hash-split boundary. The Lean port owns the concrete BLAKE3 bytes; the
  cubical witness proves the laws over an abstract hash. There are no other postulates
  in the prototype.

TARGETS NOT BUILT HERE (named honestly, not attempted):

- The cyclic case (SCC condensation + 1-WL color refinement as a finite quotient by
  `kernel refine`, NOT bisimilarity). The design note treats this; this prototype
  builds the acyclic catamorphism and the facet HIT, which are the EXACT parts.
- The h-level-ladder de-truncation (the verifier's proof-relevant path one level up)
  and the no-computable-closure negative result. These are aspirational / research-grade
  per the design note and are deliberately out of scope for the prototype.
- Univalence. It is not used and must not be claimed here; the prototype lives at
  h-level <= 1 (set-quotients, truncation, transport), exactly as riffcat does.

## How to typecheck it

Agda 2.8.0 with the cubical library 0.9. The cubical library's prebuilt interfaces in
the Nix store are reused, so only the nine component modules and their top module are
compiled, not the whole cubical library.

This project's `cubical-riffcat.agda-lib` declares `depend: cubical` and the flags
`--cubical --no-import-sorts --guardedness` (the `--guardedness` flag is required
because the cubical library was built with it, and that flag is infective on import).

Reproducible verify command from this `cubical/` directory:

    nix shell --impure --expr \
      'with import <nixpkgs> {}; agda.withPackages (p: [ p.cubical ])' \
      --command agda Riffcat.agda

If Agda and Cubical are already registered in the active environment, the shorter
equivalent is simply:

    agda Riffcat.agda

A clean run prints the module "Checking ..." lines and exits 0, with no errors and no
warnings. To force a from-scratch recheck, remove the local `_build/` first.

## Relationship to the Lean port

`../lean/` is the byte-oracle: it shares the canonical pre-hash encoding with the Rust
engine and re-implements BLAKE3 in pure Lean so it can compare final digest bytes
byte-for-byte. That is the right job for an executable witness. This cubical prototype
is the law-prover: it abstracts the hash and proves the quotient / transport / h-level
laws, which are NATURAL in cubical (transport computes) and AWKWARD in Lean (`Quot` is
opaque, `Prop` is proof-irrelevant). Different witness, different job. Lean owns the
bytes; cubical owns the cubical-specific laws. This is the capstone witness.
