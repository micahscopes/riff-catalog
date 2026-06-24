/-
Per-node local digests, mirroring `crates/riff-catalog-core/src/hash/local.rs`.

`local_node_digests`: for each requested dimension, a `node.local` record over
the node's key (identity mode only), kind (Structure only), and this dimension's
fields sorted by (name, value).
-/

import Riffcat.Schema
import Riffcat.Policy
import Riffcat.Encode

namespace Riffcat
namespace Hash

open Encode

/-- Sort fields by (name, value), matching the Rust `sort_by` on
`(name.as_str(), &value)`. -/
private def fieldLt (a b : Field) : Bool :=
  if a.name.asStr != b.name.asStr then a.name.asStr < b.name.asStr
  else Value.lt a.value b.value

/-- One node's `node.local` digest for a single dimension. -/
def localNodeDigest (policy : HashPolicy) (node : Node) (dimension : Dimension)
    : Except String Digest :=
  digestRecord policy dimension "node.local" fun b =>
    Id.run do
      let mut b := b
      if policy.viewMode == ViewMode.identityBound then
        b := pushNodeKey b node.key
      if dimension == Dimension.structure_ then
        b := pushStr b node.kind.asStr
      let fields := (node.fields.filter (fun f => f.dimension == dimension)).toArray.qsort fieldLt
      b := pushU32 b (UInt32.ofNat fields.size)
      for f in fields do
        b := pushStr b f.name.asStr
        b := pushValue b f.value
      return b

/-- Local digests for one node across all requested dimensions, as an
association list keyed by dimension (in `dimensions` order, which is the
canonical `Dimension` order). -/
def localNodeDigests (policy : HashPolicy) (node : Node) (dimensions : List Dimension)
    : Except String (List (Dimension × Digest)) :=
  dimensions.mapM (fun d => do
    let dig ← localNodeDigest policy node d
    return (d, dig))

end Hash
end Riffcat
