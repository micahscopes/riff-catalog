/-
Cycle checks and the ordered-children Merkle fold, mirroring
`crates/riff-catalog-core/src/hash/acyclic.rs`.

The Rust code is an explicit iterative post-order walk (to survive deep ASTs);
semantically it is a structural recursion over the children DAG, which is how we
write it here (memoized, fuel-bounded by the node count). The byte output of
`node.tree` is reproduced exactly:

  payload = local-digest, child-count (u32), then per child entry:
    if dimension == Structure: ordinal (u32) + label (str)
    child tree-digest (raw 32)

Child entry ordering:
  - IdentityBound : (ordinal, label) then child canonical key
  - AnonymousShape: (ordinal, label, child digest)

`check_acyclic` mirrors the DFS cycle detector: `includeDependency` selects
between Reject (children + Dependency) and NonRecursiveGraphEdges (children
only, deduped successor set).
-/

import Riffcat.Hash.View
import Riffcat.Hash.Types
import Riffcat.Hash.Local
import Riffcat.Policy
import Riffcat.Encode

namespace Riffcat
namespace Hash

open Encode View

/-! ## Cycle check -/

/-- Successor lists used by the cycle check. For Reject we use the full
recursive successors; for NonRecursiveGraphEdges we use the child edges only,
sorted + deduped (matching the Rust branch). -/
private def cycleSucc (v : IndexedGraph) (includeDependency : Bool) : Array (Array UInt32) :=
  if includeDependency then
    v.recursiveSucc
  else
    Id.run do
      let mut out : Array (Array UInt32) := #[]
      for id in [0:v.len] do
        let raw := v.children[id]!.map (fun c => c.child)
        let sorted := raw.qsort (· < ·)
        let mut dedup : Array UInt32 := #[]
        for x in sorted do
          if dedup.isEmpty || dedup.back! != x then dedup := dedup.push x
        out := out.push dedup
      return out

/-- DFS over `succ` from each unvisited root; returns the id of a node on the
current path that is revisited (a cycle witness), else none. Three-color marks
matching the Rust Unvisited/Visiting/Done. Fuel-bounded by total edge count. -/
partial def detectCycle (succ : Array (Array UInt32)) : Option Nat :=
  Id.run do
    let n := succ.size
    -- 0 = unvisited, 1 = visiting, 2 = done
    let mut marks : Array Nat := Array.replicate n 0
    for root in [0:n] do
      if marks[root]! == 0 then
        -- explicit stack of (node, nextSuccPos)
        let mut stack : Array (Nat × Nat) := #[(root, 0)]
        while _h : stack.size > 0 do
          let (node, pos) := stack[stack.size - 1]!
          if pos == 0 then
            if marks[node]! == 2 then
              stack := stack.pop
              continue
            marks := marks.set! node 1
          if pos < succ[node]!.size then
            let next := (succ[node]![pos]!).toNat
            stack := stack.set! (stack.size - 1) (node, pos + 1)
            if marks[next]! == 1 then
              return some next
            else if marks[next]! == 0 then
              stack := stack.push (next, 0)
          else
            marks := marks.set! node 2
            stack := stack.pop
    return none

def checkAcyclic (v : IndexedGraph) (includeDependency : Bool) : Except String Unit :=
  match detectCycle (cycleSucc v includeDependency) with
  | some id => .error s!"graph contains a cycle at {v.key id |>.canonicalKey}"
  | none => .ok ()

/-! ## The Merkle fold -/

/-- A child entry at payload time: (ordinal, label, child digest, child id). -/
private structure Entry where
  ordinal : UInt32
  label : String
  digest : Digest
  childId : UInt32

private def entryLtIdentity (v : IndexedGraph) (a b : Entry) : Bool :=
  if a.ordinal != b.ordinal then a.ordinal < b.ordinal
  else if a.label != b.label then a.label < b.label
  else (v.key a.childId.toNat).canonicalKey < (v.key b.childId.toNat).canonicalKey

private def entryLtAnon (a b : Entry) : Bool :=
  if a.ordinal != b.ordinal then a.ordinal < b.ordinal
  else if a.label != b.label then a.label < b.label
  else byteLt a.digest.bytes b.digest.bytes

/-- The `node.tree` digest of node `id` for one dimension, given the tree digests
of its children. -/
private def nodeTreeDigest (policy : HashPolicy) (v : IndexedGraph)
    (localDigs : Array DimDigests) (childTree : Array DimDigests)
    (id : Nat) (dimension : Dimension) : Except String Digest := do
  let entries : Array Entry := v.children[id]!.map (fun c =>
    { ordinal := c.ordinal, label := c.label,
      digest := (childTree[c.child.toNat]!).getD dimension,
      childId := c.child })
  let sorted :=
    match policy.viewMode with
    | .identityBound  => entries.qsort (entryLtIdentity v)
    | .anonymousShape => entries.qsort entryLtAnon
  digestRecord policy dimension "node.tree" fun b =>
    Id.run do
      let mut b := pushDigest b ((localDigs[id]!).getD dimension)
      b := pushU32 b (UInt32.ofNat sorted.size)
      for e in sorted do
        if dimension == Dimension.structure_ then
          b := pushU32 b e.ordinal
          b := pushStr b e.label
        b := pushDigest b e.digest
      return b

/-- Tree digests for every node across all dimensions. Memoized structural
recursion over the children DAG; fuel = node count (the DAG depth bound on an
acyclic graph). -/
def treeDigests (policy : HashPolicy) (v : IndexedGraph) (dimensions : List Dimension)
    (localDigs : Array DimDigests) : Except String (Array DimDigests) := do
  let n := v.len
  -- compute one node's full DimDigests given children already done
  let computeNode (tree : Array (Option DimDigests)) (id : Nat) : Except String DimDigests :=
    dimensions.mapM (fun d => do
      -- materialize the children's DimDigests (all present by recursion order)
      let childTree : Array DimDigests := (Array.range n).map (fun i =>
        match tree[i]! with | some dd => dd | none => [])
      let dig ← nodeTreeDigest policy v localDigs childTree id d
      return (d, dig))
  -- recursive resolver with fuel
  let rec resolve (tree : Array (Option DimDigests)) (id : Nat) (fuel : Nat)
      : Except String (Array (Option DimDigests)) := do
    match tree[id]! with
    | some _ => return tree
    | none =>
      match fuel with
      | 0 => .error "tree fold exceeded fuel (cycle?)"
      | fuel + 1 =>
        -- resolve children first
        let mut tree := tree
        for c in v.children[id]! do
          tree ← resolve tree c.child.toNat fuel
        let dd ← computeNode tree id
        return tree.set! id (some dd)
  let mut tree : Array (Option DimDigests) := Array.replicate n none
  for id in [0:n] do
    tree ← resolve tree id n
  return tree.map (fun o => o.getD [])

end Hash
end Riffcat
