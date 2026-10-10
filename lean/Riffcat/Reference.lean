/-
Self-describing references, mirroring `crates/riff-catalog-core/src/reference.rs`.

Mirrors: Facet, facet_id, AddressValue, FacetAddress::new / ::joint +
address_digest, the combined projection of graph-level digests onto a facet
(`GraphHashes::needs_joint`, `GraphHashes::facet_address`), and the free
`facet_address` that falls back to the joint pass.

`dimensions` is a sorted set in Rust (`BTreeSet<Dimension>` iterated in
`Dimension` order); we keep a canonical-ordered list (filter `Dimension.all`).
-/

import Riffcat.Schema
import Riffcat.Policy
import Riffcat.Encode
import Riffcat.Hash
import Riffcat.Hash.Types

namespace Riffcat

open Encode

/-- A facet: a policy id plus the dimension subset compared on. Stored in
canonical `Dimension` order. -/
structure Facet where
  policyId : Digest
  dimensions : List Dimension
deriving Inhabited

namespace Facet

private def canonicalize (ds : List Dimension) : List Dimension :=
  Dimension.all.filter (fun d => ds.any (fun e => decide (e = d)))

def new (policyId : Digest) (dimensions : List Dimension) : Except String Facet :=
  let ds := canonicalize dimensions
  if ds.isEmpty then .error "a facet must include at least one dimension"
  else .ok { policyId := policyId, dimensions := ds }

/-- All five dimensions: the finest facet. -/
def full (policyId : Digest) : Facet :=
  { policyId := policyId, dimensions := Dimension.all }

/-- Everything except Names. -/
def namesBlind (policyId : Digest) : Facet :=
  { policyId := policyId, dimensions := Dimension.all.filter (fun d => d != Dimension.names) }

/-- Structure only. -/
def structureOnly (policyId : Digest) : Facet :=
  { policyId := policyId, dimensions := [Dimension.structure_] }

def facetId (f : Facet) : Digest :=
  digestMeta "riffcat.facet" fun b =>
    Id.run do
      let mut b := pushDigest b f.policyId
      b := pushU32 b (UInt32.ofNat f.dimensions.length)
      for d in f.dimensions do
        b := pushStr b d.asStr
      return b

end Facet

/-- The value an address is computed from. Mirrors `AddressValue`.
`combined` holds the per-dimension digests in canonical `Dimension` order,
matching the `BTreeMap` iteration in `address_digest`; `joint` holds one
joint-pass digest. -/
inductive AddressValue where
  | combined (digests : List (Dimension × Digest))
  | joint (digest : Digest)
deriving Inhabited

/-- A graph's address at a facet: equal addresses mean the graphs are the same
once only the facet's facts are kept. -/
structure FacetAddress where
  facet : Facet
  value : AddressValue
deriving Inhabited

namespace FacetAddress

/-- A combined address from the facet's per-dimension digests. Mirrors
`FacetAddress::new`. -/
def new (facet : Facet) (digests : List (Dimension × Digest)) : Except String FacetAddress :=
  let keys := (digests.map (·.1))
  -- exact dimension-set match (as sets, both canonical-ordered here)
  if (Dimension.all.filter (fun d => keys.any (fun e => decide (e = d)))) == facet.dimensions then
    .ok { facet := facet, value := .combined digests }
  else
    .error "facet address dimensions do not match the facet's dimension set"

/-- A joint address from one joint-pass digest. Mirrors `FacetAddress::joint`. -/
def joint (facet : Facet) (digest : Digest) : FacetAddress :=
  { facet := facet, value := .joint digest }

/-- The digest that defines "equal at this facet". Combined and joint addresses
are hashed under different tags, so they never coincide. -/
def addressDigest (a : FacetAddress) : Digest :=
  match a.value with
  | .combined digests =>
    digestMeta "riffcat.facet_address" fun b =>
      Id.run do
        let mut b := pushDigest b a.facet.facetId
        for (dimension, digest) in digests do
          b := pushStr b dimension.asStr
          b := pushDigest b digest
        return b
  | .joint digest =>
    digestMeta "riffcat.facet_address.joint" fun b =>
      let b := pushDigest b a.facet.facetId
      pushDigest b digest

end FacetAddress

/-- The error text of `CatalogError::FacetNeedsJointPass`. -/
def Hash.facetNeedsJointPass : String :=
  "this graph's address at the facet needs the joint pass; pass the graph"

/-- Whether this graph's address at `facet` needs the joint pass. Mirrors
`GraphHashes::needs_joint`. -/
def Hash.GraphHashes.needsJoint (gh : Hash.GraphHashes) (facet : Facet) : Bool :=
  Hash.maskNeedsJoint gh.mixedPairs facet.dimensions

/-- The combined address at a facet. Refuses a facet that needs the joint pass,
since combining would over-match there; use `Hash.facetAddress` with the graph
instead. Mirrors `GraphHashes::facet_address`. -/
def Hash.GraphHashes.facetAddress (gh : Hash.GraphHashes) (facet : Facet)
    : Except String FacetAddress := do
  unless facet.policyId == gh.policyId do
    .error "index policy mismatch"
  if gh.needsJoint facet then
    .error Hash.facetNeedsJointPass
  let mut digests : List (Dimension × Digest) := []
  for dimension in facet.dimensions do
    match gh.graph.get dimension with
    | some d => digests := digests ++ [(dimension, d)]
    | none => .error s!"graph hashes are missing dimension {dimension.asStr}"
  FacetAddress.new facet digests

/-- The address of a graph at a facet: combined from the stored per-dimension
digests where that is exact, otherwise from the joint pass. Mirrors the free
function `facet_address`. -/
def Hash.facetAddress (policy : HashPolicy) (g : Graph) (gh : Hash.GraphHashes)
    (facet : Facet) : Except String FacetAddress := do
  if facet.policyId != gh.policyId || !gh.needsJoint facet then
    return ← gh.facetAddress facet
  return FacetAddress.joint facet (← Hash.jointDigest policy g facet.dimensions)

end Riffcat
