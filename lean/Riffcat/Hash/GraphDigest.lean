/-
Final `graph.full` assembly per dimension, mirroring
`crates/riff-catalog-core/src/hash/graph_digest.rs`.

Colour refinement over the whole skeleton: every node starts from its final
per-node digest and repeatedly takes in its neighbours' colours along every
edge (child edges with label and ordinal; flat and Dependency edges with role
and label, ordinal 0; Origin edges skipped), in both directions, until the
number of colour classes stops growing (at most n rounds).

  graph.colour payload: own colour, sorted out-records, sorted in-records,
    each record: role (str), label (str), ordinal (u32), neighbour colour
  graph.full payload: node count (u32), sorted final colours (raw 32 each),
    sorted edge records: role, label, ordinal, source colour, target colour

Edge order never matters: every multiset is byte-sorted before hashing. The
graph key is not hashed.

The same refinement is shared (`refineAndDigest`) by the per-dimension digests,
which also return the final colour of every node, and by the bare shape
(`shapeDigest`): no facts and no kinds, every node starting from a `shape.init`
record in bin `shape` (carrying the node key only when keys are bound), ending
in `graph.full` in bin `shape`. The shape's colours tell which nodes the
skeleton alone cannot tell apart.
-/

import Riffcat.Hash.View
import Riffcat.Hash.Types
import Riffcat.Policy
import Riffcat.Encode

namespace Riffcat
namespace Hash

open Encode View

/-- One skeleton edge in id space. -/
private structure SkeletonEdge where
  role : String
  label : String
  ordinal : UInt32
  src : Nat
  dst : Nat

private def skeletonEdges (g : Graph) (v : IndexedGraph) : Array SkeletonEdge :=
  Id.run do
    let mut out : Array SkeletonEdge := #[]
    for parent in [0:v.len] do
      for c in v.children[parent]! do
        out := out.push { role := "child", label := c.label, ordinal := c.ordinal,
                          src := parent, dst := c.child.toNat }
    for e in g.edges do
      if e.role != EdgeRole.origin then
        out := out.push { role := e.role.asStr, label := e.label.asStr, ordinal := 0,
                          src := ((v.idOf e.source).getD 0).toNat,
                          dst := ((v.idOf e.target).getD 0).toNat }
    return out

private def edgeRecord (e : SkeletonEdge) (endColour : Digest) : ByteArray :=
  let r := empty
  let r := pushStr r e.role
  let r := pushStr r e.label
  let r := pushU32 r e.ordinal
  pushDigest r endColour

private def classCount (colours : Array Digest) : Nat :=
  (colours.toList.map (fun d => d.toHex)).eraseDups.length

/-- Refine colours to a fixpoint over the skeleton edges, both directions, then
hash the bag of colours and the edges written with colours. Returns the graph
digest and the final colour of every node. Mirrors `refine_and_digest`. -/
private def refineAndDigest (policy : HashPolicy) (bin : String)
    (edges : Array SkeletonEdge) (initial : Array Digest)
    : Except String (Digest × Array Digest) := do
  let n := initial.size
  let mut colours : Array Digest := initial
  let mut classes := classCount colours
  for _ in [0:n] do
    let mut next : Array Digest := #[]
    for id in [0:n] do
      let outRecords : List ByteArray :=
        (edges.toList.filter (fun e => e.src == id)).map (fun e => edgeRecord e colours[e.dst]!)
      let inRecords : List ByteArray :=
        (edges.toList.filter (fun e => e.dst == id)).map (fun e => edgeRecord e colours[e.src]!)
      let own := colours[id]!
      let d ← digestRecordIn policy bin "graph.colour" fun b =>
        let b := pushDigest b own
        let b := pushSortedRecords b outRecords
        pushSortedRecords b inRecords
      next := next.push d
    let nextClasses := classCount next
    colours := next
    if nextClasses == classes then
      break
    classes := nextClasses
  let sorted := colours.qsort (fun a b => byteLt a.bytes b.bytes)
  let edgeRecords : List ByteArray :=
    edges.toList.map (fun e => pushDigest (edgeRecord e colours[e.src]!) colours[e.dst]!)
  let digest ← digestRecordIn policy bin "graph.full" fun b =>
    Id.run do
      let mut b := pushU32 b (UInt32.ofNat sorted.size)
      for c in sorted do
        b := pushDigest b c
      return pushSortedRecords b edgeRecords
  return (digest, colours)

/-- `graph.full` for one dimension, and the final colour of every node. Mirrors
`graph_digest_for_dimension`. -/
def graphDigestForDimension (policy : HashPolicy) (g : Graph) (v : IndexedGraph)
    (finalDigests : Array DimDigests) (dimension : Dimension)
    : Except String (Digest × Array Digest) :=
  let colours : Array Digest :=
    (Array.range v.len).map (fun id => (finalDigests[id]!).getD dimension)
  refineAndDigest policy dimension.asStr (skeletonEdges g v) colours

/-- The bare shape: the same refinement with no facts and no kinds, only the
skeleton (and keys, with bound keys). Its digest names the coarsest atom, "same
shape, nothing else"; its colours tell which nodes the skeleton alone cannot tell
apart. Mirrors `shape_digest`. -/
def shapeDigest (policy : HashPolicy) (g : Graph) (v : IndexedGraph)
    : Except String (Digest × Array Digest) := do
  let mut colours : Array Digest := #[]
  for id in [0:v.len] do
    let d ← digestRecordIn policy "shape" "shape.init" fun b =>
      if policy.keys == Keys.bound then pushNodeKey b (v.key id) else b
    colours := colours.push d
  refineAndDigest policy "shape" (skeletonEdges g v) colours

end Hash
end Riffcat
