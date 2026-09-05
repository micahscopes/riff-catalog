import Std

/-!
Small specifications for artifact census accounting and copy-pattern interpretation.
These do not model WGSL pointers, types, concurrency, or compiler legality.
Rust tests compare interval-union accounting with this finite byte-mask model.
-/
namespace Riffcat.ArtifactCensus

def covered : List Bool → Nat
  | [] => 0
  | b :: bs => (if b then 1 else 0) + covered bs

def uncovered : List Bool → Nat
  | [] => 0
  | b :: bs => (if b then 0 else 1) + uncovered bs

theorem byte_partition (mask : List Bool) :
    covered mask + uncovered mask = mask.length := by
  induction mask with
  | nil => rfl
  | cons b bs ih =>
    cases b <;> simp_all [covered, uncovered, Nat.add_assoc, Nat.add_comm, Nat.add_left_comm]

theorem covered_bounded (mask : List Bool) : covered mask ≤ mask.length := by
  have h := byte_partition mask
  omega

-- Duplicate or nested selections cannot be summed as disjoint partitions.
theorem overlapping_counts_are_not_additive :
    covered [true] + covered [true] ≠ covered [true] := by decide

def unionMask : List Bool → List Bool → List Bool
  | [], ys => ys
  | xs, [] => xs
  | x :: xs, y :: ys => (x || y) :: unionMask xs ys

theorem union_idempotent (mask : List Bool) : unionMask mask mask = mask := by
  induction mask with
  | nil => rfl
  | cons b bs ih => cases b <;> simp [unionMask, ih]

theorem union_coverage_bounds (left right : List Bool) :
    covered left ≤ covered (unionMask left right) ∧
    covered (unionMask left right) ≤ covered left + covered right := by
  induction left generalizing right with
  | nil => simp [unionMask, covered]
  | cons x xs ih =>
    cases right with
    | nil => simp [unionMask, covered]
    | cons y ys =>
      have lower := (ih ys).1
      have upper := (ih ys).2
      cases x <;> cases y <;> simp [unionMask, covered] <;> omega

structure Copy where
  dst : Nat
  src : Nat
  deriving DecidableEq

abbrev State := Nat → Nat

def step (state : State) (copy : Copy) : State :=
  fun location => if location = copy.dst then state copy.src else state location

def renameCopy (rename : Nat → Nat) (copy : Copy) : Copy :=
  ⟨rename copy.dst, rename copy.src⟩

def run (state : State) : List Copy → State
  | [] => state
  | copy :: rest => run (step state copy) rest

-- Safe scalar renaming requires preserved distinct locations and transported
-- state. A textual name-erasure match alone supplies neither condition.
theorem step_rename (rename : Nat → Nat)
    (injective : Function.Injective rename) (state renamed : State)
    (transport : ∀ location, renamed (rename location) = state location)
    (copy : Copy) (location : Nat) :
    step renamed (renameCopy rename copy) (rename location) = step state copy location := by
  by_cases h : location = copy.dst
  · simp [step, renameCopy, h, transport]
  · have different : rename location ≠ rename copy.dst := fun eq => h (injective eq)
    simp [step, renameCopy, h, different, transport]

theorem sequence_rename (rename : Nat → Nat)
    (injective : Function.Injective rename) (copies : List Copy)
    (state renamed : State)
    (transport : ∀ location, renamed (rename location) = state location) :
    ∀ location, run renamed (copies.map (renameCopy rename)) (rename location) =
      run state copies location := by
  induction copies generalizing state renamed with
  | nil => exact transport
  | cons copy rest ih =>
    exact ih (step state copy) (step renamed (renameCopy rename copy))
      (step_rename rename injective state renamed transport copy)

def initial : State := fun location => if location = 2 then 1 else 0
def collapse : Nat → Nat := fun location => if location = 1 then 0 else location
def copies : List Copy := [⟨0, 2⟩, ⟨2, 1⟩]

-- Even initially equal values do not justify introducing an alias.
theorem collapse_initially_agrees :
    ∀ location, initial (collapse location) = initial location := by
  intro location
  by_cases h : location = 1
  · simp [initial, collapse, h]
  · simp [collapse, h]

theorem alias_collapse_changes_result :
    run initial (copies.map (renameCopy collapse)) 2 ≠ run initial copies 2 := by decide

structure TextCopy where
  dstRoot : String
  dstMember : String
  srcRoot : String
  srcMember : String

def textPattern (copy : TextCopy) : String × String × Bool :=
  (copy.dstMember, copy.srcMember, copy.dstRoot == copy.srcRoot)

def render (copy : TextCopy) : String :=
  copy.dstRoot ++ "." ++ copy.dstMember ++ " = " ++ copy.srcRoot ++ "." ++ copy.srcMember ++ ";"

def shortCopy : TextCopy := ⟨"s", "a", "x", "b"⟩
def longCopy : TextCopy := ⟨"state", "a", "longer", "b"⟩

theorem same_pattern_different_byte_cost :
    textPattern shortCopy = textPattern longCopy ∧
    (render shortCopy).utf8ByteSize ≠ (render longCopy).utf8ByteSize := by decide

theorem no_pattern_only_exact_cost :
    ¬ ∃ cost : (String × String × Bool) → Nat,
      ∀ copy, cost (textPattern copy) = (render copy).utf8ByteSize := by
  intro ⟨cost, exactCost⟩
  have same := same_pattern_different_byte_cost.1
  have different := same_pattern_different_byte_cost.2
  apply different
  rw [← exactCost shortCopy, ← exactCost longCopy, same]

end Riffcat.ArtifactCensus
