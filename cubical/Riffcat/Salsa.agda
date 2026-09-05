{-# OPTIONS --cubical --no-import-sorts --guardedness #-}

{-
  Riffcat.Salsa

  An operational reading of Riffcat.LinkingKinds for Salsa 0.28.2, the latest
  release checked on 2026-09-03.  This models Salsa's public red-green contract,
  not its Rust implementation line by line.

  Official references:

    https://docs.rs/salsa/0.28.2/salsa/
    https://salsa-rs.github.io/salsa/reference/algorithm.html
    https://docs.rs/salsa/0.28.2/src/salsa/function/specify.rs.html

  Salsa records the inputs and queries read by a tracked function.  On a later
  revision it tries to validate those observations.  There are three useful cases:

    green       Dependencies still agree: reuse without executing.
    backdated   A dependency differed, so execute; the result still compares equal.
    changed     A dependency differed and the result differs too.

  Only the last case forces consumers of that result to regenerate.  Therefore
  "this query must rerun" and "its artifact must regenerate" are different claims.

  In the language of LinkingKinds, Salsa requires PRESERVATION for safe reuse.  It
  does not generally demand REFLECTION: dependency keys may change while results do
  not, and backdating is how the engine discovers that fact.  A proven same-kernel
  key would be an optional stronger case, not the baseline semantics.

  Current Salsa also offers independently read tracked fields, creator-scoped
  `specify`, and accumulators.  Accumulators are deliberately auxiliary: their
  contents do not participate in equality of the main query result.  The concrete
  example at the end shows why diagnostics or indices that need their own
  invalidation are often clearer as named, independently addressed output parts.
-}

module Riffcat.Salsa where

open import Cubical.Foundations.Prelude
open import Cubical.Data.Bool using (Bool ; true ; false ; true≢false)
open import Cubical.Relation.Nullary using (¬_)

open import Riffcat.LinkingKinds

record IncrementalQuery (Input Dependency Result : Type) : Type where
  field
    dependencies : Input → Dependency
    execute      : Input → Result

open IncrementalQuery public

module RedGreen
  {Input Dependency Result : Type}
  (query : IncrementalQuery Input Dependency Result)
  where

  SameDependencies : Input → Input → Type
  SameDependencies old new =
    dependencies query old ≡ dependencies query new

  SameResult : Input → Input → Type
  SameResult old new = execute query old ≡ execute query new

  SafeReuse : Type
  SafeReuse = Preserves SameDependencies SameResult (λ input → input)

  ExactChangeKey : Type
  ExactChangeKey = SameKernel SameDependencies SameResult (λ input → input)

  -- This classifies what validation learned.  The actual engine obtains this
  -- evidence by walking and, when necessary, re-executing the query graph.
  data Outcome (old new : Input) : Type where
    green : SameDependencies old new → Outcome old new
    backdated : ¬ (SameDependencies old new)
              → SameResult old new
              → Outcome old new
    changed : ¬ (SameDependencies old new)
            → ¬ (SameResult old new)
            → Outcome old new

  green-is-safe : SafeReuse → (old new : Input)
    → SameDependencies old new → SameResult old new
  green-is-safe safe old new same = safe old new same

  -- Backdating propagates through every ordinary consumer: applying one consumer
  -- to equal results preserves equality.
  backdating-keeps-consumer-green :
    {Consumer : Type}
    (consumer : Result → Consumer)
    (old new : Input)
    → SameResult old new
    → consumer (execute query old) ≡ consumer (execute query new)
  backdating-keeps-consumer-green consumer old new same = cong consumer same

------------------------------------------------------------------------
-- Main result versus sibling/auxiliary output.
--
-- These two values have the same main result and different diagnostics.  Equality
-- of the main result alone therefore cannot justify reusing the diagnostics.  This
-- is not a flaw in accumulators; it tells us which kind of product they are.  If a
-- consumer queries diagnostics directly, model them as an observed sibling port.
------------------------------------------------------------------------

record Products (Main Auxiliary : Type) : Type where
  constructor products
  field
    main      : Main
    auxiliary : Auxiliary

open Products public

module AccumulatorExample where

  old new : Products Bool Bool
  old = products true true
  new = products true false

  same-main : main old ≡ main new
  same-main = refl

  different-auxiliary : ¬ (auxiliary old ≡ auxiliary new)
  different-auxiliary = true≢false

