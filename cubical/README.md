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
- `Riffcat/Music.agda` (Deliverable 4): pitch-class sets as 12-bit characteristic
  vectors, transposition as cyclic rotation (the Z/12 action), inversion as the mirror
  (the dihedral action), the orbit relation as the congruence, `SetClass = Pcs / ~SC`,
  and `primeForm` (least rotation under a left-packing lex order) as the normalization.
  `major-minor-same-setclass : Cmajor ~SC Aminor` holds by `refl`; all of C major, A
  minor, C minor, and G major compute their prime form to exactly `[0, 3, 7]`. The
  non-collapse `major-augmented-different-setclass : ¬ (Cmajor ~SC Caug)` is a checked
  inequality (the augmented triad's prime form is `[0, 4, 8]`).
- `Riffcat.agda`: the top module. It re-exports the four pieces and gathers the
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
- The music instance: prime form computes to `[0, 3, 7]` for every major/minor triad,
  major equals minor as set classes, and the major/augmented non-collapse.

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
the nix store are reused, so only the five prototype modules are compiled (a couple of
seconds), not the whole cubical library.

One-time library setup (already done in this environment; recorded here for
reproducibility). Create `~/.agda/libraries` pointing at the cubical library:

    /nix/store/wdmb2i9bwkhmhj6rgm0wllaj947kpfv0-cubical-0.9/cubical.agda-lib

This project's `cubical-riffcat.agda-lib` declares `depend: cubical` and the flags
`--cubical --no-import-sorts --guardedness` (the `--guardedness` flag is required
because the cubical library was built with it, and that flag is infective on import).

Verify command (from this `cubical/` directory):

    /run/current-system/sw/bin/agda Riffcat.agda

A clean run prints the five "Checking ..." lines and exits 0, with no errors and no
warnings. To force a from-scratch recheck, remove the local `_build/` first.

## Relationship to the Lean port

`../lean/` is the byte-oracle: it shares the canonical pre-hash encoding with the Rust
engine and re-implements BLAKE3 in pure Lean so it can compare final digest bytes
byte-for-byte. That is the right job for an executable witness. This cubical prototype
is the law-prover: it abstracts the hash and proves the quotient / transport / h-level
laws, which are NATURAL in cubical (transport computes) and AWKWARD in Lean (`Quot` is
opaque, `Prop` is proof-irrelevant). Different witness, different job. Lean owns the
bytes; cubical owns the cubical-specific laws. This is the capstone witness.
