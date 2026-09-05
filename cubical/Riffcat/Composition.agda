{-# OPTIONS --cubical --no-import-sorts --guardedness #-}

{-
  Riffcat.Composition

  Proofs as documentation for a practical compiler question:

      Can we facet two artifacts independently, then link them, and get the
      same observable result as linking the full artifacts first?

  The answer is not "always" and not "never".  It is exactly this:

      THE LINKER MUST NOT NOTICE WHAT THE FACETS FORGOT.

  In the mathematics, "cannot notice" means that the linking operation respects
  the three equivalence relations involved: the left input facet, the right input
  facet, and the output facet.  When that condition is supplied, the set-quotient
  recursor constructs the faceted linker, and its commuting square computes by
  `refl` on representatives.

  This is also a Salsa-style reuse law.  A cached link result may survive an input
  change exactly when the changed input remains equal at the facet observed by the
  linker.  If the linker needs something the facet forgot, that something must stay
  in the dependency or be saved as a residual.

  The final section gives a tiny, fully concrete port-label story.  A coarse facet
  forgets whether a port is `true` or `false`.  A linker that returns the left port
  plainly notices that forgotten label, so it cannot descend.  Saving the label as
  a residual refines the facet and makes the same linker descend.  No hash and no
  compiler implementation is postulated in that example.
-}

module Riffcat.Composition where

open import Cubical.Foundations.Prelude
open import Cubical.Foundations.HLevels using (isProp×)
open import Cubical.Data.Bool using (Bool ; true ; false ; true≢false)
open import Cubical.Data.Sigma using (_×_ ; _,_ ; fst ; snd)
open import Cubical.Data.Unit using (Unit ; tt)
open import Cubical.Relation.Nullary using (¬_)
open import Cubical.HITs.SetQuotients
  using (_/_ ; eq/ ; squash/ ; rec ; rec2)
  renaming ([_] to ⟦_⟧)

open import Riffcat.Fold
open import Riffcat.Facet

------------------------------------------------------------------------
-- The general construction: a binary operation descends through two input
-- quotients when changing either representative along its relation changes the
-- output only along the output relation.
--
-- Compiler reading:
--   A, B       component representations
--   C          assembled representation
--   RA, RB     what each input facet forgets
--   RC         what the output observer forgets
--   combine    linker / compiler pass / query
--   stable-*   the evidence that the operation cannot notice forgotten input
------------------------------------------------------------------------

module BinaryDescent
  {A B C : Type}
  (RA : A → A → Type)
  (RB : B → B → Type)
  (RC : C → C → Type)
  (combine : A → B → C)
  (stable-left : (a a′ : A) (b : B) → RA a a′ → RC (combine a b) (combine a′ b))
  (stable-right : (a : A) (b b′ : B) → RB b b′ → RC (combine a b) (combine a b′))
  where

  -- The operation on already-faceted inputs.  `rec2` is the whole construction:
  -- the two stability proofs are precisely the permissions it needs.
  descend : (A / RA) → (B / RB) → (C / RC)
  descend = rec2 squash/
    (λ a b → ⟦ combine a b ⟧)
    (λ a a′ b r → eq/ (combine a b) (combine a′ b) (stable-left a a′ b r))
    (λ a b b′ r → eq/ (combine a b) (combine a b′) (stable-right a b b′ r))

  -- THE COMMUTING SQUARE.  Combining representatives and then taking the output
  -- class is definitionally the same as taking the two input classes and using
  -- the descended operation.  It is proved by `refl`, so this is executable
  -- documentation rather than a promised correspondence.
  commutes : (a : A) (b : B)
           → descend ⟦ a ⟧ ⟦ b ⟧ ≡ ⟦ combine a b ⟧
  commutes a b = refl

------------------------------------------------------------------------
-- A compiler/query transformation is the unary form of the same story.
--
-- `stable` says that input equality at `Input` is sufficient to guarantee output
-- equality at `Output`.  The descended function is the cacheable, facet-sensitive
-- query: it consumes an input class rather than an arbitrary representative.
------------------------------------------------------------------------

module QueryTransport
  (Input Output : Dimension)
  (query : Term Input → Term Output)
  (stable : (a b : Term Input)
          → FacetRel Input a b
          → FacetRel Output (query a) (query b))
  where

  queryFacet : Facet Input → Facet Output
  queryFacet = rec (isSetFacet Output)
    (λ a → toFacet (query a))
    (λ a b r → eq/ (query a) (query b) (stable a b r))

  -- Running the full query and then observing its output agrees with running the
  -- query induced on the input facet.
  query-commutes : (a : Term Input)
                 → queryFacet (toFacet a) ≡ toFacet (query a)
  query-commutes a = refl

  -- Cubical payoff: transporting the query result along an input facet path
  -- computes to the supplied stability evidence.
  query-path-computes : (a b : Term Input) (r : FacetRel Input a b)
    → cong queryFacet (eq/ a b r)
      ≡ eq/ (query a) (query b) (stable a b r)
  query-path-computes a b r = refl

------------------------------------------------------------------------
-- The Riffcat linking instance: two possibly different input dimensions can be
-- linked into a third output dimension.  This is the formal version of
--
--     facet after link  =  link on facets after independent facetting.
------------------------------------------------------------------------

module FacetLink
  (Left Right Output : Dimension)
  (link : Term Left → Term Right → Term Output)
  (stable-left : (a a′ : Term Left) (b : Term Right)
    → FacetRel Left a a′
    → FacetRel Output (link a b) (link a′ b))
  (stable-right : (a : Term Left) (b b′ : Term Right)
    → FacetRel Right b b′
    → FacetRel Output (link a b) (link a b′))
  where

  linkFacet : Facet Left → Facet Right → Facet Output
  linkFacet = rec2 (isSetFacet Output)
    (λ a b → toFacet (link a b))
    (λ a a′ b r → eq/ (link a b) (link a′ b) (stable-left a a′ b r))
    (λ a b b′ r → eq/ (link a b) (link a b′) (stable-right a b b′ r))

  link-commutes : (a : Term Left) (b : Term Right)
                → linkFacet (toFacet a) (toFacet b) ≡ toFacet (link a b)
  link-commutes a b = refl

  -- These two equations say more than endpoint equality: moving either input
  -- along a facet path makes the linked output travel along exactly the path
  -- provided by that side's stability proof.
  left-path-computes : (a a′ : Term Left) (b : Term Right)
    (r : FacetRel Left a a′)
    → cong (λ x → linkFacet x (toFacet b)) (eq/ a a′ r)
      ≡ eq/ (link a b) (link a′ b) (stable-left a a′ b r)
  left-path-computes a a′ b r = refl

  right-path-computes : (a : Term Left) (b b′ : Term Right)
    (r : FacetRel Right b b′)
    → cong (linkFacet (toFacet a)) (eq/ b b′ r)
      ≡ eq/ (link a b) (link a b′) (stable-right a b b′ r)
  right-path-computes a b b′ r = refl

------------------------------------------------------------------------
-- The converse: the stability evidence is not decorative.
--
-- If somebody hands us a proposed operation on facet classes and says its square
-- commutes, then the underlying linker MUST respect both input facets.  Otherwise
-- the proposed operation could distinguish two representatives that the quotient
-- has already made equal.
------------------------------------------------------------------------

module FacetLinkNecessary
  (Left Right Output : Dimension)
  (link : Term Left → Term Right → Term Output)
  (candidate : Facet Left → Facet Right → Facet Output)
  (commutes : (a : Term Left) (b : Term Right)
            → candidate (toFacet a) (toFacet b) ≡ toFacet (link a b))
  where

  must-respect-left : (a a′ : Term Left) (b : Term Right)
    → FacetRel Left a a′
    → FacetRel Output (link a b) (link a′ b)
  must-respect-left a a′ b r =
    sameClass→sameAddr (link a b) (link a′ b)
      (sym (commutes a b)
       ∙ cong (λ x → candidate x (toFacet b)) (eq/ a a′ r)
       ∙ commutes a′ b)

  must-respect-right : (a : Term Left) (b b′ : Term Right)
    → FacetRel Right b b′
    → FacetRel Output (link a b) (link a b′)
  must-respect-right a b b′ r =
    sameClass→sameAddr (link a b) (link a b′)
      (sym (commutes a b)
       ∙ cong (candidate (toFacet a)) (eq/ b b′ r)
       ∙ commutes a b′)

------------------------------------------------------------------------
-- Residuals: keep the small piece the coarse facet forgot but a later operation
-- still needs.
--
-- `RememberRel` identifies two artifacts only when they agree both at the facet
-- and in their saved residual.  It is therefore a FINER equivalence: forgetting
-- the residual always maps a remembered class to an ordinary facet class.
--
-- This is a seed of an optic, not a claim that every residual is an optic.  A full
-- optic would additionally specify a backward/update operation and its laws.
------------------------------------------------------------------------

module Remembering
  {ℓ : Level}
  (D : Dimension)
  (Residual : Type ℓ)
  (Residual-set : isSet Residual)
  (save : Term D → Residual)
  where

  RememberRel : Term D → Term D → Type ℓ
  RememberRel a b = FacetRel D a b × (save a ≡ save b)

  RememberRel-prop : (a b : Term D) → isProp (RememberRel a b)
  RememberRel-prop a b =
    isProp× (FacetRel-prop D a b) (Residual-set (save a) (save b))

  RememberedFacet : Type ℓ
  RememberedFacet = Term D / RememberRel

  remember : Term D → RememberedFacet
  remember = ⟦_⟧

  -- Dropping the saved ticket returns to the coarser ordinary facet.
  forgetResidual : RememberedFacet → Facet D
  forgetResidual = rec (isSetFacet D) toFacet
    (λ a b r → eq/ a b (fst r))

  forgetResidual-commutes : (a : Term D)
                          → forgetResidual (remember a) ≡ toFacet a
  forgetResidual-commutes a = refl

  -- A linker may fail to respect the plain facet yet respect facet+residual.
  -- In that case it still has a well-defined OUTPUT at the ordinary facet.
  module ResidualLink
    (link : Term D → Term D → Term D)
    (stable-left : (a a′ b : Term D)
      → RememberRel a a′
      → FacetRel D (link a b) (link a′ b))
    (stable-right : (a b b′ : Term D)
      → RememberRel b b′
      → FacetRel D (link a b) (link a b′))
    where

    linkRemembered : RememberedFacet → RememberedFacet → Facet D
    linkRemembered = rec2 (isSetFacet D)
      (λ a b → toFacet (link a b))
      (λ a a′ b r → eq/ (link a b) (link a′ b) (stable-left a a′ b r))
      (λ a b b′ r → eq/ (link a b) (link a b′) (stable-right a b b′ r))

    residual-link-commutes : (a b : Term D)
      → linkRemembered (remember a) (remember b) ≡ toFacet (link a b)
    residual-link-commutes a b = refl

------------------------------------------------------------------------
-- Concrete port-label story.
--
-- Imagine `Bool` is a two-port interface.  The coarse facet `All` forgets which
-- port we had: every label is equivalent to every other.  `chooseLeft` is a tiny
-- linker that returns the left label.  It can see exactly what `All` forgot.
------------------------------------------------------------------------

module PortExample where

  All : Bool → Bool → Type
  All _ _ = Unit

  chooseLeft : Bool → Bool → Bool
  chooseLeft left _ = left

  -- A concrete failure of the needed congruence: `true` and `false` are equal at
  -- the coarse input facet, but the output labels are not equal.
  coarse-inputs-match : All true false
  coarse-inputs-match = tt

  coarse-link-breaks : ¬ (chooseLeft true false ≡ chooseLeft false false)
  coarse-link-breaks = true≢false

  -- Save the forgotten label as a residual.  Two inputs now match only when their
  -- residual labels match.  The original coarse observation is still present as
  -- the first component; the saved label is the second.
  SameRemembered : Bool → Bool → Type
  SameRemembered x y = All x y × (x ≡ y)

  RememberedPort : Type
  RememberedPort = Bool / SameRemembered

  ExactPort : Type
  ExactPort = Bool / _≡_

  choose-left-stable : (x x′ y : Bool)
    → SameRemembered x x′
    → chooseLeft x y ≡ chooseLeft x′ y
  choose-left-stable x x′ y remembered = snd remembered

  choose-right-stable : (x y y′ : Bool)
    → SameRemembered y y′
    → chooseLeft x y ≡ chooseLeft x y′
  choose-right-stable x y y′ remembered = refl

  module SavedLink = BinaryDescent
    SameRemembered SameRemembered _≡_
    chooseLeft choose-left-stable choose-right-stable

  chooseAfterRemembering : RememberedPort → RememberedPort → ExactPort
  chooseAfterRemembering = SavedLink.descend

  -- With the residual present, "remember independently then link" computes to
  -- "link first then observe".  Again the documentation is checked by `refl`.
  saved-link-commutes : (x y : Bool)
    → chooseAfterRemembering ⟦ x ⟧ ⟦ y ⟧ ≡ ⟦ chooseLeft x y ⟧
  saved-link-commutes = SavedLink.commutes
