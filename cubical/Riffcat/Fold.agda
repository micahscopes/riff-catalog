{-# OPTIONS --cubical --no-import-sorts --guardedness #-}

{-
  Riffcat.Fold

  Deliverable 1: a small dimension-tagged structure as an inductive type, with a
  catamorphism fold (the acyclic per-dimension digest as the unique algebra map),
  over an ABSTRACT hash / codomain.

  This mirrors riffcat's core abstractly. Per the established hash-split, we do NOT
  implement BLAKE3 here: the Digest type and the hashing algebra are abstract. The
  Lean port (riff-catalog/lean/) remains the byte-oracle. The cubical witness proves
  the quotient / transport / h-level laws over the abstract hash.

  The engine equation we model (riff-catalog-core/src/hash/acyclic.rs, node_tree_digest):

      d_D(parent) = alpha_D( local_D(parent) , [ d_D(child_1) , ... , d_D(child_k) ] )

  where on the Structure dimension each child enters framed by its (ordinal, label),
  and on the other dimensions the frame is trivial. The fold memoizes in the engine
  (hash-consing); memoization is sound precisely because a catamorphism is a function
  of the value, which is exactly what `fold` below is.
-}

module Riffcat.Fold where

open import Cubical.Foundations.Prelude
open import Cubical.Data.Sigma using (_×_ ; _,_ ; fst ; snd)
open import Cubical.Data.Nat using (ℕ ; isSetℕ)
open import Cubical.Data.List using (List ; [] ; _∷_ ; map)

private
  variable
    ℓ : Level

------------------------------------------------------------------------
-- The closed dimension enum (riff-catalog-schema/src/dimension.rs: a closed
-- 5-enum). We model the closed set; only the Structure dimension carries an
-- ordinal+label frame on its children, the rest carry a trivial frame.
------------------------------------------------------------------------

data Dimension : Type where
  Structure  : Dimension
  Names      : Dimension
  Constants  : Dimension
  Types      : Dimension
  TraceEvents : Dimension

-- A label is modeled as a natural number (an interned name id). A local payload
-- (the per-node, per-dimension local digest input: the `kind` and the D-tagged
-- fields that local.rs hashes) is also modeled abstractly as a ℕ. Both are hSets.
Label : Type
Label = ℕ

Local : Type
Local = ℕ

-- The frame on a child edge. On Structure it is (ordinal, label); elsewhere trivial.
-- This mirrors acyclic.rs lines 178-184 where ordinal/label are pushed only on
-- Structure.
data Frame : Dimension → Type where
  structFrame  : (ordinal : ℕ) (label : Label) → Frame Structure
  trivialFrame : {D : Dimension} → Frame D

------------------------------------------------------------------------
-- The dimension-tagged structure as a strictly-positive inductive (W-type shaped).
-- A node = local payload + an ordered list of (frame, child) pairs. This is the
-- initial algebra mu F of the polynomial functor
--     F_D X = Local x List (Frame_D x X).
------------------------------------------------------------------------

data Term (D : Dimension) : Type where
  node : Local → List (Frame D × Term D) → Term D

-- A leaf: a node with no children.
leaf : {D : Dimension} → Local → Term D
leaf l = node l []

------------------------------------------------------------------------
-- The catamorphism. An F-algebra at dimension D over a codomain X is a function
--     alpha : Local -> List (Frame D x X) -> X.
-- `fold alpha` is the unique algebra map Term D -> X.
--
-- The defining equation `fold alpha (node l cs) = alpha l (map ... cs)` is,
-- DEFINITIONALLY in Agda, the engine recursion equation
--     d(parent) = alpha(local, map d children).
------------------------------------------------------------------------

Algebra : Dimension → Type ℓ → Type ℓ
Algebra D X = Local → List (Frame D × X) → X

module _ {X : Type ℓ} {D : Dimension} (alpha : Algebra D X) where

  -- `fold` is the unique algebra map. The child list is folded by an inlined
  -- structurally-recursive helper `foldChildren` (recursion on the list spine), so
  -- the termination checker sees the descent into each child. This computes exactly
  -- like `map (fold on the child)`, and `foldChildren-is-map` below records that.
  fold         : Term D → X
  foldChildren : List (Frame D × Term D) → List (Frame D × X)

  fold (node l cs) = alpha l (foldChildren cs)
  foldChildren [] = []
  foldChildren ((fr , t) ∷ cs) = (fr , fold t) ∷ foldChildren cs

------------------------------------------------------------------------
-- The abstract hash / Digest, and the blake3 algebra over it (abstract).
-- This is the hash-split boundary: Digest is an abstract hSet, hashRecord is an
-- abstract function. The address `addr` is then `fold hashAlg`, exactly the engine's
-- per-dimension Merkle fold, but over the abstract hash.
------------------------------------------------------------------------

postulate
  Digest      : Type
  isSetDigest : isSet Digest
  -- The record-hashing primitive: it takes the assembled payload (the local digest
  -- plus the canonically-ordered framed child digests) to a digest. Abstract; the
  -- Lean port owns the concrete BLAKE3 bytes.
  hashRecord  : {D : Dimension} → Local → List (Frame D × Digest) → Digest

-- The hashing F-algebra at dimension D. Concrete in shape (it assembles the payload
-- and calls the abstract primitive), abstract in the hash.
hashAlg : (D : Dimension) → Algebra D Digest
hashAlg D l cs = hashRecord {D} l cs

-- The per-dimension content address: the unique algebra map into Digest. This IS
-- the engine's node_tree_digest, modeled exactly as a catamorphism.
addr : {D : Dimension} → Term D → Digest
addr {D} = fold (hashAlg D)

-- The inlined child fold agrees with `map` of the child fold, by structural
-- induction on the list. This lets us state the engine equation in the familiar
-- `map d children` shape.
foldChildren-is-map :
    {X : Type ℓ} {D : Dimension} (alpha : Algebra D X)
    (cs : List (Frame D × Term D))
  → foldChildren alpha cs ≡ map (λ p → (fst p , fold alpha (snd p))) cs
foldChildren-is-map alpha [] = refl
foldChildren-is-map alpha ((fr , t) ∷ cs) =
  cong ((fr , fold alpha t) ∷_) (foldChildren-is-map alpha cs)

------------------------------------------------------------------------
-- The catamorphism computes: the defining equation holds by refl (it is
-- definitional). This is the substitution principle in seed form: the address of a
-- parent is a function of the addresses of its (framed) children.
------------------------------------------------------------------------

-- The fold's defining equation, definitionally:
addr-node : {D : Dimension} (l : Local) (cs : List (Frame D × Term D))
          → addr (node l cs) ≡ hashRecord {D} l (foldChildren (hashAlg D) cs)
addr-node l cs = refl

-- And the same equation in the engine's `map d children` shape:
addr-node-map : {D : Dimension} (l : Local) (cs : List (Frame D × Term D))
          → addr (node l cs) ≡ hashRecord {D} l (map (λ p → (fst p , addr (snd p))) cs)
addr-node-map {D} l cs = cong (hashRecord {D} l) (foldChildren-is-map (hashAlg D) cs)

-- Uniqueness / the substitution principle (the standing soundness obligation flagged
-- by the ideals spike): if two child lists fold to equal digest lists, the parent
-- addresses are equal. Proved by congruence on the (definitional) fold equation.
addr-cong-children :
    {D : Dimension} (l : Local) (cs ds : List (Frame D × Term D))
  → foldChildren (hashAlg D) cs ≡ foldChildren (hashAlg D) ds
  → addr (node l cs) ≡ addr (node l ds)
addr-cong-children {D} l cs ds q = cong (hashRecord {D} l) q
