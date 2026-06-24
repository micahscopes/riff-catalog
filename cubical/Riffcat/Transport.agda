{-# OPTIONS --cubical --no-import-sorts --guardedness #-}

{-
  Riffcat.Transport

  Deliverable 3: THE CUBICAL PAYOFF, demonstrated and checked.

  Transport along a quotient path COMPUTES. In Cubical Agda, `eq/ a b r : [ a ] ≡ [ b ]`
  is a genuine path, and the recursor's defining clause
      rec set f feq (eq/ a b r i) = feq a b r i
  is DEFINITIONAL. So applying an anchored fact to the quotient path reduces, by the
  cubical operational semantics, to the supplied respect-proof: a goal provable by
  `refl`.

  This is the heart of why cubical and not Lean. In Lean 4, `Quot.sound : R a b ->
  Quot.mk a = Quot.mk b` is an OPAQUE proof: there is no reduction rule that lets you
  transport along it and watch it simplify; `cong (Quot.lift f h) (Quot.sound r)` is
  NOT definitionally `h a b r`. Here it is, and we check it twice:

  - `anchor-rides` (abstract): over the abstract hash, applying the anchored fact to
    a facet path reduces to the respect-proof, by refl. This is the general law.
  - the `Concrete` module: a fully concrete instance (parity on ℕ) where the anchored
    fact applied along an actual quotient path reduces all the way to `refl : true ≡
    true`, and the endpoints reduce to actual booleans. Nothing is postulated here;
    it runs.

  Honest scope (per the design note, section 2.3): for riffcat the practical force is
  mild because the address is an EFFECTIVE quotient and the engine compares bytes; the
  computing-transport matters for the FORMALIZATION, where anchored-fact proofs compose
  by reduction instead of by a chain of opaque rewrites. We do not oversell it: it is a
  statement about the two type theories, checked here by refl, not a new engine feature.
-}

module Riffcat.Transport where

open import Cubical.Foundations.Prelude
open import Cubical.HITs.SetQuotients
open import Cubical.Data.Nat using (ℕ ; zero ; suc ; isSetℕ)
open import Cubical.Data.Bool using (Bool ; true ; false ; isSetBool)

open import Riffcat.Fold
open import Riffcat.Facet

private
  variable
    ℓ : Level

------------------------------------------------------------------------
-- The general law, over the ABSTRACT hash: an anchored fact applied to a facet path
-- reduces to its respect-proof. Holds by refl (it is the recursor's defining clause).
------------------------------------------------------------------------

module _ {B : Type ℓ} (Bset : isSet B) (D : Dimension)
         (P : Term D → B)
         (resp : (a b : Term D) → FacetRel D a b → P a ≡ P b)
         (a b : Term D) (r : FacetRel D a b) where

  -- The anchored fact (the SetQuotients recursor on P).
  factOn : Facet D → B
  factOn = rec Bset P resp

  -- THE computing transport: riding the fact along the facet path [ a ] ≡ [ b ]
  -- reduces, definitionally, to the supplied respect-proof. This is the cubical
  -- advantage over Lean's inert Quot.sound, checked by refl.
  anchor-rides : cong factOn (eq/ a b r) ≡ resp a b r
  anchor-rides = refl

  -- Equivalently: the path the fact takes between the two representatives is exactly
  -- resp a b r (the same statement, named for the anchors chapter).
  fact-travels : PathP (λ i → B) (factOn (toFacet a)) (factOn (toFacet b))
  fact-travels = cong factOn (eq/ a b r)

  fact-travels-computes : fact-travels ≡ resp a b r
  fact-travels-computes = refl

-- The normalization itself (addrFacet) rides a facet path to the (refl) equality of
-- addresses, because addr respects the facet definitionally. By refl.
addr-rides : (D : Dimension) (a b : Term D) (r : FacetRel D a b)
           → cong addrFacet (eq/ a b r) ≡ r
addr-rides D a b r = refl

------------------------------------------------------------------------
-- A FULLY CONCRETE computing instance (no abstract hash, no postulate): parity on ℕ.
-- This grounds the abstract law in something that reduces to actual booleans, so the
-- "computes" claim is not just a statement about an abstract path.
------------------------------------------------------------------------

module Concrete where

  -- parity as a fact on ℕ.
  parity : ℕ → Bool
  parity zero          = true
  parity (suc zero)    = false
  parity (suc (suc n)) = parity n

  -- the parity equivalence and its quotient.
  SameParity : ℕ → ℕ → Type
  SameParity x y = parity x ≡ parity y

  ParityQuot : Type
  ParityQuot = ℕ / SameParity

  -- the fact descends because it respects the relation (the relation IS its kernel).
  parityFact : ParityQuot → Bool
  parityFact = rec isSetBool parity (λ a b r → r)

  -- a concrete quotient path between 0 and 2 (same parity, witnessed by refl).
  n0 : ℕ
  n0 = 0

  n2 : ℕ
  n2 = 2

  r0-2 : SameParity n0 n2
  r0-2 = refl

  -- class brackets at this specific quotient (pins the relation for inference).
  cls : ℕ → ParityQuot
  cls = [_]

  -- THE concrete computing example: riding the parity fact along the path [ 0 ] ≡
  -- [ 2 ] reduces all the way to refl : true ≡ true. Provable by refl.
  parity-rides : cong parityFact (eq/ {R = SameParity} n0 n2 r0-2) ≡ refl
  parity-rides = refl

  -- the endpoints reduce to actual booleans (the fact genuinely runs).
  parity-at-0 : parityFact (cls n0) ≡ true
  parity-at-0 = refl

  parity-at-2 : parityFact (cls n2) ≡ true
  parity-at-2 = refl

  -- and 0 and 2 really are the same class (the path is inhabited).
  zero-two-same-class : cls n0 ≡ cls n2
  zero-two-same-class = eq/ {R = SameParity} n0 n2 r0-2
