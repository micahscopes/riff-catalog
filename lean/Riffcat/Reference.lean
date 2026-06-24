/-
Self-describing references, mirroring `crates/riff-catalog-core/src/reference.rs`.

Mirrors: Facet, facet_id, FacetAddress::new + address_digest, and the projection
of graph-level digests onto a facet (`GraphHashes::facet_address`).

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

/-- A graph's digests projected onto a facet. `digests` are in canonical
`Dimension` order, matching the `BTreeMap` iteration in `address_digest`. -/
structure FacetAddress where
  facet : Facet
  digests : List (Dimension × Digest)
deriving Inhabited

namespace FacetAddress

def new (facet : Facet) (digests : List (Dimension × Digest)) : Except String FacetAddress :=
  let keys := (digests.map (·.1))
  -- exact dimension-set match (as sets, both canonical-ordered here)
  if (Dimension.all.filter (fun d => keys.any (fun e => decide (e = d)))) == facet.dimensions then
    .ok { facet := facet, digests := digests }
  else
    .error "facet address dimensions do not match the facet's dimension set"

/-- The digest that defines "equal at this facet". -/
def addressDigest (a : FacetAddress) : Digest :=
  digestMeta "riffcat.facet_address" fun b =>
    Id.run do
      let mut b := pushDigest b a.facet.facetId
      for (dimension, digest) in a.digests do
        b := pushStr b dimension.asStr
        b := pushDigest b digest
      return b

end FacetAddress

/-- Project graph-level digests onto a facet (mirrors `GraphHashes::facet_address`). -/
def Hash.GraphHashes.facetAddress (gh : Hash.GraphHashes) (facet : Facet)
    : Except String FacetAddress := do
  unless facet.policyId == gh.policyId do
    .error "index policy mismatch"
  let mut digests : List (Dimension × Digest) := []
  for dimension in facet.dimensions do
    match gh.graph.get dimension with
    | some d => digests := digests ++ [(dimension, d)]
    | none => .error s!"graph hashes are missing dimension {dimension.asStr}"
  FacetAddress.new facet digests

end Riffcat
