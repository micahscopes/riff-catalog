/-
The engine's laws, stated over the port's own types.

A law here is either a `theorem` with a proof, or a named proposition (`def ... :
Prop`) that says what is expected to hold and is not proved. A proposition
asserts nothing: it cannot make the development inconsistent, and anything that
needs it must take it as a hypothesis. There are no axioms in this file.

  Law 1  Order independence. PROVED. Sorting a list gives the same result for
         every permutation of it (`Riffcat.Laws.Sorting`); packaged here for
         the record, field and node lists the encoder sorts.
  Law 2  Facets refine. PROVED. Equal digests in every dimension of a facet give
         equal digests in every dimension of any facet contained in it.
  Law 3  Transport. PROVED, both directions. A fact can be read off a facet
         address exactly when the fact agrees on units the facet identifies.
  Law 4  Free keys. STATED (`FreeKeysRenameInvariant`). With free keys, renaming
         node keys by an injection changes no per-dimension graph digest.
  Law 5  Colour refinement. STATED (`RefinementStabilizes`,
         `ComponentDigestOrderFree`). Refinement stops splitting within as many
         rounds as there are members; a component's digest does not depend on
         the order its members are listed in. Colour refinement cannot tell
         every two different graphs apart, and no law here says it can.
  Law 6  Encoding. STATED (`HeaderSeparates`). The record header determines the
         schema version, recipe, bin, keys mode and tag.

`CollisionFreeOn` is the hash assumption, as a property of a set of inputs. It
is false of the set of all byte strings for any hash of fixed size, so it is
never assumed globally.
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
dimensions in `D` (see `FacetAddress.addressDigest`: for a combined address it
iterates the digest list that `facetAddress` builds in `facet.dimensions` order,
i.e. the canonical filter of `D`). So "equal at facet `D`" is "the per-dimension
digest map agrees pointwise on every dimension in `D`". If `D₂ ⊆ D₁` and two
graphs agree on `D₁`, they agree on `D₂` by restriction. This is the order-reversing (contravariant)
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
    ## Law 4: free keys (key renaming never moves a digest)
    ##############################################################

Renaming node keys by an injection leaves every free-keys digest unchanged: with
`Keys.free`, no `NodeKey` is ever pushed into a hash payload (see
`localNodeDigest` and the `node.component_context` record: the `pushNodeKey`
calls are guarded by `policy.keys == Keys.bound`), and the graph key is never
hashed. Keys then only order traversal (via `IndexedGraph.build`), and an
injective renaming is a relabeling of that traversal, which the sorts
re-canonicalize.

This is about keys, not the `names` dimension: `names` facts still count in a
free-keys digest. Forgetting names is the facet that leaves out `names`.

A full proof must show the whole pipeline (`build`, `local`, the fold, the
graph digest) writes no key with free keys, then
that an injective key renaming induces a permutation of the sorted-by-key order
that the digest is invariant under (Law 1's renumbering clause feeds this).

The statement is made precise with a key renaming over a graph. It is stated,
not proved. -/

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

/-- Rename every key in a graph by `ρ`. The graph key is supplied by the caller;
it is not hashed. -/
def renameKeys (ρ : NodeKey → NodeKey) (gk : GraphKey) (g : Graph) : Graph :=
  { graphKey := gk
    nodes := g.nodes.map (renameNode ρ)
    children := g.children.map (renameChild ρ)
    edges := g.edges.map (renameEdge ρ) }

/-- An injective renaming on the canonical-key strings (the only thing the engine
ever compares keys by). -/
def KeyInjective (ρ : NodeKey → NodeKey) : Prop :=
  ∀ a b, (ρ a).canonicalKey = (ρ b).canonicalKey → a.canonicalKey = b.canonicalKey

/-- LAW 4 (stated, not proved): with free keys, an injective key renaming leaves
every per-dimension graph digest unchanged. `gk` is the graph's own key and `gk'`
the renamed graph's key; neither is hashed.

A proof needs (i) that no key is written into a payload when `keys = free`,
through `local`, the fold and the graph digest, and (ii) that an injective
renaming permutes the sorted-by-key node order, which Law 1 absorbs. -/
def FreeKeysRenameInvariant : Prop :=
  ∀ (policy : HashPolicy) (gk gk' : GraphKey) (g : Graph)
    (dims : List Dimension) (ρ : NodeKey → NodeKey),
    policy.keys = Keys.free →
    KeyInjective ρ →
    g.graphKey = gk →
    (Hash.digestGraph policy gk' (renameKeys ρ gk' g) dims).map (·.graph)
      = (Hash.digestGraph policy gk g dims).map (·.graph)

/-! ##############################################################
    ## Law 5: colour refinement
    ##############################################################

Colour refinement gives every member of a strongly connected component a colour,
then repeatedly recolours each member by its own colour and its neighbours'
colours. A round can only split colour classes, never merge them. The number of
classes is at most the number of members, so the splitting stops within that
many rounds; `refine` runs with that many rounds of fuel.

Members that end with the same colour are treated as interchangeable. That is
the one approximation in the digest: there are different graphs that colour
refinement cannot tell apart, and they get equal digests. Nothing here claims
otherwise. -/

/-- A colouring of the members: a map from members to colour classes. -/
abbrev MemberColoring := UInt32 → Nat

/-- `c'` refines `c`: members that `c'` gives one colour, `c` gives one colour.
A refinement round always refines. -/
def Refines (members : List UInt32) (c' c : MemberColoring) : Prop :=
  ∀ a ∈ members, ∀ b ∈ members, c' a = c' b → c a = c b

/-- Two colourings split the members into the same classes. -/
def SamePartition (members : List UInt32) (c' c : MemberColoring) : Prop :=
  ∀ a ∈ members, ∀ b ∈ members, (c' a = c' b ↔ c a = c b)

/-- Iterate `f` `n` times. -/
def iter {β : Type} (f : β → β) : Nat → β → β
  | 0,     x => x
  | n + 1, x => iter f n (f x)

/-- LAW 5a (stated, not proved): a step that only ever splits classes leaves
the classes unchanged at some round no later than the number of members. The
colours themselves may keep changing; the classes are what the digest uses.

A proof counts classes: the count never drops under `Refines`, rises on every
round that changes the classes, and is at most `members.length`. -/
def RefinementStabilizes : Prop :=
  ∀ (members : List UInt32) (step : MemberColoring → MemberColoring)
    (c₀ : MemberColoring),
    (∀ c, Refines members (step c) c) →
    ∃ rounds : Nat, rounds ≤ members.length ∧
      SamePartition members (iter step (rounds + 1) c₀) (iter step rounds c₀)

/-- LAW 5b (stated, not proved): a component's digest does not depend on the
order its members are listed in, because the colours are sorted before they are
hashed.

A proof reduces to Law 1 applied to the colour list. -/
def ComponentDigestOrderFree : Prop :=
  ∀ (policy : HashPolicy) (dimension : Dimension)
    (members₁ members₂ : Array UInt32) (internal : Array Hash.InternalEdge)
    (color : Hash.ColorMap),
    members₁.toList ~ members₂.toList →
    (Hash.componentDigest policy dimension members₁ internal color).map (·.toHex)
      = (Hash.componentDigest policy dimension members₂ internal color).map (·.toHex)

/-! ##############################################################
    ## Law 6: the encoding, and the hash assumption
    ##############################################################

Every variable-length field in a record is written with its length first, every
value with a tag, and every record starts with a header naming the schema
version, recipe, bin, keys mode and record tag. So the bytes of a record can be
read back in one way only, and records of different kinds never share bytes.

Equal digests then mean equal records only if the hash does not collide on the
records in question. That is an assumption about a set of inputs. -/

/-- No two byte strings in `inputs` have the same digest. A statement that
needs collision freedom takes this as a hypothesis about the inputs it talks
about. Over all byte strings it is false for any hash of fixed size, so it is
never an axiom. -/
def CollisionFreeOn (inputs : ByteArray → Prop) : Prop :=
  ∀ a b : ByteArray, inputs a → inputs b →
    (Encode.digestBytes a).toHex = (Encode.digestBytes b).toHex → a.toList = b.toList

/-- LAW 6 (stated, not proved): the record header determines the schema
version, recipe, bin, keys mode and record tag. Two headers with the same bytes
were written for the same things.

A proof shows that `pushStr` (a length, then the bytes) and `pushU32` can each
be read back in one way, and reads the header field by field. -/
def HeaderSeparates : Prop :=
  ∀ (p p' : HashPolicy) (bin bin' tag tag' : String),
    (Encode.recordBytesIn p bin tag id).toList
      = (Encode.recordBytesIn p' bin' tag' id).toList →
    p.schemaVersion = p'.schemaVersion ∧ p.recipe.asStr = p'.recipe.asStr ∧
      bin = bin' ∧ p.keys.asStr = p'.keys.asStr ∧ tag = tag'

end Laws
end Riffcat
