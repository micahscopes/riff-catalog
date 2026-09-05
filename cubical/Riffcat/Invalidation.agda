{-# OPTIONS --cubical --no-import-sorts --guardedness #-}

{-
  Riffcat.Invalidation

  A Boolean dirty bit answers only "must I look again?"  A faceted compiler needs
  to carry more useful information: WHICH facet changed, WHY it changed, and what a
  downstream transformation can conclude from that change.

  This module models invalidation as algebraic data indexed by Riffcat's dimensions.
  Every dimension chooses its own change-evidence type and merge operation.  A whole
  change set is the dependent product of those carriers:

      ChangeSet A = (dimension : Dimension) -> Delta A dimension

  An invalidation transport maps change sets across a compiler pass.  Its two laws
  say that it preserves "nothing changed" and merging.  Consequently transports
  compose: a pipeline can move, discard, enrich, or combine facet effects without
  collapsing them to one global dirty flag.

  `InvalidationPolicy` is the deliberate place for richer logic.  It interprets the
  algebraic evidence as keep/revalidate/recompute/retract, and must justify every
  `keep`.  In an executable persistent system, a change payload that requests a
  complex check should contain a checker address plus canonical inputs rather than
  an opaque closure.  Agda functions here specify the meaning, not the wire format.
-}

module Riffcat.Invalidation where

open import Cubical.Foundations.Prelude
open import Cubical.Data.List using (List ; [] ; _∷_ ; _++_)
open import Cubical.Data.List.Properties using (++-unit-r ; ++-assoc)
open import Cubical.Relation.Nullary using (¬_)

open import Riffcat.Fold using
  (Dimension ; Structure ; Names ; Constants ; Types ; TraceEvents)

------------------------------------------------------------------------
-- A separate change algebra for every facet dimension.
------------------------------------------------------------------------

record InvalidationAlgebra : Type₁ where
  field
    Delta            : Dimension → Type
    zero             : (dimension : Dimension) → Delta dimension
    merge            : (dimension : Dimension)
                     → Delta dimension → Delta dimension → Delta dimension
    merge-zero-left  : (dimension : Dimension) (change : Delta dimension)
                     → merge dimension (zero dimension) change ≡ change
    merge-zero-right : (dimension : Dimension) (change : Delta dimension)
                     → merge dimension change (zero dimension) ≡ change
    merge-assoc      : (dimension : Dimension) (x y z : Delta dimension)
                     → merge dimension (merge dimension x y) z
                     ≡ merge dimension x (merge dimension y z)

open InvalidationAlgebra public

ChangeSet : InvalidationAlgebra → Type
ChangeSet algebra = (dimension : Dimension) → Delta algebra dimension

no-change : (algebra : InvalidationAlgebra) → ChangeSet algebra
no-change algebra dimension = zero algebra dimension

merge-changes : (algebra : InvalidationAlgebra)
  → ChangeSet algebra → ChangeSet algebra → ChangeSet algebra
merge-changes algebra left right dimension =
  merge algebra dimension (left dimension) (right dimension)

merge-no-change-left : (algebra : InvalidationAlgebra)
  (changes : ChangeSet algebra)
  → merge-changes algebra (no-change algebra) changes ≡ changes
merge-no-change-left algebra changes =
  funExt (λ dimension → merge-zero-left algebra dimension (changes dimension))

merge-no-change-right : (algebra : InvalidationAlgebra)
  (changes : ChangeSet algebra)
  → merge-changes algebra changes (no-change algebra) ≡ changes
merge-no-change-right algebra changes =
  funExt (λ dimension → merge-zero-right algebra dimension (changes dimension))

merge-changes-assoc : (algebra : InvalidationAlgebra)
  (x y z : ChangeSet algebra)
  → merge-changes algebra (merge-changes algebra x y) z
  ≡ merge-changes algebra x (merge-changes algebra y z)
merge-changes-assoc algebra x y z =
  funExt (λ dimension →
    merge-assoc algebra dimension (x dimension) (y dimension) (z dimension))

-- These predicates retain the evidence's dimension.  A caller can ask about one
-- facet without turning the complete change set into a Boolean.
UnchangedAt : (algebra : InvalidationAlgebra)
  → ChangeSet algebra → Dimension → Type
UnchangedAt algebra changes dimension =
  changes dimension ≡ zero algebra dimension

ChangedAt : (algebra : InvalidationAlgebra)
  → ChangeSet algebra → Dimension → Type
ChangedAt algebra changes dimension = ¬ (UnchangedAt algebra changes dimension)

AllUnchanged : (algebra : InvalidationAlgebra) → ChangeSet algebra → Type
AllUnchanged algebra changes =
  (dimension : Dimension) → UnchangedAt algebra changes dimension

all-unchanged-is-no-change : (algebra : InvalidationAlgebra)
  (changes : ChangeSet algebra)
  → AllUnchanged algebra changes
  → changes ≡ no-change algebra
all-unchanged-is-no-change algebra changes all-unchanged =
  funExt all-unchanged

-- The HoTT-shaped operation is ordinary dependent transport: when two facet
-- indices are identified, their change evidence moves along that path.  Path
-- induction proves that the algebraic structure moves coherently as well.
reindex-change : (algebra : InvalidationAlgebra)
  {source target : Dimension}
  → source ≡ target
  → Delta algebra source
  → Delta algebra target
reindex-change algebra path = subst (Delta algebra) path

reindex-zero : (algebra : InvalidationAlgebra)
  {source target : Dimension}
  (path : source ≡ target)
  → reindex-change algebra path (zero algebra source) ≡ zero algebra target
reindex-zero algebra {source = source} =
  J (λ target path →
      reindex-change algebra path (zero algebra source) ≡ zero algebra target)
    (substRefl {B = Delta algebra} (zero algebra source))

reindex-merge : (algebra : InvalidationAlgebra)
  {source target : Dimension}
  (path : source ≡ target)
  (left right : Delta algebra source)
  → reindex-change algebra path (merge algebra source left right)
  ≡ merge algebra target
      (reindex-change algebra path left)
      (reindex-change algebra path right)
reindex-merge algebra {source = source} path left right =
  J (λ target path →
      reindex-change algebra path (merge algebra source left right)
      ≡ merge algebra target
          (reindex-change algebra path left)
          (reindex-change algebra path right))
    (substRefl {B = Delta algebra} (merge algebra source left right)
     ∙ sym (cong₂ (merge algebra source)
         (substRefl {B = Delta algebra} left)
         (substRefl {B = Delta algebra} right)))
    path

-- Associativity is enough when changes are accumulated in one known order.  A
-- replicated/concurrent summary needs the stronger join-like laws below: order and
-- duplicate delivery must not affect the merged invalidation evidence.
record ConcurrentInvalidationAlgebra : Type₁ where
  field
    algebra          : InvalidationAlgebra
    merge-comm       : (dimension : Dimension)
      (left right : Delta algebra dimension)
      → merge algebra dimension left right
      ≡ merge algebra dimension right left
    merge-idempotent : (dimension : Dimension)
      (change : Delta algebra dimension)
      → merge algebra dimension change change ≡ change

------------------------------------------------------------------------
-- Transport facet-indexed invalidation evidence through a compiler pass.
--
-- `carry` is allowed to move evidence between dimensions.  For example, changing
-- a name can alter a trace facet while leaving a nameless machine-code facet clean.
-- The homomorphism laws are the compositional boundary.
------------------------------------------------------------------------

record InvalidationTransport
  (Source Target : InvalidationAlgebra) : Type where
  field
    carry       : ChangeSet Source → ChangeSet Target
    carry-zero  : carry (no-change Source) ≡ no-change Target
    carry-merge : (left right : ChangeSet Source)
      → carry (merge-changes Source left right)
      ≡ merge-changes Target (carry left) (carry right)

open InvalidationTransport public

identityTransport : (algebra : InvalidationAlgebra)
  → InvalidationTransport algebra algebra
carry (identityTransport algebra) changes = changes
carry-zero (identityTransport algebra) = refl
carry-merge (identityTransport algebra) left right = refl

composeTransport :
  {A B C : InvalidationAlgebra}
  → InvalidationTransport A B
  → InvalidationTransport B C
  → InvalidationTransport A C
carry (composeTransport first next) changes = carry next (carry first changes)
carry-zero (composeTransport first next) =
  cong (carry next) (carry-zero first) ∙ carry-zero next
carry-merge (composeTransport first next) left right =
  cong (carry next) (carry-merge first left right)
  ∙ carry-merge next (carry first left) (carry first right)

-- The algebra laws alone do NOT say that a transport describes its compiler pass.
-- In particular, the useless map that sends every input change to `no-change` can
-- satisfy them.  `TracksTransformation` is the semantic boundary: computing an
-- output delta directly must agree with transporting the input delta.
record TracksTransformation
  {Input Output : Type}
  (Source Target : InvalidationAlgebra)
  (source-delta : Input → Input → ChangeSet Source)
  (target-delta : Output → Output → ChangeSet Target)
  (transform : Input → Output)
  (transport : InvalidationTransport Source Target)
  : Type where
  field
    transport-agrees : (old new : Input)
      → carry transport (source-delta old new)
      ≡ target-delta (transform old) (transform new)

open TracksTransformation public

-- Exact tracking composes with the transformations.  Once every stage earns its
-- semantic transport law, the complete pass chain needs no new global argument.
tracks-transformation-compose :
  {Input Middle Output : Type}
  {A B C : InvalidationAlgebra}
  {delta-A : Input → Input → ChangeSet A}
  {delta-B : Middle → Middle → ChangeSet B}
  {delta-C : Output → Output → ChangeSet C}
  {first : Input → Middle}
  {next : Middle → Output}
  {first-transport : InvalidationTransport A B}
  {next-transport : InvalidationTransport B C}
  → TracksTransformation A B delta-A delta-B first first-transport
  → TracksTransformation B C delta-B delta-C next next-transport
  → TracksTransformation A C delta-A delta-C
      (λ input → next (first input))
      (composeTransport first-transport next-transport)
transport-agrees
  (tracks-transformation-compose
    {first = first} {next = next}
    {first-transport = first-transport}
    {next-transport = next-transport}
    first-tracks next-tracks)
  old new =
    cong (carry next-transport) (transport-agrees first-tracks old new)
    ∙ transport-agrees next-tracks (first old) (first new)

------------------------------------------------------------------------
-- Rich invalidation logic lives in an interpreter with a safety boundary.
------------------------------------------------------------------------

data CacheAction : Type where
  keep revalidate recompute retract : CacheAction

record InvalidationPolicy
  (algebra : InvalidationAlgebra)
  (Context Cached : Type)
  (ValidBefore ValidAfter : Context → Cached → Type)
  : Type₁ where
  field
    decide : Context → Cached → ChangeSet algebra → CacheAction

    -- Other actions are conservative.  `keep` is the one answer that must prove
    -- that skipping work preserves the cache's warrant.
    keep-sound : (context : Context) (cached : Cached)
      (changes : ChangeSet algebra)
      → decide context cached changes ≡ keep
      → ValidBefore context cached
      → ValidAfter context cached

------------------------------------------------------------------------
-- A useful concrete algebra: retain every reason, per dimension.
--
-- Lists are intentionally simple rather than canonical.  A production addressable
-- representation could use normalized sets or Merkle collections.  The point proved
-- here is that accumulating reasons forms a lawful change algebra.
------------------------------------------------------------------------

ReasonBag : (Reason : Dimension → Type) → InvalidationAlgebra
Delta (ReasonBag Reason) dimension = List (Reason dimension)
zero (ReasonBag Reason) dimension = []
merge (ReasonBag Reason) dimension = _++_
merge-zero-left (ReasonBag Reason) dimension change = refl
merge-zero-right (ReasonBag Reason) dimension change = ++-unit-r change
merge-assoc (ReasonBag Reason) dimension = ++-assoc

data CompilerReason : Dimension → Type where
  renamed             : CompilerReason Names
  syntax-shape-changed : CompilerReason Structure
  constant-changed    : CompilerReason Constants
  type-fact-changed   : CompilerReason Types
  trace-origin-changed : CompilerReason TraceEvents

CompilerInvalidation : InvalidationAlgebra
CompilerInvalidation = ReasonBag CompilerReason

rename-only : ChangeSet CompilerInvalidation
rename-only Names = renamed ∷ []
rename-only Structure = []
rename-only Constants = []
rename-only Types = []
rename-only TraceEvents = []

rename-invalidates-names :
  rename-only Names ≡ renamed ∷ []
rename-invalidates-names = refl

rename-keeps-structure : UnchangedAt CompilerInvalidation rename-only Structure
rename-keeps-structure = refl

rename-keeps-types : UnchangedAt CompilerInvalidation rename-only Types
rename-keeps-types = refl
