/-
Phase 2: the engine's correctness laws, stated as Lean theorems over the port's
own types, with the tractable ones proved.

This module mirrors the "engine checks too / to prove" list from the spike. Each
law is stated against the Phase-1 spec types (`Graph`, `Digest`, `Facet`,
`FacetAddress`, the per-dimension digest maps), then either PROVED outright (no
`sorry`) or stated as a clearly-marked TARGET. Targets use a named `axiom` with a
`TODO` so they typecheck, are greppable, and never masquerade as proofs.

Status table (kept in sync with lean/README.md):

  Law 1  DETERMINISM / order-independence  : PROVED (kernel) + STATED at the
         encoder boundary. The kernel "sorting canonicalizes" is fully proved in
         `Riffcat.Laws.Sorting`; here we package it as the engine-facing
         statement that the canonical record/field/node commitments are
         permutation-invariant.
  Law 2  FACET REFINEMENT LATTICE          : PROVED. Equality of the per-dimension
         digest map at a finer facet forces equality at every coarser facet,
         because a facet address commits exactly the sorted per-dimension digests
         and a coarser facet projects to a subset.
  Law 3  ANCHOR / TRANSPORT (Quot UP)      : PROVED (easy direction) + STATED.
         "A fact rides facet F iff ~_F ⊆ ker(fact)" is the universal property of
         the facet quotient. The factor-through-the-quotient direction is
         `Quot.lift`, proved by `rfl`; the converse (necessity) is stated.
  Law 4  AnonymousShape (names blind)      : TARGET (axiom). Renaming node keys by
         an injection leaves every AnonymousShape digest unchanged. Stated
         precisely; the deepest one (the whole pipeline must be shown key-free in
         anonymous mode).
  Law 5  SCC / WL termination + soundness  : TARGET (axiom) for termination, with
         the honest WL-equivalence (not isomorphism) caveat stated in prose and a
         soundness/incompleteness target.
  Law 6  ENCODING INJECTIVITY (mod hash)   : TARGET (axiom). The canonical byte
         encoding is injective (length prefixes + domain-separating tags), so
         distinct shapes give distinct pre-image bytes; BLAKE3 collision-freedom
         stays an explicit, separate axiom.

The proved laws stand on the port's own definitions. The targets are honest
statements of what remains, not stubs that pretend to be done.
-/

import Riffcat.Schema
import Riffcat.Policy
import Riffcat.Encode
import Riffcat.Reference
import Riffcat.Hash
import Riffcat.Laws.Sorting

namespace Riffcat
namespace Laws

open Encode
open List

/-! ##############################################################
    ## Law 1: DETERMINISM / order-independence
    ##############################################################

The digest of a (policy, graph) is invariant under permuting field and edge
insertion order and under renumbering internal node ids. The reason is local and
structural: every multiset the engine commits (the field list in
`localNodeDigest`, the record multiset in `pushSortedRecords`, the node list in
`IndexedGraph.build`) is *sorted* before it is hashed, by a total order whose key
does not depend on insertion order or on `u32` ids. So the committed byte
sequence is the unique sorted arrangement of the multiset, which a permutation
cannot change.

The mathematical kernel is fully proved in `Riffcat.Laws.Sorting`:
`isort_perm_invariant`. Here we lift it to the encoder boundary. We model the
canonical commitment of a record multiset as `isort byteLe` (the engine's
`pushSortedRecords` does the same sort, then a fixed count + concat that is a
function of the sorted list alone). -/

/-- A byte-array `Builder`. -/
abbrev RecordList := List ByteArray

/-- The canonical commitment of a record multiset: sort, then the bytes are a
function of the sorted list alone (`pushSortedRecords` pushes a count = length
and concatenates, both determined by the sorted list). We package "what is
hashed" as the sorted list itself; equal sorted lists give equal committed
bytes. -/
def canonicalRecords (byteLe : ByteArray → ByteArray → Bool) (records : RecordList)
    : RecordList :=
  isort byteLe records

/-- LAW 1 (kernel, PROVED): the canonical commitment of a record multiset is
invariant under permuting the records, for any total/antisymmetric byte order.
Permuting field/edge insertion order permutes exactly these record lists, so the
committed bytes, hence the digest, are unchanged. -/
theorem determinism_records
    (byteLe : ByteArray → ByteArray → Bool)
    (total : ∀ x y, byteLe x y = true ∨ byteLe y x = true)
    (trans : ∀ x y z, byteLe x y = true → byteLe y z = true → byteLe x z = true)
    (antisymm : ∀ x y, byteLe x y = true → byteLe y x = true → x = y)
    {r₁ r₂ : RecordList} (h : r₁ ~ r₂) :
    canonicalRecords byteLe r₁ = canonicalRecords byteLe r₂ :=
  isort_perm_invariant byteLe total trans antisymm h

/-- LAW 1 (field commitment, PROVED): `localNodeDigest` sorts the per-dimension
fields by `(name, value)` before committing them, so two field lists that are
permutations of each other commit to the same sorted field list. Stated over the
abstract field comparator (the engine's `fieldLt` is a strict total order; the
canonicalization argument is comparator-agnostic). -/
theorem determinism_fields
    (fieldLe : Field → Field → Bool)
    (total : ∀ x y, fieldLe x y = true ∨ fieldLe y x = true)
    (trans : ∀ x y z, fieldLe x y = true → fieldLe y z = true → fieldLe x z = true)
    (antisymm : ∀ x y, fieldLe x y = true → fieldLe y x = true → x = y)
    {f₁ f₂ : List Field} (h : f₁ ~ f₂) :
    isort fieldLe f₁ = isort fieldLe f₂ :=
  isort_perm_invariant fieldLe total trans antisymm h

/-- LAW 1 (node commitment, PROVED): `IndexedGraph.build` sorts nodes by canonical
key and assigns `u32` ids in that order, so the node ordering, and every id
derived from it, is a function of the key multiset alone. Renumbering or
reordering the input node list cannot change the sorted-by-key order. Stated over
the abstract key comparator. -/
theorem determinism_nodes
    (keyLe : Node → Node → Bool)
    (total : ∀ x y, keyLe x y = true ∨ keyLe y x = true)
    (trans : ∀ x y z, keyLe x y = true → keyLe y z = true → keyLe x z = true)
    (antisymm : ∀ x y, keyLe x y = true → keyLe y x = true → x = y)
    {n₁ n₂ : List Node} (h : n₁ ~ n₂) :
    isort keyLe n₁ = isort keyLe n₂ :=
  isort_perm_invariant keyLe total trans antisymm h

/-! ##############################################################
    ## Law 2: FACET REFINEMENT LATTICE
    ##############################################################

Equal at a finer facet implies equal at every coarser one. A facet address over a
dimension set `D` commits the sorted per-dimension digests for exactly the
dimensions in `D` (see `FacetAddress.addressDigest`: it iterates `a.digests`,
which `facetAddress` builds in `facet.dimensions` order, i.e. the canonical
filter of `D`). So "equal at facet `D`" is "the per-dimension digest map agrees
pointwise on every dimension in `D`". If `D₂ ⊆ D₁` and two graphs agree on `D₁`,
they agree on `D₂` by restriction. This is the order-reversing (contravariant)
direction of the lattice: smaller dimension set = coarser facet = less
information committed.

We work directly with the per-dimension digest map `Hash.DimDigests` and the
projection onto a dimension list. -/

open Hash (DimDigests)

/-- Equality of two per-dimension digest maps "at a facet over dimension set
`dims`": they agree (as `Option Digest`) on every dimension in `dims`. This is
exactly what `facetAddress` then `addressDigest` commits, dimension by dimension,
so it is the content of "equal at this facet" before the hash. -/
def EqAtFacet (dims : List Dimension) (m₁ m₂ : DimDigests) : Prop :=
  ∀ d ∈ dims, m₁.get d = m₂.get d

/-- LAW 2 (PROVED): finer-implies-coarser. If two digest maps are equal at the
finer facet over `larger`, and `smaller ⊆ larger`, then they are equal at the
coarser facet over `smaller`. Equality at the larger dimension set forces
equality at every subset. -/
theorem facet_refinement
    {larger smaller : List Dimension} (hsub : ∀ d ∈ smaller, d ∈ larger)
    {m₁ m₂ : DimDigests} (h : EqAtFacet larger m₁ m₂) :
    EqAtFacet smaller m₁ m₂ := by
  intro d hd
  exact h d (hsub d hd)

/-- LAW 2 (corollary, PROVED): if the maps are equal at `larger`, every facet
address built by projecting onto a subset `smaller` has *pointwise equal*
per-dimension digest entries on both sides, so the inputs to `addressDigest`
coincide. We state this as equality of the projected digest lists (the exact
list `addressDigest` folds over). -/
theorem facet_refinement_projection
    {larger smaller : List Dimension} (hsub : ∀ d ∈ smaller, d ∈ larger)
    {m₁ m₂ : DimDigests} (h : EqAtFacet larger m₁ m₂) :
    smaller.map (fun d => (d, m₁.get d)) = smaller.map (fun d => (d, m₂.get d)) := by
  apply List.map_congr_left
  intro d hd
  rw [facet_refinement hsub h d hd]

/-! ##############################################################
    ## Law 3: ANCHOR / TRANSPORT soundness as the quotient universal property
    ##############################################################

A fact rides facet `F` iff `~_F` is contained in `ker(fact)`: a function out of
the facet quotient exists iff it respects the facet congruence. This is the
universal property of `Quot`. We model the facet congruence `~_F` over a carrier
of "facet observations" (the per-dimension digest map restricted to `F`), and the
facet address is the normalization onto the quotient.

`~_F a b` := the two carriers agree at every dimension of `F` (i.e. `EqAtFacet`,
which is exactly the content `addressDigest` commits). The facet quotient is
`Quot (FacetRel F)`; the address is `Quot.mk`. A "fact that rides `F`" is any
`fact : Carrier → β`. Law 3 says: `fact` factors through the quotient iff it
respects `~_F`. We prove BOTH directions. -/

/-- The carrier of facet observations: a per-dimension digest map. -/
abbrev Carrier := DimDigests

/-- The facet congruence `~_F`: agreement on every dimension of `F`. This is the
relation the address quotients by; two carriers are `F`-indistinguishable iff
their `F`-projected digests coincide. -/
def FacetRel (F : List Dimension) (a b : Carrier) : Prop := EqAtFacet F a b

/-- The facet quotient: carriers up to `F`-indistinguishability. The address (the
normalization) is `Quot.mk (FacetRel F)`. -/
abbrev FacetQuot (F : List Dimension) := Quot (FacetRel F)

/-- The address / normalization map onto the facet quotient. -/
def address (F : List Dimension) (a : Carrier) : FacetQuot F := Quot.mk (FacetRel F) a

/-- LAW 3 (sufficiency, PROVED): a fact that respects `~_F` (i.e. `~_F ⊆ ker fact`)
rides the facet, meaning it factors uniquely through the quotient. The factoring
map is `Quot.lift`, and recovering the original fact from it holds by `rfl`. This
is the "transport is sound" direction: an `F`-respecting fact transports along
the address. -/
theorem transport_factors {β : Type} (F : List Dimension)
    (fact : Carrier → β) (h : ∀ a b, FacetRel F a b → fact a = fact b) :
    ∀ a, (Quot.lift fact h) (address F a) = fact a :=
  fun _ => rfl

/-- LAW 3 (necessity, PROVED): conversely, if any function `g` out of the facet
quotient recovers `fact` along the address (`g (address F a) = fact a`), then
`fact` must respect `~_F`. So "rides `F`" (factors through the quotient) is
*equivalent* to "`~_F ⊆ ker fact`", not merely implied by it. -/
theorem transport_respects {β : Type} (F : List Dimension)
    (fact : Carrier → β) (g : FacetQuot F → β)
    (hg : ∀ a, g (address F a) = fact a) :
    ∀ a b, FacetRel F a b → fact a = fact b := by
  intro a b hrel
  have : address F a = address F b := Quot.sound hrel
  calc fact a = g (address F a) := (hg a).symm
    _ = g (address F b) := by rw [this]
    _ = fact b := hg b

/-! ##############################################################
    ## Law 4: AnonymousShape correctness (names never enter the digest)
    ##############################################################

Renaming node keys by an injection leaves every AnonymousShape digest unchanged:
in `ViewMode.anonymousShape`, no `NodeKey` and no graph key is ever pushed into a
hash payload (see `localNodeDigest`, `graphDigestForDimension`, the
`node.component_context` record: the `pushNodeKey`/`pushStr graphKey` calls are
all guarded by `policy.viewMode == ViewMode.identityBound`). Keys then only order
traversal (via `IndexedGraph.build`), and an injective renaming is a relabeling
of that traversal, which the sorts re-canonicalize. So the digest depends only on
the shape, not the names.

This is the deepest target: a full proof must show the *entire* pipeline
(`build`, `local`, the fold, `graph.full`) is key-free in anonymous mode, then
that an injective key renaming induces a permutation of the sorted-by-key order
that the digest is invariant under (Law 1's renumbering clause feeds this).

We make the statement precise with a key-renaming over a graph, then state the
invariance as a TARGET axiom. -/

/-- Apply a key renaming `ρ` to every key position in a graph (node keys, child
endpoints, edge endpoints, and the graph key's owner). Field/kind/label *names*
are not keys and are untouched; this renames identity only. -/
def renameNodeKey (ρ : NodeKey → NodeKey) : NodeKey → NodeKey := ρ

def renameNode (ρ : NodeKey → NodeKey) (n : Node) : Node :=
  { n with key := ρ n.key }

def renameChild (ρ : NodeKey → NodeKey) (c : ChildEdge) : ChildEdge :=
  { c with parent := ρ c.parent, child := ρ c.child }

def renameEdge (ρ : NodeKey → NodeKey) (e : Edge) : Edge :=
  { e with source := ρ e.source, target := ρ e.target }

/-- Rename every key in a graph by `ρ`. The graph key is left abstract here (it
does not enter an anonymous-mode digest); callers supply the matching renamed
key. -/
def renameKeys (ρ : NodeKey → NodeKey) (gk : GraphKey) (g : Graph) : Graph :=
  { graphKey := gk
    nodes := g.nodes.map (renameNode ρ)
    children := g.children.map (renameChild ρ)
    edges := g.edges.map (renameEdge ρ) }

/-- An injective renaming on the canonical-key strings (the only thing the engine
ever compares keys by). -/
def KeyInjective (ρ : NodeKey → NodeKey) : Prop :=
  ∀ a b, (ρ a).canonicalKey = (ρ b).canonicalKey → a.canonicalKey = b.canonicalKey

/-- LAW 4 (TARGET, axiom): in AnonymousShape mode, an injective key renaming
leaves every per-dimension `graph.full` digest unchanged. Names never enter the
digest, so the result depends only on the shape.

TODO(phase2): prove. Requires (i) a lemma that no payload pusher fires a key
write when `viewMode = anonymousShape`, threaded through `local`/fold/`graph.full`,
and (ii) that an injective renaming induces a permutation of the sorted-by-key
node order that Law 1 (renumbering invariance) absorbs. Flagged as the deepest
law by the conceptual spike. -/
axiom anonymousShape_name_blind
    (policy : HashPolicy) (gk gk' : GraphKey) (g : Graph)
    (dims : List Dimension) (ρ : NodeKey → NodeKey)
    (hanon : policy.viewMode = ViewMode.anonymousShape)
    (hinj : KeyInjective ρ) :
    (Hash.digestGraph policy gk' (renameKeys ρ gk' g) dims).map (·.graph)
      = (Hash.digestGraph policy gk g dims).map (·.graph)

/-! ##############################################################
    ## Law 5: SCC / WL termination + the cyclic-case spec
    ##############################################################

`CondenseScc` is well-defined and WL refinement terminates. Termination is by
monotone partition refinement capped at `|members|` rounds (the partition only
ever splits, so the distinct-class count is non-decreasing and bounded by the
member count; `refine` stops when it is stable, with fuel = `members.size`).

HONEST CAVEAT (stated, not hidden): the only approximation inside an SCC is the
1-WL coloring. 1-WL is *sound* (WL-equivalent members get the same color, so the
digest cannot distinguish graphs that are genuinely WL-equivalent) but
*iso-incomplete* (there exist non-isomorphic graphs that 1-WL cannot tell apart,
so the digest may equate them). The honest correctness statement is therefore
WL-equivalence, NOT graph isomorphism. We do not claim the digest separates all
non-isomorphic SCCs.

We state termination and the soundness direction as TARGETs. -/

/-- The refinement step over a color map, abstracted to its partition content: a
"coloring" is a map from members to color classes (here, indices). `refine`
applies a step that can only split classes, never merge them. -/
abbrev MemberColoring := UInt32 → Nat

/-- A coloring `c'` *refines* `c` if same `c'`-color implies same `c`-color
(`c'` distinguishes at least as much). WL rounds always refine. -/
def Refines (members : List UInt32) (c' c : MemberColoring) : Prop :=
  ∀ a ∈ members, ∀ b ∈ members, c' a = c' b → c a = c b

/-- Iterate `f` `n` times (local definition; Lean core has no `Function.iterate`
without extra imports). -/
def iter {β : Type} (f : β → β) : Nat → β → β
  | 0,     x => x
  | n + 1, x => iter f n (f x)

/-- LAW 5a (TARGET, axiom): WL refinement reaches a fixpoint within `|members|`
rounds. The class count is non-decreasing under `Refines` and bounded by
`members.length`, so a strictly-refining chain has length at most `members.length`;
`refine`'s fuel cap of `members.size` is sufficient and the loop terminates at a
stable coloring.

TODO(phase2): prove. Requires a measure (the distinct-class count) shown to
strictly increase on every non-stable round and to be bounded by `members.length`,
giving termination in `≤ |members|` rounds. The Lean `refine` already carries this
fuel; this law certifies the cap is not lossy. -/
axiom wl_refine_terminates
    (members : List UInt32)
    (step : MemberColoring → MemberColoring) (c₀ : MemberColoring)
    (hrefine : ∀ c, Refines members (step c) c) :
    ∃ rounds : Nat, rounds ≤ members.length ∧
      step (iter step rounds c₀) = iter step rounds c₀

/-- LAW 5b (TARGET, axiom): SOUNDNESS of the 1-WL coloring (the honest, non-iso
statement). The component digest depends only on the *multiset* of final colors
and quotient edges, not on member order: WL-equivalent members (same final
color) are interchangeable. Concretely, `componentDigest` is invariant under
permuting the `members` array, because it sorts the colors (`byteLt`) before
committing. This is the property the engine actually guarantees; iso-completeness
is explicitly NOT claimed (1-WL is iso-incomplete, so distinct non-isomorphic
SCCs may share a digest).

TODO(phase2): prove. Reduces to Law 1's canonicalization applied to the color
multiset: permuting `members` permutes the pre-sort color list, and the sort
canonicalizes it, so the `wl.component` payload (count + sorted colors + sorted
quotient edges) is unchanged. -/
axiom wl_coloring_sound
    (policy : HashPolicy) (dimension : Dimension)
    (members₁ members₂ : Array UInt32) (internal : Array Hash.InternalEdge)
    (color : Hash.ColorMap)
    (hperm : members₁.toList ~ members₂.toList) :
    (Hash.componentDigest policy dimension members₁ internal color).map (·.toHex)
      = (Hash.componentDigest policy dimension members₂ internal color).map (·.toHex)

/-! ##############################################################
    ## Law 6: ENCODING INJECTIVITY (modulo hash collisions)
    ##############################################################

The canonical byte encoding is injective: distinct graphs at a facet produce
distinct pre-image byte sequences. The reason is the domain-separation discipline
in `Encode` (invariant I1): every record is `push_str`-framed (a `u64`
length-prefix before each variable-length field), every value carries a tag
string, every record carries a record tag and the full policy context, and
`pushSortedRecords` prefixes a `u32` count. Length prefixes + tags make the
encoding prefix-free and self-delimiting, so the byte stream parses back to a
unique structure: encoding is injective on well-formed inputs.

Therefore distinct shapes give distinct pre-image bytes, and distinct digests
imply distinct shapes UNLESS BLAKE3 collides. BLAKE3 collision-resistance stays
an explicit axiom, kept separate from the (provable, but here targeted) encoding
injectivity. -/

/-- BLAKE3 collision-resistance, as an explicit AXIOM (never a theorem). This is
the one named cryptographic assumption the whole lockstep rests on, stated here
so it is auditable and isolated. Equal digests with distinct pre-images would be
a collision; we assume none. -/
axiom blake3_collision_free :
    ∀ a b : ByteArray,
      (Encode.digestBytes a).toHex = (Encode.digestBytes b).toHex → a.toList = b.toList

/-- LAW 6 (TARGET, axiom): the canonical record encoding `recordBytes` is
injective in its payload-builder content for a fixed `(policy, dimension, tag)`:
distinct payloads produce distinct pre-hash bytes, because the header is a fixed
length-prefixed prefix and the payload is appended verbatim.

TODO(phase2): prove the structural injectivity of `pushStr`/`pushU32`/`pushU64`/
`pushValue`/`pushSortedRecords` (length prefixes + tags ⇒ unique parse), then
compose with `blake3_collision_free` to conclude distinct digests imply distinct
shapes. The composition (distinct shapes ⇒ distinct digests) is the engine-facing
corollary. -/
axiom encoding_injective
    (policy : HashPolicy) (dimension : Dimension) (tag : String)
    (p₁ p₂ : Encode.Builder → Encode.Builder)
    (hne : (Encode.recordBytes policy dimension tag p₁).toList
            ≠ (Encode.recordBytes policy dimension tag p₂).toList) :
    -- distinct pre-image bytes ⇒ distinct digests, unless blake3 collides:
    (Encode.digestBytes (Encode.recordBytes policy dimension tag p₁)).toHex
      ≠ (Encode.digestBytes (Encode.recordBytes policy dimension tag p₂)).toHex

end Laws
end Riffcat
