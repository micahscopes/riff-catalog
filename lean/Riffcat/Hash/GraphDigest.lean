/-
Final `graph.full` assembly per dimension, mirroring
`crates/riff-catalog-core/src/hash/graph_digest.rs`.

Flat edges (every role except Origin) fold through every dimension using that
dimension's endpoint digests. Role and label are Structure-only payload (carried
in every dimension's edge record though, since they define topology). Node keys
ride along in identity mode.

  payload (graph.full):
    if identity: graph key (str)
    node count (u32), then per node entry (sorted by key in identity, by digest
      in anonymous):
        if identity: node key (str)
        node final digest (raw 32)
    sorted edge-record multiset (u32 count + concat), each edge record:
        if identity: source key (str), target key (str)
        role (str), label (str), src final digest, dst final digest
-/

import Riffcat.Hash.View
import Riffcat.Hash.Types
import Riffcat.Policy
import Riffcat.Encode

namespace Riffcat
namespace Hash

open Encode View

private structure NodeEntry where
  id : UInt32
  digest : Digest

def graphDigestForDimension (policy : HashPolicy) (g : Graph) (v : IndexedGraph)
    (finalDigests : Array DimDigests) (dimension : Dimension) : Except String Digest := do
  let digestOf (id : UInt32) : Digest := (finalDigests[id.toNat]!).getD dimension
  -- node entries
  let entries : Array NodeEntry := (Array.range v.len).map (fun id =>
    { id := UInt32.ofNat id, digest := digestOf (UInt32.ofNat id) })
  let sortedNodes :=
    match policy.viewMode with
    | .identityBound =>
        entries.qsort (fun a b =>
          (v.key a.id.toNat).canonicalKey < (v.key b.id.toNat).canonicalKey)
    | .anonymousShape =>
        entries.qsort (fun a b => byteLt a.digest.bytes b.digest.bytes)
  -- edge records (skip Origin)
  let edgeRecords : List ByteArray :=
    (g.edges.filter (fun e => e.role != EdgeRole.origin)).map (fun e =>
      let src := (v.idOf e.source).getD 0
      let dst := (v.idOf e.target).getD 0
      let r := empty
      let r := if policy.viewMode == ViewMode.identityBound then
                 let r := pushNodeKey r e.source
                 pushNodeKey r e.target
               else r
      let r := pushStr r e.role.asStr
      let r := pushStr r e.label.asStr
      let r := pushDigest r (digestOf src)
      pushDigest r (digestOf dst))
  digestRecord policy dimension "graph.full" fun b =>
    Id.run do
      let mut b := b
      if policy.viewMode == ViewMode.identityBound then
        b := pushStr b g.graphKey.canonicalKey
      b := pushU32 b (UInt32.ofNat sortedNodes.size)
      for e in sortedNodes do
        if policy.viewMode == ViewMode.identityBound then
          b := pushNodeKey b (v.key e.id.toNat)
        b := pushDigest b e.digest
      b := pushSortedRecords b edgeRecords
      return b

end Hash
end Riffcat
