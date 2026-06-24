/-
CondenseScc orchestration, mirroring
`crates/riff-catalog-core/src/hash/condensed.rs`.

Tarjan condensation, WL refinement per component, condensation-DAG folding, and
per-node component-context digests. Components are processed in Tarjan emission
order (every component a member reaches is finished first), so a member's
outgoing cross-component edges can be folded into its WL initial color
(`wl.init`). Cross-incoming edges stay out (context, not content, invariant I3).

The per-component results (`member_colors`, `component_tree`) accumulate as we
walk emission order, exactly as in Rust; `cross_edge_record` reads the already
computed successor-component fold and target-member color.
-/

import Riffcat.Hash.View
import Riffcat.Hash.Types
import Riffcat.Hash.Scc
import Riffcat.Hash.Wl
import Riffcat.Policy
import Riffcat.Encode

namespace Riffcat
namespace Hash

open Encode View

/-- A component's per-dimension member colors: dimension -> (member id -> color).
Held as an association list keyed by dimension (canonical order). -/
private abbrev PerDimColors := List (Dimension × ColorMap)

private def PerDimColors.get (m : PerDimColors) (d : Dimension) : ColorMap :=
  match m.find? (fun (k, _) => decide (k = d)) with
  | some (_, c) => c | none => []

/-- A reporting `ComponentHash`, mirroring the Rust struct (only the fields the
golden vectors and the structural spec exercise). -/
structure ComponentHash where
  componentIndex : UInt32
  members : Array NodeKey
  digests : DimDigests
deriving Inhabited

/-- Record for one outgoing cross-component edge: role, label, ordinal, then the
target component's fold and the target member's color. -/
private def crossEdgeRecord (edge : InternalEdge) (targetTree targetColor : Digest) : ByteArray :=
  let r := empty
  let r := pushStr r edge.role
  let r := pushStr r edge.label
  let r := pushU32 r edge.ordinal
  let r := pushDigest r targetTree
  pushDigest r targetColor

/-- Accumulator across components processed in emission order. -/
private structure CondAccum where
  memberColors : Array PerDimColors      -- per component index
  wlComponent : Array DimDigests
  componentTree : Array DimDigests

def condensedDigests (policy : HashPolicy) (v : IndexedGraph) (dimensions : List Dimension)
    (localDigs : Array DimDigests)
    : Except String (Array DimDigests × Array ComponentHash) := do
  let components := stronglyConnectedComponents v.recursiveSucc
  let nComp := components.size
  let nNodes := v.len
  -- comp_of[node id] = component index
  let mut compOf : Array Nat := Array.replicate nNodes 0
  for ci in [0:nComp] do
    for member in components[ci]! do
      compOf := compOf.set! member.toNat ci
  -- partition recursive edges
  let mut internal : Array (Array InternalEdge) := Array.replicate nComp #[]
  let mut crossByComponent : Array (Array InternalEdge) := Array.replicate nComp #[]
  let mut crossByMember : Array (Array InternalEdge) := Array.replicate nNodes #[]
  for edge in v.recursive do
    let sc := compOf[edge.src.toNat]!
    let tc := compOf[edge.dst.toNat]!
    let ie : InternalEdge :=
      { role := edge.role, label := edge.label, ordinal := edge.ordinal,
        src := edge.src, dst := edge.dst }
    if sc == tc then
      internal := internal.set! sc (internal[sc]!.push ie)
    else
      crossByComponent := crossByComponent.set! sc (crossByComponent[sc]!.push ie)
      crossByMember := crossByMember.set! edge.src.toNat (crossByMember[edge.src.toNat]!.push ie)

  -- process components in emission order, accumulating colors / trees
  let mut acc : CondAccum := { memberColors := #[], wlComponent := #[], componentTree := #[] }
  for ci in [0:nComp] do
    let members := components[ci]!
    let mut perDim : PerDimColors := []
    let mut component : DimDigests := []
    let mut tree : DimDigests := []
    for dimension in dimensions do
      -- wl.init per member: local digest + sorted outgoing cross-edge records
      let mut init : ColorMap := []
      for member in members do
        let records : List ByteArray := (crossByMember[member.toNat]!.map (fun edge =>
          let targetComp := compOf[edge.dst.toNat]!
          let targetTree := (acc.componentTree[targetComp]!).getD dimension
          let targetColor := ((acc.memberColors[targetComp]!).get dimension).get edge.dst
          crossEdgeRecord edge targetTree targetColor)).toList
        let dig ← digestRecord policy dimension "wl.init" fun b =>
          let b := pushDigest b ((localDigs[member.toNat]!).getD dimension)
          pushSortedRecords b records
        init := init ++ [(member, dig)]
      let colors ← refine policy dimension members internal[ci]! init
      let compDig ← componentDigest policy dimension members internal[ci]! colors
      component := component ++ [(dimension, compDig)]
      -- component.tree: own component digest + cross-edge records over final colors
      let records : List ByteArray := (crossByComponent[ci]!.map (fun edge =>
        let targetComp := compOf[edge.dst.toNat]!
        let targetTree := (acc.componentTree[targetComp]!).getD dimension
        let targetColor := ((acc.memberColors[targetComp]!).get dimension).get edge.dst
        let base := crossEdgeRecord edge targetTree targetColor
        -- prepend the source member's final color
        let withSource := pushDigest empty (colors.get edge.src)
        withSource ++ base)).toList
      let treeDig ← digestRecord policy dimension "component.tree" fun b =>
        let b := pushDigest b compDig
        pushSortedRecords b records
      tree := tree ++ [(dimension, treeDig)]
      perDim := perDim ++ [(dimension, colors)]
    acc := { acc with
      memberColors := acc.memberColors.push perDim
      wlComponent := acc.wlComponent.push component
      componentTree := acc.componentTree.push tree }

  -- per-node final digests: node.component_context
  let mut nodeFinal : Array DimDigests := #[]
  for id in [0:nNodes] do
    let ci := compOf[id]!
    let mut digests : DimDigests := []
    for dimension in dimensions do
      let dig ← digestRecord policy dimension "node.component_context" fun b =>
        Id.run do
          let mut b := b
          if policy.viewMode == ViewMode.identityBound then
            b := pushNodeKey b (v.key id)
          b := pushDigest b ((localDigs[id]!).getD dimension)
          b := pushDigest b (((acc.memberColors[ci]!).get dimension).get (UInt32.ofNat id))
          b := pushDigest b ((acc.componentTree[ci]!).getD dimension)
          return b
      digests := digests ++ [(dimension, dig)]
    nodeFinal := nodeFinal.push digests

  -- reporting component hashes
  let mut componentHashes : Array ComponentHash := #[]
  for ci in [0:nComp] do
    let members := components[ci]!
    let memberKeys := members.map (fun m => v.key m.toNat)
    componentHashes := componentHashes.push {
      componentIndex := UInt32.ofNat ci
      members := memberKeys
      digests := acc.componentTree[ci]! }

  return (nodeFinal, componentHashes)

end Hash
end Riffcat
