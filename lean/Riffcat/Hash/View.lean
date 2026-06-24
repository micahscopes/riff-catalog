/-
Indexed view of a graph, mirroring `crates/riff-catalog-core/src/hash/view.rs`.

INVARIANT (I5): no `u32` id ever enters a hash payload. Ids order traversal;
digests and (in identity mode) canonical keys order payloads.

`build`:
  - nodes sorted by canonical key, ids assigned in that order;
  - children per parent, each sorted by (ordinal, label, child-id);
  - recursive edges = children (role "child", real ordinal) then Dependency
    edges (role "dependency", ordinal 0), in that construction order;
  - recursive_succ per node, sorted ascending and deduped.
-/

import Riffcat.Schema

namespace Riffcat
namespace View

/-- A child edge in id space (label/ordinal kept for payload-time ordering). -/
structure ChildRef where
  label : String
  ordinal : UInt32
  child : UInt32
deriving Inhabited

/-- A recursive edge: a child edge or a Dependency edge, in id space. -/
structure RecEdge where
  role : String
  label : String
  ordinal : UInt32
  src : UInt32
  dst : UInt32
deriving Inhabited

/-- The indexed view: parallel arrays indexed by `u32` node id. -/
structure IndexedGraph where
  keys : Array NodeKey
  nodes : Array Node
  children : Array (Array ChildRef)
  recursive : Array RecEdge
  recursiveSucc : Array (Array UInt32)
deriving Inhabited

def IndexedGraph.len (v : IndexedGraph) : Nat := v.keys.size
def IndexedGraph.key (v : IndexedGraph) (id : Nat) : NodeKey := v.keys[id]!
def IndexedGraph.node (v : IndexedGraph) (id : Nat) : Node := v.nodes[id]!

/-- Id of a key (linear scan over canonical keys, matching the BTreeMap lookup
semantics; graphs are small). -/
def IndexedGraph.idOf (v : IndexedGraph) (k : NodeKey) : Option UInt32 :=
  Id.run do
    for i in [0:v.keys.size] do
      if v.keys[i]!.canonicalKey == k.canonicalKey then
        return some (UInt32.ofNat i)
    return none

/-- Sort by canonical key (Rust `sort_by_key(canonical_key)`, a stable sort over
distinct keys). -/
private def sortNodesByKey (nodes : List Node) : Array Node :=
  (nodes.toArray).qsort (fun a b => a.key.canonicalKey < b.key.canonicalKey)

/-- Child traversal order: (ordinal, label) then child id. -/
private def childLt (a b : ChildRef) : Bool :=
  if a.ordinal != b.ordinal then a.ordinal < b.ordinal
  else if a.label != b.label then a.label < b.label
  else a.child < b.child

/-- Ascending u32 sort + dedup of a successor list. -/
private def sortDedup (xs : Array UInt32) : Array UInt32 :=
  Id.run do
    let sorted := xs.qsort (· < ·)
    let mut out : Array UInt32 := #[]
    for x in sorted do
      if out.isEmpty || out.back! != x then
        out := out.push x
    return out

def build (g : Graph) : IndexedGraph :=
  Id.run do
    let nodes := sortNodesByKey g.nodes
    let keys := nodes.map (fun n => n.key)
    let n := keys.size
    -- id lookup helper over the sorted keys.
    let idOf (k : NodeKey) : UInt32 := Id.run do
      for i in [0:n] do
        if keys[i]!.canonicalKey == k.canonicalKey then
          return UInt32.ofNat i
      return 0  -- unreachable on a validated graph
    -- children per parent
    let mut children : Array (Array ChildRef) := Array.replicate n #[]
    for c in g.children do
      let p := (idOf c.parent).toNat
      let cref : ChildRef := { label := c.label.asStr, ordinal := c.ordinal, child := idOf c.child }
      children := children.set! p (children[p]!.push cref)
    children := children.map (fun list => list.qsort childLt)
    -- recursive edges: children first (role "child"), then Dependency edges.
    let mut recursive : Array RecEdge := #[]
    for parent in [0:n] do
      for c in children[parent]! do
        recursive := recursive.push {
          role := "child", label := c.label, ordinal := c.ordinal,
          src := UInt32.ofNat parent, dst := c.child }
    for e in g.edges do
      if e.role.isRecursive then
        recursive := recursive.push {
          role := "dependency", label := e.label.asStr, ordinal := 0,
          src := idOf e.source, dst := idOf e.target }
    -- recursive successor lists, sorted + deduped.
    let mut succ : Array (Array UInt32) := Array.replicate n #[]
    for e in recursive do
      let s := e.src.toNat
      succ := succ.set! s (succ[s]!.push e.dst)
    succ := succ.map sortDedup
    return {
      keys := keys
      nodes := nodes
      children := children
      recursive := recursive
      recursiveSucc := succ
    }

end View
end Riffcat
