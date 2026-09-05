{-# OPTIONS --cubical --no-import-sorts --guardedness #-}

{-
  Riffcat.LinkingKinds

  A small map of the kinds of composition we may have, and what each kind lets us
  conclude.  The hierarchy is intentionally plain:

    preserving
      Equal-enough inputs give equal-enough outputs.
      Conclusion: the operation is well defined on facets; cached reuse is safe.

    reflecting
      Equal-enough outputs imply equal-enough inputs.
      Conclusion: the output did not hide any distinction in the chosen input view.

    same kernel = preserving + reflecting
      The input observation and output observation divide examples into exactly the
      same classes.  Conclusion: this is an exact change key for that operation.

    lossless with a residual
      The output plus a saved remainder rebuilds the original.
      Conclusion: exact reverse travel is possible when the residual is retained.

    approximate recovery
      We have a guesser but no round-trip law.
      Conclusion: we have a candidate reconstruction, not a proof of inversion.

  None of these words implies all the others.  In particular, preservation is not
  invertibility, and an exact change key does not necessarily reconstruct the full
  source artifact.

  The second half applies the map to Riffcat documents made of named, independently
  addressed parts.  It proves links between selected parts and serial composition of
  queries.  These are the first laws needed for bundles without choosing a final
  graph, cache, or compiler architecture.
-}

module Riffcat.LinkingKinds where

open import Cubical.Foundations.Prelude
open import Cubical.Data.Sigma using (_×_ ; _,_)
open import Cubical.HITs.SetQuotients using (_/_ ; eq/ ; rec)
  renaming ([_] to ⟦_⟧)

open import Riffcat.Fold
open import Riffcat.Facet

------------------------------------------------------------------------
-- Relations describe what an observer considers equal.  They may be Riffcat
-- facet equality, ordinary equality, a type equivalence, or something else.
------------------------------------------------------------------------

Preserves : {A B : Type}
  → (A → A → Type) → (B → B → Type) → (A → B) → Type
Preserves RA RB f = (x y : _) → RA x y → RB (f x) (f y)

Reflects : {A B : Type}
  → (A → A → Type) → (B → B → Type) → (A → B) → Type
Reflects RA RB f = (x y : _) → RB (f x) (f y) → RA x y

SameKernel : {A B : Type}
  → (A → A → Type) → (B → B → Type) → (A → B) → Type
SameKernel RA RB f = Preserves RA RB f × Reflects RA RB f

preserves-compose :
  {A B C : Type}
  {RA : A → A → Type} {RB : B → B → Type} {RC : C → C → Type}
  {f : A → B} {g : B → C}
  → Preserves RA RB f
  → Preserves RB RC g
  → Preserves RA RC (λ x → g (f x))
preserves-compose {f = f} {g = g} f-preserves g-preserves x y same =
  g-preserves (f x) (f y) (f-preserves x y same)

reflects-compose :
  {A B C : Type}
  {RA : A → A → Type} {RB : B → B → Type} {RC : C → C → Type}
  {f : A → B} {g : B → C}
  → Reflects RA RB f
  → Reflects RB RC g
  → Reflects RA RC (λ x → g (f x))
reflects-compose {f = f} {g = g} f-reflects g-reflects x y same =
  f-reflects x y (g-reflects (f x) (f y) same)

same-kernel-compose :
  {A B C : Type}
  {RA : A → A → Type} {RB : B → B → Type} {RC : C → C → Type}
  {f : A → B} {g : B → C}
  → SameKernel RA RB f
  → SameKernel RB RC g
  → SameKernel RA RC (λ x → g (f x))
same-kernel-compose
  {RA = RA} {RB = RB} {RC = RC} {f = f} {g = g}
  f-exact g-exact =
    preserves-compose
      {RA = RA} {RB = RB} {RC = RC} {f = f} {g = g}
      (fst f-exact) (fst g-exact)
  , reflects-compose
      {RA = RA} {RB = RB} {RC = RC} {f = f} {g = g}
      (snd f-exact) (snd g-exact)

------------------------------------------------------------------------
-- A residual is the remembered part of a lossy-looking transformation.  This
-- structure is weaker than a full optic: it says how to rebuild, but says nothing
-- yet about lawful updates to the focused output.
------------------------------------------------------------------------

record LosslessWithResidual (Source View Residual : Type) : Type where
  field
    forward   : Source → View
    save      : Source → Residual
    rebuild   : View → Residual → Source
    roundtrip : (source : Source)
              → rebuild (forward source) (save source) ≡ source

module ResidualEncoding
  {Source View Residual : Type}
  (witness : LosslessWithResidual Source View Residual)
  where

  open LosslessWithResidual witness

  encode : Source → View × Residual
  encode source = forward source , save source

  decode : View × Residual → Source
  decode pair = rebuild (fst pair) (snd pair)

  decode-encode : (source : Source) → decode (encode source) ≡ source
  decode-encode = roundtrip

-- No equality law is intentionally present.  Machine learning may implement the
-- guess, but its result remains an estimate until a separate checker supplies one.
record ApproximateRecovery (Source View : Type) : Type where
  field
    forward : Source → View
    guess   : View → Source

------------------------------------------------------------------------
-- Riffcat bundles: a document is a family of named parts.  The dependent dimension
-- permits, for example, one part to expose Names and another to expose Types.
------------------------------------------------------------------------

module Parts
  (Part : Type)
  (partDimension : Part → Dimension)
  (Uses : Part → Type)
  where

  Bundle : Type
  Bundle = (part : Part) → Term (partDimension part)

  addresses : Bundle → Part → Digest
  addresses bundle part = addr (bundle part)

  -- Two documents are equal for this consumer exactly when every part it reads has
  -- the same selected facet address.  Parts it does not read impose no obligation.
  SameObserved : Bundle → Bundle → Type
  SameObserved old new =
    (part : Part) → Uses part
    → FacetRel (partDimension part) (old part) (new part)

  Observation : Type
  Observation = Bundle / SameObserved

  observe : Bundle → Observation
  observe = ⟦_⟧

  module Query
    (Output : Dimension)
    (build : Bundle → Term Output)
    (stable : Preserves SameObserved (FacetRel Output) build)
    where

    onObservation : Observation → Facet Output
    onObservation = rec (isSetFacet Output)
      (λ bundle → toFacet (build bundle))
      (λ old new same →
        eq/ (build old) (build new) (stable old new same))

    -- "Observe then build" and "build then facet" are the same route.
    commutes : (bundle : Bundle)
      → onObservation (observe bundle) ≡ toFacet (build bundle)
    commutes bundle = refl

    -- Preservation is enough for safe reuse.  Supplying reflection upgrades this
    -- consumer observation to an exact change key for this particular build.
    exact-key : Reflects SameObserved (FacetRel Output) build
      → SameKernel SameObserved (FacetRel Output) build
    exact-key reflects = stable , reflects

  ----------------------------------------------------------------------
  -- Linking two selected parts.  The proof mentions only the two selected ports.
  -- Thus unrelated changes are harmless by construction, while each used part has
  -- an explicit facet-preservation obligation.
  ----------------------------------------------------------------------

  module SelectedLink
    (Output : Dimension)
    (source target : Part)
    (source-used : Uses source)
    (target-used : Uses target)
    (link : Term (partDimension source)
         → Term (partDimension target)
         → Term Output)
    (source-stable :
      (a a′ : Term (partDimension source))
      (b : Term (partDimension target))
      → FacetRel (partDimension source) a a′
      → FacetRel Output (link a b) (link a′ b))
    (target-stable :
      (a : Term (partDimension source))
      (b b′ : Term (partDimension target))
      → FacetRel (partDimension target) b b′
      → FacetRel Output (link a b) (link a b′))
    where

    assemble : Bundle → Term Output
    assemble bundle = link (bundle source) (bundle target)

    assemble-preserves : Preserves SameObserved (FacetRel Output) assemble
    assemble-preserves old new same =
        source-stable
          (old source) (new source) (old target)
          (same source source-used)
      ∙ target-stable
          (new source) (old target) (new target)
          (same target target-used)

    module Linked = Query Output assemble assemble-preserves

    linking-commutes : (bundle : Bundle)
      → Linked.onObservation (observe bundle) ≡ toFacet (assemble bundle)
    linking-commutes = Linked.commutes

  ----------------------------------------------------------------------
  -- Serial query composition.  Preservation always composes.  Exactness composes
  -- only when both stages reflect as well as preserve.
  ----------------------------------------------------------------------

  module Serial
    (Middle Output : Dimension)
    (first : Bundle → Term Middle)
    (next : Term Middle → Term Output)
    (first-preserves : Preserves SameObserved (FacetRel Middle) first)
    (next-preserves : Preserves (FacetRel Middle) (FacetRel Output) next)
    where

    composite : Bundle → Term Output
    composite bundle = next (first bundle)

    composite-preserves : Preserves SameObserved (FacetRel Output) composite
    composite-preserves = preserves-compose
      {RA = SameObserved}
      {RB = FacetRel Middle}
      {RC = FacetRel Output}
      {f = first}
      {g = next}
      first-preserves next-preserves

    module Composed = Query Output composite composite-preserves

    composition-commutes : (bundle : Bundle)
      → Composed.onObservation (observe bundle) ≡ toFacet (composite bundle)
    composition-commutes = Composed.commutes
