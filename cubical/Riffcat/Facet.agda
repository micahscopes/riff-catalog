{-# OPTIONS --cubical --no-import-sorts --guardedness #-}

{-
  Riffcat.Facet

  Deliverable 2: the facet as a set-quotient HIT.

  - Define the facet congruence ~_F on the structure and form `A / ~_F` via
    Cubical.HITs.SetQuotients.
  - The content address is the quotient map; where there is a normalization, the
    quotient is effective / split (decide a class by computing and comparing
    addresses, never touching the HIT path constructor).
  - ANCHORS THEOREM as the SetQuotients recursion principle: a fact out of `A / ~_F`
    is exactly a function on A that respects ~_F (rec / elim). This IS the universal
    property; "respects ~_F" = "~_F is contained in ker(fact)".
  - FACET REFINEMENT: a finer facet quotient surjects onto a coarser one (the
    quotient-of-a-quotient factorization).

  A facet (riff-catalog-core/src/hash/reference.rs::Facet) is "equal at F" = equality
  of the facet address. We model the congruence as the kernel of the address map at a
  dimension: a ~_F b iff addr a == addr b. This is the EXACT facet relation: equal
  per-dimension digests (invariant I9: per-dimension digests do not depend on which
  other dimensions were requested).
-}

module Riffcat.Facet where

open import Cubical.Foundations.Prelude
open import Cubical.Functions.Surjection using (isSurjection ; _↠_)
open import Cubical.HITs.PropositionalTruncation using (∣_∣₁ ; squash₁)
open import Cubical.HITs.SetQuotients
  renaming ([_] to ⟦_⟧)
open import Cubical.Relation.Binary.Base using (module BinaryRelation)
open BinaryRelation using (isEquivRel ; equivRel)

open import Riffcat.Fold

private
  variable
    ℓ : Level

------------------------------------------------------------------------
-- The facet congruence at a dimension D: "equal at facet D" is equality of the
-- per-dimension content address. This is a propositional equivalence relation
-- (it is the kernel of `addr`, pulled back from the hSet equality on Digest).
------------------------------------------------------------------------

-- The facet relation at a dimension D: "equal at facet D" = equal addresses.
FacetRel : (D : Dimension) → Term D → Term D → Type
FacetRel D a b = addr a ≡ addr b

-- It is propositionally valued (Digest is an hSet, so its path space is an hProp).
FacetRel-prop : (D : Dimension) (a b : Term D) → isProp (FacetRel D a b)
FacetRel-prop D a b = isSetDigest (addr a) (addr b)

-- It is an equivalence relation.
FacetRel-equiv : (D : Dimension) → isEquivRel (FacetRel D)
FacetRel-equiv D = equivRel
  (λ a → refl)
  (λ a b p → sym p)
  (λ a b c p q → p ∙ q)

------------------------------------------------------------------------
-- The facet quotient `A / ~_F` and the content address as the quotient map.
------------------------------------------------------------------------

Facet : (D : Dimension) → Type
Facet D = Term D / FacetRel D

-- The quotient map: the content address as a map onto the set of classes.
toFacet : {D : Dimension} → Term D → Facet D
toFacet = ⟦_⟧

-- The facet quotient is a set (0-truncated, by the HIT's squash/). riffcat lives at
-- h-level 0; this confirms it.
isSetFacet : (D : Dimension) → isSet (Facet D)
isSetFacet D = squash/

------------------------------------------------------------------------
-- THE ANCHORS THEOREM = the SetQuotients recursion principle.
--
-- A fact P : Term D -> B (B an hSet) descends to a fact on the quotient Facet D
-- EXACTLY when it respects ~_F (when ~_F is contained in ker P). The descended fact
-- agrees with P on representatives, definitionally.
------------------------------------------------------------------------

module _ {B : Type ℓ} (Bset : isSet B) (D : Dimension) where

  -- "respects the facet": ~_F is contained in ker P.
  Respects : (Term D → B) → Type ℓ
  Respects P = (a b : Term D) → FacetRel D a b → P a ≡ P b

  -- The anchor: the unique descent of a facet-respecting fact onto the quotient.
  anchor : (P : Term D → B) → Respects P → Facet D → B
  anchor P resp = rec Bset P resp

  -- Computation rule: the anchored fact agrees with P on every representative,
  -- definitionally (holds by refl). This is what "a fact riding the address" means,
  -- and it COMPUTES.
  anchor-β : (P : Term D → B) (resp : Respects P) (a : Term D)
           → anchor P resp (toFacet a) ≡ P a
  anchor-β P resp a = refl

------------------------------------------------------------------------
-- The address is an EFFECTIVE / SPLIT quotient via the normalization.
--
-- `addr` factors through the quotient (it respects ~_F by definition: ~_F IS the
-- kernel of addr). The induced map `addrFacet : Facet D -> Digest` is the
-- normalization that names each class. Because Digest has decidable equality, you
-- decide class membership by computing and comparing addresses, never evaluating the
-- HIT path constructor. The clean statement `Facet D ~= Image addr` (the quotient is
-- the set of normal-form addresses) is the effectivity; here we record the splitting
-- map and its β-rule.
------------------------------------------------------------------------

-- addr respects the facet trivially (~_F is its kernel).
addr-respects : (D : Dimension) → (a b : Term D) → FacetRel D a b → addr a ≡ addr b
addr-respects D a b p = p

-- The normalization map out of the quotient.
addrFacet : {D : Dimension} → Facet D → Digest
addrFacet {D} = rec isSetDigest addr (addr-respects D)

-- It is a section-style name: it agrees with addr on representatives, definitionally.
addrFacet-β : {D : Dimension} (a : Term D) → addrFacet (toFacet a) ≡ addr a
addrFacet-β a = refl

-- Two classes are equal as soon as their normal-form addresses agree. This is the
-- "decide membership by comparing names" half of effectivity, and it follows from
-- the quotient's own eq/ constructor.
sameAddr→sameClass : {D : Dimension} (a b : Term D)
                   → addr a ≡ addr b → toFacet a ≡ toFacet b
sameAddr→sameClass {D} a b p = eq/ a b p

-- Conversely the classes determine the address (addrFacet is well defined), so
-- "same class" and "same address" coincide: an effective quotient.
sameClass→sameAddr : {D : Dimension} (a b : Term D)
                   → toFacet a ≡ toFacet b → addr a ≡ addr b
sameClass→sameAddr {D} a b q = cong addrFacet q

------------------------------------------------------------------------
-- FACET REFINEMENT: a finer facet surjects onto a coarser one.
--
-- "Finer" means: whenever a and b are identified at the finer facet, they are
-- identified at the coarser facet too (finer => coarser, monotonicity of forgetting).
-- Given a refinement witness, the finer quotient maps ONTO the coarser quotient, and
-- that map is surjective: the quotient-of-a-quotient factorization.
--
-- We state refinement generically over two relations R (finer) and S (coarser) on
-- the same carrier A, with R contained in S.
------------------------------------------------------------------------

module Refinement {A : Type ℓ}
                  (R S : A → A → Type)
                  (R⊆S : (a b : A) → R a b → S a b) where

  -- The factorization map: send a finer class to its coarser class. Well defined
  -- because R-related points are S-related, so they have the same coarser class.
  coarsen : (A / R) → (A / S)
  coarsen = rec squash/ ⟦_⟧ (λ a b r → eq/ a b (R⊆S a b r))

  -- It agrees with the obvious thing on representatives, definitionally.
  coarsen-β : (a : A) → coarsen ⟦ a ⟧ ≡ ⟦ a ⟧
  coarsen-β a = refl

  -- The factorization is SURJECTIVE: every coarser class is hit. This is the
  -- "finer quotient surjects onto coarser" theorem. Proved by quotient induction
  -- into a proposition (each fiber-inhabitation is propositionally truncated, hence a
  -- prop), using that the coarser class ⟦ a ⟧ has the explicit preimage ⟦ a ⟧.
  coarsen-surjective : isSurjection coarsen
  coarsen-surjective =
    elimProp (λ x → squash₁) (λ a → ∣ ⟦ a ⟧ , refl ∣₁)

  -- Packaged as a surjection.
  coarsen↠ : (A / R) ↠ (A / S)
  coarsen↠ = coarsen , coarsen-surjective

-- The substantive instance of refinement (two genuinely different relations on one
-- carrier, finer surjecting onto coarser) is the pitch-class tower in Riffcat.Music:
-- transposition-only is finer than transposition-and-inversion, and the set-class
-- quotient is a coarsening of the transposition-class quotient. See
-- `Riffcat.Music.setClassFromTransClass↠`.
