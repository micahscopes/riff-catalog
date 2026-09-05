{-# OPTIONS --cubical --no-import-sorts --guardedness #-}

{-
  Riffcat.Cache

  Proofs as documentation for cache invalidation in a faceted, causally observed
  compiler.

  The central distinction is small but important:

    * an artifact is immutable evidence of what was produced;
    * a receipt is the revisable claim that the artifact still answers a query.

  A content-addressed artifact therefore need not be deleted when later history
  arrives.  Instead its receipt is revalidated.  If the receipt is no longer live,
  the materialization is retracted from the current view while the historical
  artifact remains available.

  This module is a CORRESPONDENCE with hhs3's co-transaction model, not a proof of
  the hhs3 implementation.  It captures four reusable proof obligations:

    1. a cached query must factor through the support observation it declares;
    2. every member of an atomic support bundle must remain live;
    3. skipping revalidation outside a hard revision bound requires a proof;
    4. a cached liveness verdict is safe only when its key determines the verdict.

  The first obligation is Riffcat's familiar facet-preservation law.  The next two
  add the causal and co-transactional dimensions.  The fourth draws a bright line
  between a complete support digest and an unsafe cache of traversal-dependent
  intermediate verdicts.
-}

module Riffcat.Cache where

open import Cubical.Foundations.Prelude
open import Cubical.Data.Bool using (Bool ; true ; false ; true≢false)
open import Cubical.Relation.Nullary using (¬_)

------------------------------------------------------------------------
-- Receipts and horizon-relative liveness.
--
-- `observed-at` is the historical position used to produce the result.  `from`
-- in `ValidAt` is the later horizon from which that old observation is judged.
-- Keeping those coordinates separate is the part of hhs3 that a plain dependency
-- list does not express.
------------------------------------------------------------------------

record Receipt (Frontier Support : Type) : Type where
  constructor receipt
  field
    observed-at : Frontier
    supports    : Support

open Receipt public

record Liveness (Frontier Support : Type) : Type₁ where
  field
    Live : Frontier → Support → Type

open Liveness public

ValidAt : {Frontier Support : Type}
  → Liveness Frontier Support
  → Frontier
  → Receipt Frontier Support
  → Type
ValidAt liveness from cached = Live liveness from (supports cached)

------------------------------------------------------------------------
-- Co-transactional bundles.
--
-- The bundle is live exactly when every named support is live.  This is an
-- all-or-nothing policy unit: consumers never receive a half-live bundle.
------------------------------------------------------------------------

record SupportBundle (Part Support : Type) : Type where
  constructor bundle
  field
    support-at : Part → Support

open SupportBundle public

BundleLive : {Frontier Part Support : Type}
  → Liveness Frontier Support
  → Frontier
  → SupportBundle Part Support
  → Type
BundleLive liveness from bundled =
  (part : _) → Live liveness from (support-at bundled part)

bundle-member-live :
  {Frontier Part Support : Type}
  {liveness : Liveness Frontier Support}
  {from : Frontier} {bundled : SupportBundle Part Support}
  → BundleLive liveness from bundled
  → (part : Part)
  → Live liveness from (support-at bundled part)
bundle-member-live all-live part = all-live part

-- One dead member refutes liveness of the whole bundle.  Nothing here selects a
-- partially valid result.
one-dead-voids-bundle :
  {Frontier Part Support : Type}
  {liveness : Liveness Frontier Support}
  {from : Frontier} {bundled : SupportBundle Part Support}
  (part : Part)
  → ¬ (Live liveness from (support-at bundled part))
  → ¬ (BundleLive liveness from bundled)
one-dead-voids-bundle part dead all-live = dead (all-live part)

-- Transitive retraction is ordinary implication read backwards: if a downstream
-- result can be live only while its upstream support is live, voiding the upstream
-- rules out a live downstream result as well.
support-voids-dependent : {UpstreamLive DownstreamLive : Type}
  → (DownstreamLive → UpstreamLive)
  → ¬ UpstreamLive
  → ¬ DownstreamLive
support-voids-dependent requires-upstream upstream-dead downstream-live =
  upstream-dead (requires-upstream downstream-live)

------------------------------------------------------------------------
-- A cacheable query declares the complete observation through which it factors.
--
-- `_~support_` may be equality at a Riffcat facet rather than equality of the full
-- input.  `stable` is the proof that the query cannot notice what that observation
-- forgot.  This is the safe-reuse law.
------------------------------------------------------------------------

record CacheableQuery (Input Support Output : Type) : Type₁ where
  field
    observe     : Input → Support
    run         : Input → Output
    _~support_  : Support → Support → Type
    _~output_   : Output → Output → Type
    stable      : (old new : Input)
                → observe old ~support observe new
                → run old ~output run new

module QueryCache
  {Input Support Output : Type}
  (query : CacheableQuery Input Support Output)
  where

  open CacheableQuery query

  -- A materialization keeps the exact input for this small model, a separately
  -- stored result, and proofs tying both the result and receipt to that execution.
  -- A real engine can replace `input` with an address into immutable storage.
  record Materialization (Frontier : Type) : Type where
    constructor materialized
    field
      input            : Input
      result           : Output
      result-correct   : result ≡ run input
      support-receipt  : Receipt Frontier Support
      receipt-captures : supports support-receipt ≡ observe input

  open Materialization public

  record Green
    {Frontier : Type}
    (liveness : Liveness Frontier Support)
    (cached : Materialization Frontier)
    (new : Input)
    (from : Frontier)
    : Type where
    field
      still-live    : ValidAt liveness from (support-receipt cached)
      same-supports :
        supports (support-receipt cached) ~support observe new

  -- Green reuse is sound because the receipt names the old observation and the
  -- query is proved to factor through that observation.  Liveness warrants using
  -- the result now; preservation proves that its observable value is unchanged.
  green-is-sound :
    {Frontier : Type}
    (liveness : Liveness Frontier Support)
    (cached : Materialization Frontier)
    (new : Input)
    (from : Frontier)
    → Green liveness cached new from
    → result cached ~output run new
  green-is-sound liveness cached new from evidence =
    subst (λ output → output ~output run new)
      (sym (result-correct cached))
      (stable (input cached) new
        (subst (λ support → support ~support observe new)
          (receipt-captures cached)
          (Green.same-supports evidence)))

  -- These evidence types keep three different events distinct.  Backdating means
  -- "we had to run, but the output stayed equal".  Replacement means the output
  -- changed.  Retraction means the old receipt is no longer warranted at `from`.
  record Backdated {Frontier : Type}
    (cached : Materialization Frontier) (new : Input) : Type where
    field
      supports-differ :
        ¬ (observe (input cached) ~support observe new)
      output-agrees : result cached ~output run new

  record Replaced {Frontier : Type}
    (cached : Materialization Frontier) (new : Input) : Type where
    field
      output-differs : ¬ (result cached ~output run new)

  record Retracted
    {Frontier : Type}
    (liveness : Liveness Frontier Support)
    (cached : Materialization Frontier)
    (from : Frontier)
    : Type where
    field
      receipt-dead : ¬ (ValidAt liveness from (support-receipt cached))

  data Outcome
    {Frontier : Type}
    (liveness : Liveness Frontier Support)
    (cached : Materialization Frontier)
    (new : Input)
    (from : Frontier)
    : Type where
    green      : Green liveness cached new from
               → Outcome liveness cached new from
    backdated  : Backdated cached new
               → Outcome liveness cached new from
    replaced   : Replaced cached new
               → Outcome liveness cached new from
    retracted  : Retracted liveness cached from
               → Outcome liveness cached new from

------------------------------------------------------------------------
-- Hard and soft revision filters.
--
-- A hard filter earns the right to skip work by carrying `stable-outside`.
-- A soft filter is deliberately only a candidate-selection hint.  Its type offers
-- no way to conclude that a receipt stayed valid merely because the hint said it
-- was unaffected.
------------------------------------------------------------------------

record HardRevisionFilter
  (Change Frontier Cached : Type)
  (Valid : Frontier → Cached → Type)
  : Type₁ where
  field
    start          : Change → Frontier
    end            : Change → Frontier
    may-affect     : Change → Cached → Type
    stable-outside : (change : Change) (cached : Cached)
      → ¬ (may-affect change cached)
      → Valid (start change) cached
      → Valid (end change) cached

module HardFilter
  {Change Frontier Cached : Type}
  {Valid : Frontier → Cached → Type}
  (filter : HardRevisionFilter Change Frontier Cached Valid)
  where

  open HardRevisionFilter filter

  skip-revalidation-is-safe : (change : Change) (cached : Cached)
    → ¬ (may-affect change cached)
    → Valid (start change) cached
    → Valid (end change) cached
  skip-revalidation-is-safe = stable-outside

record SoftRevisionFilter (Change Cached : Type) : Type₁ where
  field
    may-affect : Change → Cached → Type

------------------------------------------------------------------------
-- Support-digest verdict caching.
--
-- Equality of keys is useful only after proving that the key determines the
-- top-level verdict.  In practice `Support` must include the complete canonical
-- support slice, including any disciplined negative observations.
------------------------------------------------------------------------

record CompleteSupportKey (Support Key Verdict : Type) : Type₁ where
  field
    key        : Support → Key
    evaluate   : Support → Verdict
    sufficient : (old new : Support)
      → key old ≡ key new
      → evaluate old ≡ evaluate new

module VerdictCache
  {Support Key Verdict : Type}
  (contract : CompleteSupportKey Support Key Verdict)
  where

  open CompleteSupportKey contract

  cache-hit-is-sound : (old new : Support)
    → key old ≡ key new
    → evaluate old ≡ evaluate new
  cache-hit-is-sound = sufficient

------------------------------------------------------------------------
-- Concrete sanity check: the artifact persists after its warrant expires.
------------------------------------------------------------------------

module WarrantCanExpire where

  equality-liveness : Liveness Bool Bool
  Live equality-liveness from required = from ≡ required

  cached-receipt : Receipt Bool Bool
  cached-receipt = receipt true true

  live-when-observed : ValidAt equality-liveness true cached-receipt
  live-when-observed = refl

  dead-from-later-horizon : ¬ (ValidAt equality-liveness false cached-receipt)
  dead-from-later-horizon false≡true = true≢false (sym false≡true)

  artifact : Bool
  artifact = true

  -- Retraction changed the warrant, not the immutable artifact.
  artifact-still-exists : artifact ≡ true
  artifact-still-exists = refl
