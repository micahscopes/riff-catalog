/-
Foundational sorting lemmas for the correctness laws (no Mathlib).

The single mathematical kernel under Law 1 (DETERMINISM / order-independence) is
"sorting canonicalizes": the engine never hashes input order, it sorts every
multiset (`push_sorted_records`, the node sort by canonical key, the field sort
by (name, value)) before committing it. So permuting the input cannot change the
output, *because* the sort throws the order away.

This module makes that precise over an abstract Boolean comparator `le` that is a
total preorder (reflexive, transitive, total) with antisymmetry on the equality
used to compare list elements. We define our own insertion sort (Lean core has
no `List.insertionSort`/`List.Sorted` without Mathlib) and prove:

  - `isort_perm`        : `isort le l ~ l`            (sorting permutes)
  - `Sorted_isort`      : `Sorted le (isort le l)`     (sorting sorts)
  - `sorted_perm_eq`    : two `Sorted` permutations are equal (UNIQUENESS)
  - `isort_perm_invariant` (the headline): `l₁ ~ l₂ → isort le l₁ = isort le l₂`

`isort_perm_invariant` is the canonicalization fact Law 1 rests on. The Rust
engine uses an unstable comparison sort over records that are pairwise distinct
under a strict byte-lex order; the Lean spec uses `Array.qsort`. We prove the
property of the *specification's* sorting discipline (any correct sort of a
strict total order is the unique sorted permutation), which is exactly the
property the engine relies on.
-/

namespace Riffcat
namespace Laws

open List

variable {α : Type}

/-- Insert `a` into `l` under comparator `le` (insertion sort step). -/
def ins (le : α → α → Bool) (a : α) : List α → List α
  | [] => [a]
  | b :: l => if le a b then a :: b :: l else b :: ins le a l

/-- Insertion sort under comparator `le`. -/
def isort (le : α → α → Bool) : List α → List α
  | [] => []
  | a :: l => ins le a (isort le l)

/-- `ins` adds exactly its element: the result is a permutation of `a :: l`. -/
theorem ins_perm (le : α → α → Bool) (a : α) (l : List α) :
    ins le a l ~ a :: l := by
  induction l with
  | nil => simp [ins]
  | cons b l ih =>
    unfold ins
    by_cases h : le a b
    · simp [h]
    · simp only [h]
      calc b :: ins le a l ~ b :: (a :: l) := ih.cons b
        _ ~ a :: b :: l := by
              have := List.perm_middle (l₁ := [b]) (l₂ := l) (a := a)
              simpa using this

/-- `isort` is a permutation of its input. -/
theorem isort_perm (le : α → α → Bool) (l : List α) : isort le l ~ l := by
  induction l with
  | nil => simp [isort]
  | cons a l ih =>
    unfold isort
    calc ins le a (isort le l) ~ a :: isort le l := ins_perm le a (isort le l)
      _ ~ a :: l := ih.cons a

/-! ## Sortedness -/

/-- `Sorted le l`: adjacent-and-transitive, every element `le`-bounds all later
ones. Defined inline (Lean core has no `List.Sorted`). -/
inductive Sorted (le : α → α → Bool) : List α → Prop where
  | nil : Sorted le []
  | cons (a : α) (l : List α) :
      (∀ b ∈ l, le a b = true) → Sorted le l → Sorted le (a :: l)

theorem sorted_singleton (le : α → α → Bool) (a : α) : Sorted le [a] :=
  .cons a [] (by simp) .nil

/-- Membership is preserved by `ins` (it adds exactly `a`). -/
theorem mem_ins (le : α → α → Bool) (a : α) (l : List α) (x : α) :
    x ∈ ins le a l ↔ x ∈ a :: l :=
  (ins_perm le a l).mem_iff

/-- `ins` preserves sortedness, given the comparator is *total* (so the element
that loses the `le a b` test is `le`-bounded the other way) and *transitive* on
the head bound. -/
theorem Sorted_ins (le : α → α → Bool)
    (total : ∀ x y, le x y = true ∨ le y x = true)
    (trans : ∀ x y z, le x y = true → le y z = true → le x z = true)
    (a : α) (l : List α) (hl : Sorted le l) : Sorted le (ins le a l) := by
  induction l with
  | nil => simpa [ins] using sorted_singleton le a
  | cons b l ih =>
    unfold ins
    by_cases h : le a b
    · -- a goes in front; need a to bound b :: l
      simp only [h]
      refine .cons a (b :: l) ?_ hl
      intro y hy
      cases hy with
      | head => exact h
      | tail _ hy' =>
        -- b bounds y (from hl), and a ≤ b, so a ≤ y by transitivity
        cases hl with
        | cons _ _ hb _ => exact trans a b y h (hb y hy')
    · -- b stays in front; recurse, then show b bounds ins le a l
      simp only [h]
      cases hl with
      | cons _ _ hb hrest =>
        refine .cons b (ins le a l) ?_ (ih hrest)
        intro y hy
        have hy' : y ∈ a :: l := (mem_ins le a l y).1 hy
        cases hy' with
        | head =>
          -- y = a; from ¬ le a b and totality, le b a
          rcases total a b with hab | hba
          · exact absurd hab h
          · exact hba
        | tail _ hyl => exact hb y hyl

/-- `isort` produces a sorted list. -/
theorem Sorted_isort (le : α → α → Bool)
    (total : ∀ x y, le x y = true ∨ le y x = true)
    (trans : ∀ x y z, le x y = true → le y z = true → le x z = true)
    (l : List α) : Sorted le (isort le l) := by
  induction l with
  | nil => exact .nil
  | cons a l ih =>
    unfold isort
    exact Sorted_ins le total trans a (isort le l) ih

/-! ## Uniqueness of the sorted order -/

/-- Reflexivity follows from totality (`le x x ∨ le x x`). -/
theorem le_refl_of_total (le : α → α → Bool)
    (total : ∀ x y, le x y = true ∨ le y x = true) (x : α) : le x x = true := by
  rcases total x x with h | h <;> exact h

/-- A `Sorted` head `le`-bounds every element of the tail. -/
theorem sorted_head_bound (le : α → α → Bool) {a : α} {l : List α}
    (h : Sorted le (a :: l)) : ∀ b ∈ l, le a b = true := by
  cases h with
  | cons _ _ hb _ => exact hb

theorem sorted_tail (le : α → α → Bool) {a : α} {l : List α}
    (h : Sorted le (a :: l)) : Sorted le l := by
  cases h with
  | cons _ _ _ hrest => exact hrest

/-- UNIQUENESS: two sorted lists that are permutations of each other are equal,
given the comparator is *total* and *antisymmetric* (`le x y` and `le y x`
implies `x = y`). This is the heart of "sorting canonicalizes": there is exactly
one sorted arrangement of a given multiset. -/
theorem sorted_perm_eq (le : α → α → Bool)
    (total : ∀ x y, le x y = true ∨ le y x = true)
    (antisymm : ∀ x y, le x y = true → le y x = true → x = y) :
    ∀ {l₁ l₂ : List α}, Sorted le l₁ → Sorted le l₂ → l₁ ~ l₂ → l₁ = l₂ := by
  intro l₁
  induction l₁ with
  | nil =>
    intro l₂ _ _ hperm
    exact List.Perm.nil_eq hperm
  | cons a t ih =>
    intro l₂ hs1 hs2 hperm
    -- a is a member of l₂
    have hmem : a ∈ l₂ := hperm.mem_iff.1 (by simp)
    cases l₂ with
    | nil => exact absurd hmem (by simp)
    | cons b s =>
      -- show a = b: a bounds everything in t (so via perm, in s∪{b}); b bounds s
      have hba : a = b := by
        -- a ≤ b and b ≤ a, by both being heads of sorted lists permuting the same multiset
        have hb_bounds : ∀ y ∈ b :: s, le b y = true := by
          intro y hy
          cases hy with
          | head => exact le_refl_of_total le total b
          | tail _ hy' => exact sorted_head_bound le hs2 y hy'
        have ha_bounds : ∀ y ∈ a :: t, le a y = true := by
          intro y hy
          cases hy with
          | head => exact le_refl_of_total le total a
          | tail _ hy' => exact sorted_head_bound le hs1 y hy'
        -- b ∈ a :: t (perm), a ∈ b :: s (perm)
        have hb_in : b ∈ a :: t := hperm.mem_iff.2 (by simp)
        have ha_in : a ∈ b :: s := hmem
        exact antisymm a b (ha_bounds b hb_in) (hb_bounds a ha_in)
      subst hba
      -- strip the common head and recurse
      have hperm' : t ~ s := (List.perm_cons a).1 hperm
      have := ih (sorted_tail le hs1) (sorted_tail le hs2) hperm'
      rw [this]

/-- THE HEADLINE: `isort` is invariant under permutation of its input. Permuting
the records (field/edge/node insertion order) cannot change the sorted output,
hence cannot change anything downstream of the sort. -/
theorem isort_perm_invariant (le : α → α → Bool)
    (total : ∀ x y, le x y = true ∨ le y x = true)
    (trans : ∀ x y z, le x y = true → le y z = true → le x z = true)
    (antisymm : ∀ x y, le x y = true → le y x = true → x = y)
    {l₁ l₂ : List α} (h : l₁ ~ l₂) : isort le l₁ = isort le l₂ := by
  apply sorted_perm_eq le total antisymm
    (Sorted_isort le total trans l₁) (Sorted_isort le total trans l₂)
  exact (isort_perm le l₁).trans (h.trans (isort_perm le l₂).symm)

end Laws
end Riffcat
