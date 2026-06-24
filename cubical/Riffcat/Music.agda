{-# OPTIONS --cubical --no-import-sorts --guardedness #-}

{-
  Riffcat.Music

  Deliverable 4: THE MUSIC INSTANCE (the on-ramp, concrete).

  Pitch classes are the twelve positions of a 12-bit characteristic vector (a
  `List Bool` of length 12: index i is present iff pitch class i sounds). A pitch-class
  SET is such a vector.

  - Transposition is cyclic rotation of the vector (the Z/12 group action). Inversion
    is the mirror (reverse the rotation), so transposition-and-inversion is the
    dihedral action.
  - The orbit relation under transposition (and, separately, under transposition AND
    inversion) is the congruence; the set class is `Pcs / ~`.
  - The PRIME FORM / minimal-rotation is the normalization picking a representative:
    the lexicographically least vector over all transpositions (for T-classes) and over
    all transpositions of the set and its inversion (for set classes, the Forte form).

  The demo's headline (memory: canonical "3-11"): major and minor triads land in the
  SAME set-class quotient. We check that as `refl`: C major and A minor (indeed any
  major and any minor triad) normalize to the SAME prime form, the set class Forte
  calls 3-11, whose prime form is [0, 3, 7].

  Everything here is concrete and computes: no postulate, no abstract hash. This is the
  set-class grounding of the abstract HIT in Riffcat.Facet (the orbit relation is a
  FacetRel-shaped kernel: x ~ y iff primeForm x ≡ primeForm y).
-}

module Riffcat.Music where

open import Cubical.Foundations.Prelude
open import Cubical.Data.Bool using (Bool ; true ; false ; not ; _and_ ; _or_ ; if_then_else_ ; isSetBool ; true≢false)
open import Cubical.Relation.Nullary using (¬_)
open import Cubical.Data.Nat using (ℕ ; zero ; suc)
open import Cubical.Data.List using (List ; [] ; _∷_ ; _++_ ; rev ; length)
open import Cubical.Data.Sigma using (_×_ ; _,_ ; fst ; snd)
open import Cubical.HITs.SetQuotients renaming ([_] to ⟦_⟧)
open import Cubical.Functions.Surjection using (_↠_)

open import Riffcat.Facet using (module Refinement)

------------------------------------------------------------------------
-- Pitch-class sets as 12-bit vectors.
------------------------------------------------------------------------

-- A pitch-class set: the characteristic vector. We keep it as a plain List Bool and
-- only ever build length-12 ones; the operations preserve length.
Pcs : Type
Pcs = List Bool

-- Build a 12-vector with the given pitch classes set. `pc i` flags class i.
-- We write the twelve booleans explicitly via a helper.
mkPcs : (b0 b1 b2 b3 b4 b5 b6 b7 b8 b9 b10 b11 : Bool) → Pcs
mkPcs b0 b1 b2 b3 b4 b5 b6 b7 b8 b9 b10 b11 =
  b0 ∷ b1 ∷ b2 ∷ b3 ∷ b4 ∷ b5 ∷ b6 ∷ b7 ∷ b8 ∷ b9 ∷ b10 ∷ b11 ∷ []

F : Bool
F = false

T : Bool
T = true

------------------------------------------------------------------------
-- The Z/12 transposition action: rotate the vector by one semitone (up). A single
-- rotation moves the last element to the front (rotate "up" by a semitone shifts each
-- present class i to i+1 mod 12). Iterating gives all twelve transpositions.
------------------------------------------------------------------------

-- one cyclic right-rotation: last element to the front. On a length-12 vector this is
-- transposition up by one semitone.
rotate1 : Pcs → Pcs
rotate1 [] = []
rotate1 (x ∷ xs) = lastToFront x xs
  where
    -- move the last element of (x ∷ xs) to the front, keep the rest in order.
    lastToFront : Bool → List Bool → List Bool
    lastToFront y [] = y ∷ []
    lastToFront y (z ∷ zs) with lastToFront z zs
    ... | (h ∷ t) = h ∷ y ∷ t
    ... | []      = y ∷ []      -- unreachable for nonempty, total by construction

-- transpose by n semitones: rotate n times.
transpose : ℕ → Pcs → Pcs
transpose zero    p = p
transpose (suc n) p = transpose n (rotate1 p)

-- inversion: reverse the vector. On the characteristic vector, mirroring index i to
-- -i is, up to a transposition, list reversal. Composing reversal with the twelve
-- rotations gives the full dihedral (transpose-and-invert) orbit, which is what the
-- set-class prime form quotients by.
invert : Pcs → Pcs
invert = rev

------------------------------------------------------------------------
-- Lexicographic order on Bool vectors with `true < false`, so that a vector with a
-- pitch class present EARLIER is SMALLER. The least rotation under this order is the
-- conventional left-packed prime form (a pitch at class 0, intervals packed to the
-- front), which for the major/minor triad set class is [0, 3, 7].
------------------------------------------------------------------------

-- is xs lexicographically ≤ ys (assuming equal length), with true < false?
lexLE : List Bool → List Bool → Bool
lexLE [] _ = true
lexLE (_ ∷ _) [] = false
lexLE (x ∷ xs) (y ∷ ys) =
  if x then (if y then lexLE xs ys else true)        -- x=true: x ≤ y always (true is least); tie if y=true
       else (if y then false else lexLE xs ys)       -- x=false: ≤ only if y=false too, then compare tails

-- the lexicographically smaller of two vectors.
lexMin : List Bool → List Bool → List Bool
lexMin xs ys = if lexLE xs ys then xs else ys

------------------------------------------------------------------------
-- All twelve transpositions, and the minimal one = the transposition-class normal
-- form (the "tightest packing" / transposition prime form).
------------------------------------------------------------------------

-- the list of all twelve transpositions of p.
allTranspositions : Pcs → List Pcs
allTranspositions p = go 12 p
  where
    go : ℕ → Pcs → List Pcs
    go zero    _ = []
    go (suc n) q = q ∷ go n (rotate1 q)

-- fold lexMin over a nonempty list of candidates, seeded by the first.
minOf : Pcs → List Pcs → Pcs
minOf seed [] = seed
minOf seed (c ∷ cs) = minOf (lexMin seed c) cs

-- transposition-only normal form: least over the twelve transpositions.
transNormalForm : Pcs → Pcs
transNormalForm p with allTranspositions p
... | []       = p
... | (c ∷ cs) = minOf c cs

-- set-class (transpose-AND-invert) prime form: least over transpositions of BOTH the
-- set and its inversion. This is the Forte prime form.
primeForm : Pcs → Pcs
primeForm p = lexMin (transNormalForm p) (transNormalForm (invert p))

------------------------------------------------------------------------
-- The orbit relations as congruences, and the set class as a quotient.
------------------------------------------------------------------------

-- transposition-class relation: same transposition normal form.
_~T_ : Pcs → Pcs → Type
p ~T q = transNormalForm p ≡ transNormalForm q

-- set-class relation (transpose and invert): same prime form. This is the orbit
-- relation under the dihedral action, the congruence whose classes are the set
-- classes.
_~SC_ : Pcs → Pcs → Type
p ~SC q = primeForm p ≡ primeForm q

-- the transposition-class quotient and the set-class quotient.
TransClass : Type
TransClass = Pcs / _~T_

SetClass : Type
SetClass = Pcs / _~SC_

-- the set-class address: the quotient map.
toSetClass : Pcs → SetClass
toSetClass = ⟦_⟧

------------------------------------------------------------------------
-- The named chords (pitch class 0 = C). Triads as 12-bit vectors.
--   C major  = {C, E, G}   = {0, 4, 7}
--   A minor  = {A, C, E}   = {9, 0, 4}
--   C minor  = {C, Eb, G}  = {0, 3, 7}
--   G major  = {G, B, D}   = {7, 11, 2}
------------------------------------------------------------------------

Cmajor : Pcs
Cmajor = mkPcs T F F F T F F T F F F F   -- 0,4,7

Aminor : Pcs
Aminor = mkPcs T F F F T F F F F T F F   -- 0,4,9

Cminor : Pcs
Cminor = mkPcs T F F T F F F T F F F F   -- 0,3,7

Gmajor : Pcs
Gmajor = mkPcs F F T F F F F T F F F T   -- 2,7,11

-- the augmented triad {0,4,8} and diminished triad {0,3,6} stand alone (different
-- set classes); we include the augmented one to exhibit a NON-collapse.
Caug : Pcs
Caug = mkPcs T F F F T F F F T F F F     -- 0,4,8

------------------------------------------------------------------------
-- THE CHECKED EXAMPLE: major and minor triads land in the SAME set-class quotient
-- (Forte 3-11). All by refl, by computation on the concrete vectors.
------------------------------------------------------------------------

-- The prime form of the major/minor triad set class. Forte 3-11; the textbook prime
-- form is [0, 3, 7], i.e. classes 0, 3, 7 present. Agda computes exactly this.
primeForm-3-11 : Pcs
primeForm-3-11 = mkPcs T F F T F F F T F F F F   -- 0,3,7

-- C major normalizes to the 3-11 prime form.
Cmajor-prime : primeForm Cmajor ≡ primeForm-3-11
Cmajor-prime = refl

-- A minor normalizes to the SAME prime form.
Aminor-prime : primeForm Aminor ≡ primeForm-3-11
Aminor-prime = refl

-- C minor too.
Cminor-prime : primeForm Cminor ≡ primeForm-3-11
Cminor-prime = refl

-- G major too.
Gmajor-prime : primeForm Gmajor ≡ primeForm-3-11
Gmajor-prime = refl

-- THE headline: C major and A minor are in the same set class (3-11). By refl.
major-minor-same-setclass : Cmajor ~SC Aminor
major-minor-same-setclass = refl

-- packaged as an actual equality of classes in the quotient (the address collision).
major-minor-same-address : toSetClass Cmajor ≡ toSetClass Aminor
major-minor-same-address = eq/ Cmajor Aminor major-minor-same-setclass

-- and a NON-collapse, to show the quotient is not trivial: the augmented triad is a
-- DIFFERENT set class from the major triad. Its prime form is [0, 4, 8] (Forte 3-12),
-- visibly different from [0, 3, 7], so major and augmented are different set classes.
Caug-prime : primeForm Caug ≡ mkPcs T F F F T F F F T F F F   -- 0,4,8
Caug-prime = refl

-- The non-collapse, as a CHECKED inequality (not just visual): the major and augmented
-- triads are NOT in the same set class. Project position 3 of the prime forms: the
-- major triad has a pitch class there ([0,3,7]), the augmented does not ([0,4,8]).
private
  pos3 : Pcs → Bool
  pos3 (_ ∷ _ ∷ _ ∷ b ∷ _) = b
  pos3 _ = false

major-augmented-different-setclass : ¬ (Cmajor ~SC Caug)
major-augmented-different-setclass eq = true≢false (cong pos3 eq)

-- (Cmajor's prime form is [0,3,7]; Caug's is [0,4,8]; these are visibly different
--  vectors, so major and augmented are different set classes. The distinctness is the
--  inequality of these two concrete vectors, which holds since they differ in a
--  decidable position; we leave it as the visible computed forms rather than wiring a
--  Bool-vector apartness proof, to keep the module focused.)

------------------------------------------------------------------------
-- Grounding the abstract HIT: the set-class quotient is a FACET in exactly the sense
-- of Riffcat.Facet (a kernel-of-normalization congruence). And the set-class quotient
-- is a COARSENING of the transposition-class quotient: ~T refines ~SC (same
-- transposition normal form implies same prime form), so by the refinement theorem
-- the transposition-class quotient surjects onto the set-class quotient.
------------------------------------------------------------------------

-- ~T refines ~SC: if two sets share a transposition normal form, they share a prime
-- form. (Same T-normal-form => same transNormalForm on the set; the prime form is
-- lexMin of that with the inversion's, and the inversion of T-equal sets need not be
-- T-equal in general, so we prove this for the engine via the normal forms directly.)
--
-- We prove the contained-in fact by: primeForm depends on p ONLY through
-- transNormalForm p and transNormalForm (invert p). For ~T we have equal
-- transNormalForm; we additionally need equal transNormalForm of the inversion. That
-- holds when the relation we coarsen from is "same prime form's first component AND
-- same inversion component". To keep the refinement HONEST and provable, we coarsen
-- from the FULL orbit data: define ~T' as "equal transNormalForm AND equal
-- transNormalForm of inversion", which clearly refines ~SC and is what the dihedral
-- tower actually layers.

_~T'_ : Pcs → Pcs → Type
p ~T' q = (transNormalForm p ≡ transNormalForm q)
        × (transNormalForm (invert p) ≡ transNormalForm (invert q))

~T'⊆~SC : (p q : Pcs) → p ~T' q → p ~SC q
~T'⊆~SC p q (eT , eInv) = cong₂ lexMin eT eInv

-- the refinement theorem, instantiated: the finer (transpose-data) quotient surjects
-- onto the coarser set-class quotient.
open Refinement _~T'_ _~SC_ ~T'⊆~SC
  using ()
  renaming (coarsen to setClassFromTransClass ; coarsen↠ to setClassFromTransClass↠)
