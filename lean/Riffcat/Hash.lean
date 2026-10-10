/-
Per-dimension digest orchestration, mirroring
`crates/riff-catalog-core/src/hash/mod.rs` (`digest_graph`).

`digestGraph`:
  - check policy supported, graph key matches, validate;
  - build the indexed view;
  - local digests per node;
  - per cycle policy: Reject / NonRecursiveGraphEdges check acyclicity; every
    policy then uses the condensed fold;
  - graph.full per dimension, with the final colours of every node;
  - the bare shape digest and its colours, then the mixed-pairs mask;
  - assemble GraphHashes (policy id, per-node local/tree/component, components,
    graph-level digests, shape digest, mixed-pairs mask).

The joint pass (`jointDigest`, `jointDigests`) and the mask helpers (`pairBit`,
`maskNeedsJoint`, `dimensionsKey`) live here too, as in the Rust `hash/mod.rs`;
the facet-level entry points that need `Facet` are in `Riffcat/Reference.lean`.

We model the dimension set as the canonical-ordered `Dimension.all` (the common
case and what the golden vectors use). The Rust `DigestRequest` carries a
`BTreeSet<Dimension>` iterated in `Dimension` order, which `Dimension.all`
reproduces.
-/

import Riffcat.Hash.View
import Riffcat.Hash.Types
import Riffcat.Hash.Local
import Riffcat.Hash.Condensed
import Riffcat.Hash.GraphDigest
import Riffcat.Policy
import Riffcat.Encode

namespace Riffcat
namespace Hash

open View

/-- Per-node digest bundle, mirroring `NodeHashes` (Rust field `local` is
spelled `localD` here, since `local` is a Lean keyword). -/
structure NodeHashes where
  localD : DimDigests
  tree : DimDigests
  component : Option DimDigests
deriving Inhabited

/-- Full result, mirroring `GraphHashes` (the parts the lockstep exercises). -/
structure GraphHashes where
  policyId : Digest
  nodes : List (NodeKey × NodeHashes)
  components : Array ComponentHash
  graph : DimDigests
  /-- The bare shape: nodes and edges with no facts and no kinds. -/
  shape : Digest
  /-- Pairs of dimensions that vary together inside one group of nodes the bare
  shape cannot tell apart (bit `pairBit`). Not hashed. -/
  mixedPairs : UInt32
deriving Inhabited

/-- The bit for a pair of distinct dimensions in `GraphHashes.mixedPairs`; no
bit for a dimension paired with itself.
Mirrors `pair_bit`: pairs are numbered in the order (0,1), (0,2), ..., (0,n-1),
(1,2), ... by position in the closed dimension set. -/
def pairBit (a b : Dimension) : UInt32 :=
  if a.ord == b.ord then 0 else
  let (i, j) := if a.ord < b.ord then (a.ord, b.ord) else (b.ord, a.ord)
  let n := Dimension.all.length
  let before : Nat := (List.range i).foldl (fun acc k => acc + (n - 1 - k)) 0
  (1 : UInt32) <<< UInt32.ofNat (before + (j - i - 1))

/-- Whether a mixed-pairs mask flags a pair inside this set of dimensions
(given in canonical order). Mirrors `mask_needs_joint`. -/
def maskNeedsJoint (mixedPairs : UInt32) (dimensions : List Dimension) : Bool :=
  Id.run do
    let dims := dimensions.toArray
    for i in [0:dims.size] do
      for j in [i+1:dims.size] do
        if (mixedPairs &&& pairBit dims[i]! dims[j]!) != 0 then
          return true
    return false

/-- A stable text key for a set of dimensions: their names joined by `+`, in the
fixed dimension order. Mirrors `dimensions_key`. -/
def dimensionsKey (dimensions : List Dimension) : String :=
  String.intercalate "+" (dimensions.map (·.asStr))

/-- For every group of nodes the bare shape cannot tell apart, which dimensions
vary inside it; every pair of dimensions that vary inside one group is
recorded. Mirrors `mixed_pairs`. `colours` holds the final colour of every node
per computed dimension, in dimension order. -/
def mixedPairs (shapeColours : Array Digest) (colours : List (Dimension × Array Digest))
    : UInt32 :=
  Id.run do
    -- group node ids by shape colour
    let mut keys : Array String := #[]
    let mut groups : Array (Array Nat) := #[]
    for id in [0:shapeColours.size] do
      let hex := shapeColours[id]!.toHex
      match keys.findIdx? (fun k => k == hex) with
      | some k => groups := groups.modify k (fun members => members.push id)
      | none =>
        keys := keys.push hex
        groups := groups.push #[id]
    let mut mask : UInt32 := 0
    for members in groups do
      if members.size > 1 then
        let varying : Array Dimension :=
          (colours.filter (fun (_, c) => members.any (fun m => c[m]! != c[members[0]!]!))).toArray.map (·.1)
        for i in [0:varying.size] do
          for j in [i+1:varying.size] do
            mask := mask ||| pairBit varying[i]! varying[j]!
    return mask

/-- Compute per-dimension digests for a graph under a policy and dimension set
(canonical order). -/
def digestGraph (policy : HashPolicy) (graphKey : GraphKey) (g : Graph)
    (dimensions : List Dimension) : Except String GraphHashes := do
  policy.checkSupported
  unless g.graphKey == graphKey do
    .error s!"graph key mismatch: requested {graphKey.canonicalKey}, got {g.graphKey.canonicalKey}"
  g.validate
  let v := build g
  let n := v.len
  -- local digests per node
  let mut localDigs : Array DimDigests := #[]
  for id in [0:n] do
    localDigs := localDigs.push (← localNodeDigests policy (v.node id) dimensions)
  -- one fold for every graph
  let (finalDigests, components) ← condensedDigests policy v dimensions localDigs
  -- graph.full per dimension
  let mut graphDigests : DimDigests := []
  let mut colours : List (Dimension × Array Digest) := []
  for dimension in dimensions do
    let (dig, finalColours) ← graphDigestForDimension policy g v finalDigests dimension
    graphDigests := graphDigests ++ [(dimension, dig)]
    colours := colours ++ [(dimension, finalColours)]
  -- the bare shape, and which dimensions vary inside shape-equal groups
  let (shape, shapeColours) ← shapeDigest policy g v
  let mixed := mixedPairs shapeColours colours
  -- per-node bundles
  -- component-of-node map (only under CondenseScc)
  let componentOfNode : Array (Option Nat) := Id.run do
    let mut m : Array (Option Nat) := Array.replicate n none
    for ci in [0:components.size] do
      for memberKey in components[ci]!.members do
        match v.idOf memberKey with
        | some id => m := m.set! id.toNat (some ci)
        | none => pure ()
    return m
  let mut nodes : List (NodeKey × NodeHashes) := []
  for id in [0:n] do
    let comp : Option DimDigests :=
      match componentOfNode[id]! with
      | some ci => some components[ci]!.digests
      | none => none
    nodes := nodes ++ [(v.key id, {
      localD := localDigs[id]!
      tree := finalDigests[id]!
      component := comp })]
  return {
    policyId := policy.policyId
    nodes := nodes
    components := components
    graph := graphDigests
    shape := shape
    mixedPairs := mixed
  }

/-- The joint pass: hash the graph with all of `dimensions`' facts as one
dimension. Every kept fact moves into the Structure bin under a name prefixed by
its dimension (`<dimension>/<name>`); kinds are kept only when Structure is among
the dimensions, else `_`. The skeleton (keys, children, edges) is unchanged.
Mirrors `joint_digest`. -/
def jointDigest (policy : HashPolicy) (g : Graph) (dimensions : List Dimension)
    : Except String Digest := do
  let hasStructure := dimensions.contains Dimension.structure_
  let nodes : List Node := g.nodes.map (fun node =>
    let kind : Name := if hasStructure then node.kind else ⟨"_"⟩
    let fields : List Field :=
      (node.fields.filter (fun f => dimensions.contains f.dimension)).map (fun f =>
        { dimension := Dimension.structure_
          name := ⟨s!"{f.dimension.asStr}/{f.name.asStr}"⟩
          value := f.value })
    { key := node.key, kind := kind, fields := fields })
  let joint : Graph :=
    { graphKey := g.graphKey, nodes := nodes, children := g.children, edges := g.edges }
  let hashes ← digestGraph policy g.graphKey joint [Dimension.structure_]
  match hashes.graph.get Dimension.structure_ with
  | some d => return d
  | none => .error "structure requested"

/-- The joint digest of every set of computed dimensions the mask flags, keyed by
`dimensionsKey`, sorted by key (Rust `BTreeMap<String, Digest>` order). Empty for
a graph with no flagged pair. Mirrors `joint_digests`. -/
def jointDigests (policy : HashPolicy) (g : Graph) (hashes : GraphHashes)
    : Except String (List (String × Digest)) := do
  if hashes.mixedPairs == 0 then
    return []
  let computed : List Dimension := hashes.graph.map (·.1)
  let mut out : Array (String × Digest) := #[]
  for subset in [1:2 ^ computed.length] do
    let dimensions : List Dimension :=
      ((List.range computed.length).filter (fun i => ((subset >>> i) &&& 1) == 1)).map
        (fun i => computed[i]!)
    if dimensions.length > 1 && maskNeedsJoint hashes.mixedPairs dimensions then
      out := out.push (dimensionsKey dimensions, ← jointDigest policy g dimensions)
  return (out.qsort (fun a b => a.1 < b.1)).toList

end Hash
end Riffcat
