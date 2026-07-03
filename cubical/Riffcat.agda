{-# OPTIONS --cubical --no-import-sorts --guardedness #-}

{-
  Riffcat: a Cubical Agda PROTOTYPE of riffcat's core.

  This is the top module. It re-exports the four pieces and collects the headline
  checked results in one place, so `agda Riffcat.agda` typechecking green is a single
  green light over the whole prototype.

  What this prototype is, and is not:

  - It is a PROTOTYPE that demonstrates the genuinely-cubical layer the Lean port
    cannot reach: the facet as a set-quotient HIT, the anchors theorem as the
    SetQuotients recursion principle, transport along a quotient path that COMPUTES,
    and the music set-class instance.
  - It is NOT the byte-for-byte port. The Lean development (riff-catalog/lean/) owns
    the BLAKE3 byte-oracle. Per the established hash-split, this witness proves the
    quotient / transport / h-level laws over an ABSTRACT hash; it does not implement
    BLAKE3 (a third hand-rolled hash would add no assurance about BLAKE3 and would be
    huge). The abstract hash lives in Riffcat.Fold as the only postulate.

  See README.md for the full account of proved-vs-postulated and how to typecheck.
-}

module Riffcat where

open import Cubical.Foundations.Prelude using (_≡_ ; refl ; cong)
open import Cubical.Relation.Nullary using (¬_)
open import Cubical.HITs.SetQuotients using (eq/)

------------------------------------------------------------------------
-- 1. The dimension-tagged structure + the catamorphism fold, over an abstract hash.
------------------------------------------------------------------------
open import Riffcat.Fold public

------------------------------------------------------------------------
-- 2. The facet as a set-quotient HIT: the congruence, the quotient, the anchors
--    theorem (rec), the effective/split quotient via the normalization, and facet
--    refinement (finer surjects onto coarser).
------------------------------------------------------------------------
open import Riffcat.Facet public

------------------------------------------------------------------------
-- 3. The cubical payoff: transport along a quotient path computes (by refl), both
--    abstractly and in a fully concrete instance.
------------------------------------------------------------------------
open import Riffcat.Transport public

------------------------------------------------------------------------
-- 4. The music instance: pitch-class sets, transposition/inversion action, the
--    set class as a quotient, prime form as normalization, and major = minor.
------------------------------------------------------------------------
open import Riffcat.Music public

------------------------------------------------------------------------
-- The headline checked results, gathered (each is proved above, by refl unless noted):
------------------------------------------------------------------------

-- The fold's defining equation is definitional (the catamorphism computes):
_ : {D : Dimension} (l : Local) (cs : _)
  → addr (node {D} l cs) ≡ hashRecord {D} l (foldChildren (hashAlg D) cs)
_ = addr-node

-- The anchors theorem computes: riding a fact along a facet path reduces to its
-- respect-proof. (Abstract law; the concrete instance is Concrete.parity-rides.)
_ : Concrete.SameParity 0 2
_ = Concrete.r0-2

-- The transport-computes headline, fully concrete and by refl:
transport-computes : cong Concrete.parityFact
                       (eq/ {R = Concrete.SameParity} 0 2 Concrete.r0-2) ≡ refl
transport-computes = Concrete.parity-rides

-- The music headline: C major and A minor share a set class (Forte 3-11), by refl:
music-major-equals-minor : Cmajor ~SC Aminor
music-major-equals-minor = major-minor-same-setclass

-- ... and the non-collapse: major and augmented are different set classes:
music-major-not-augmented : ¬ (Cmajor ~SC Caug)
music-major-not-augmented = major-augmented-different-setclass

-- ... and the packing-rule witness shared with the engine: the minor seventh (Forte
-- 4-26) normalizes to the compact published prime form [0,3,5,8], not the lex-least
-- [0,2,5,9]. The exact case the Rust prime_form fix pinned, checked by refl:
music-minor-seventh-compact : primeForm Aminor7 ≡ primeForm-4-26
music-minor-seventh-compact = Aminor7-prime
