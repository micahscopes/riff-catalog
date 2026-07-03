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
  - The PRIME FORM / most-compact normal order is the normalization picking a
    representative: the vector whose 12-bit integer reading (pitch class 11 the most
    significant bit) is minimal over all transpositions (for T-classes) and over all
    transpositions of the set and its inversion (for set classes). This is Rahn's
    most-compact rule and it is the SAME canonicalization the Rust engine computes
    (riff-catalog-music::set_theory: the min-bitmask normal form and prime form), so
    the two witnesses share one convention, the published catalog's.

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

-- The EXPLICIT inversion operation: the I generator of the dihedral group, the exact
-- pitch-class mirror pc -> (12 - pc) mod 12. On the length-12 characteristic vector
-- this is the positional shuffle out[i] = in[(12 - i) mod 12], i.e. fix index 0 and
-- swap i with 12-i. This is the SAME move the demo's `invert` toggle performs on a
-- chord's pitch classes; we write it positionally so it computes by refl. (It differs
-- from `rev` above only by a transposition, so both give the same set class, but this
-- one is the on-the-nose I generator the demo shows audibly.)
invertI : Pcs → Pcs
invertI (b0 ∷ b1 ∷ b2 ∷ b3 ∷ b4 ∷ b5 ∷ b6 ∷ b7 ∷ b8 ∷ b9 ∷ b10 ∷ b11 ∷ []) =
  b0 ∷ b11 ∷ b10 ∷ b9 ∷ b8 ∷ b7 ∷ b6 ∷ b5 ∷ b4 ∷ b3 ∷ b2 ∷ b1 ∷ []
invertI p = p  -- non-12 vectors are left alone; we only ever build length-12 ones

------------------------------------------------------------------------
-- The Rahn packing order: read the characteristic vector as a 12-bit integer with
-- pitch class 11 the MOST significant bit, and prefer the smaller integer. Minimizing
-- that integer clears the highest pitch class first, then the next one down: exactly
-- Rahn's "smallest span, then packed inward from the right" normal-order rule, and
-- exactly the min-bitmask normal form the Rust engine computes, so the witnesses
-- share ONE canonicalization. (Plain left-to-right lexicographic packing is NOT that
-- rule: it disagrees with the published catalog on e.g. the minor seventh, Forte
-- 4-26, whose compact prime form is checked below.)
------------------------------------------------------------------------

-- is xs ≤ ys read most-significant-bit-first (heads are the high bits, false < true)?
msbLE : List Bool → List Bool → Bool
msbLE [] _ = true
msbLE (_ ∷ _) [] = false
msbLE (x ∷ xs) (y ∷ ys) =
  if x then (if y then msbLE xs ys else false)   -- x has the high bit, y not: x > y
       else (if y then true else msbLE xs ys)    -- y has the high bit, x not: x < y

-- is xs ≤ ys as 12-bit integers (index 11 most significant)? Reverse both so the
-- high bit leads, then compare most-significant-first.
rahnLE : List Bool → List Bool → Bool
rahnLE xs ys = msbLE (rev xs) (rev ys)

-- the more compact (smaller-integer) of two vectors.
rahnMin : List Bool → List Bool → List Bool
rahnMin xs ys = if rahnLE xs ys then xs else ys

------------------------------------------------------------------------
-- All twelve transpositions, and the most compact one (minimal as a 12-bit integer)
-- = the transposition-class normal form, the Tn-type. This is the demo's
-- A/B-distinguished form: tNF of a major triad is [0,4,7] (3-11B), of a minor triad
-- [0,3,7] (3-11A), matching the engine's transposition_normal_form on the nose.
------------------------------------------------------------------------

-- the list of all twelve transpositions of p.
allTranspositions : Pcs → List Pcs
allTranspositions p = go 12 p
  where
    go : ℕ → Pcs → List Pcs
    go zero    _ = []
    go (suc n) q = q ∷ go n (rotate1 q)

-- fold rahnMin over a nonempty list of candidates, seeded by the first.
minOf : Pcs → List Pcs → Pcs
minOf seed [] = seed
minOf seed (c ∷ cs) = minOf (rahnMin seed c) cs

-- transposition-only normal form: most compact over the twelve transpositions.
transNormalForm : Pcs → Pcs
transNormalForm p with allTranspositions p
... | []       = p
... | (c ∷ cs) = minOf c cs

-- set-class (transpose-AND-invert) prime form: most compact over transpositions of
-- BOTH the set and its inversion. This is the published (Rahn) prime form, the same
-- value the engine's set_theory::prime_form returns.
primeForm : Pcs → Pcs
primeForm p = rahnMin (transNormalForm p) (transNormalForm (invert p))

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
-- THE PACKING-RULE WITNESS: the minor seventh (Forte 4-26). Its published prime form
-- is the compact [0,3,5,8]; the left-to-right lexicographic shortcut instead lands on
-- the looser [0,2,5,9]. This is the exact case the Rust engine's prime_form was fixed
-- on (riff-catalog-music::set_theory); checking it here, by refl, locks the cubical
-- witness and the engine to ONE convention.
------------------------------------------------------------------------

Aminor7 : Pcs
Aminor7 = mkPcs T F F F T F F T F T F F   -- {A,C,E,G} = 0,4,7,9

primeForm-4-26 : Pcs
primeForm-4-26 = mkPcs T F F T F T F F T F F F   -- 0,3,5,8

Aminor7-prime : primeForm Aminor7 ≡ primeForm-4-26
Aminor7-prime = refl

------------------------------------------------------------------------
-- THE EXPLICIT INVERSION OPERATION (the demo's `invert` toggle, checked).
--
-- The set-class quotient FOLDS inversion: major and minor already share a prime form
-- (above). The finer transposition class does NOT fold it: at the Tn level major and
-- minor are DISTINCT (this is the standard A/B distinction, 3-11B vs 3-11A). Inversion
-- is exactly the move between them. We check, all by refl on the concrete vectors:
--   (1) major and minor are DIFFERENT transposition classes (the A/B distinction holds);
--   (2) inverting the major triad (the I generator invertI) lands on the MINOR type;
--   (3) the augmented triad is inversionally SYMMETRIC: invertI fixes it on the nose.
------------------------------------------------------------------------

-- (1) The A/B distinction: major and minor are NOT the same transposition class, so
-- the Tn-type keeps them apart (3-11B vs 3-11A). Project a position where their
-- transposition normal forms differ. (Agda computes tNF Cmajor = [0,4,7], the 3-11B
-- form on the nose, and tNF Aminor = [0,3,7], the 3-11A form; position 4 is true for
-- major and false for minor, so we project it and read off true ≢ false.)
private
  pos4 : Pcs → Bool
  pos4 (_ ∷ _ ∷ _ ∷ _ ∷ b ∷ _) = b
  pos4 _ = false

major-minor-different-transclass : ¬ (Cmajor ~T Aminor)
major-minor-different-transclass eq = true≢false (cong pos4 eq)

-- (2) THE inversion headline: invert the C major triad with the explicit I generator
-- and it lands on the MINOR type (3-11B inverts to 3-11A). Checked at the Tn level,
-- where the two are genuinely distinct, so this equality has content. By refl.
invert-major-is-minor : invertI Cmajor ~T Aminor
invert-major-is-minor = refl

-- the same fact, but reading the minor type off C minor (the literal [0,3,7]).
invert-major-is-minor-type : invertI Cmajor ~T Cminor
invert-major-is-minor-type = refl

-- (3) The augmented triad is inversionally symmetric: the I generator fixes it exactly
-- (not merely up to transposition). By refl on the concrete vector.
invert-augmented-fixed : invertI Caug ≡ Caug
invert-augmented-fixed = refl

------------------------------------------------------------------------
-- Grounding the abstract HIT: the set-class quotient is a FACET in exactly the sense
-- of Riffcat.Facet (a kernel-of-normalization congruence). And the set-class quotient
-- is a COARSENING of the transposition-class quotient: ~T refines ~SC (same
-- transposition normal form implies same prime form), so by the refinement theorem
-- the transposition-class quotient surjects onto the set-class quotient.
------------------------------------------------------------------------

-- ~T refines ~SC: if two sets share a transposition normal form, they share a prime
-- form. (Same T-normal-form => same transNormalForm on the set; the prime form is
-- rahnMin of that with the inversion's, and the inversion of T-equal sets need not be
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
~T'⊆~SC p q (eT , eInv) = cong₂ rahnMin eT eInv

-- the refinement theorem, instantiated: the finer (transpose-data) quotient surjects
-- onto the coarser set-class quotient.
open Refinement _~T'_ _~SC_ ~T'⊆~SC
  using ()
  renaming (coarsen to setClassFromTransClass ; coarsen↠ to setClassFromTransClass↠)
