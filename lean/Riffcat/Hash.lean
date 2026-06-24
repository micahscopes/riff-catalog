/-
Per-dimension digest orchestration, mirroring
`crates/riff-catalog-core/src/hash/mod.rs` (`digest_graph`).

`digestGraph`:
  - check policy supported, graph key matches, validate;
  - build the indexed view;
  - local digests per node;
  - per cycle policy: Reject / NonRecursiveGraphEdges -> acyclic tree fold;
    CondenseScc -> condensed fold;
  - graph.full per dimension;
  - assemble GraphHashes (policy id, per-node local/tree/component, components,
    graph-level digests).

We model the dimension set as the canonical-ordered `Dimension.all` (the common
case and what the golden vectors use). The Rust `DigestRequest` carries a
`BTreeSet<Dimension>` iterated in `Dimension` order, which `Dimension.all`
reproduces.
-/

import Riffcat.Hash.View
import Riffcat.Hash.Types
import Riffcat.Hash.Local
import Riffcat.Hash.Acyclic
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
deriving Inhabited

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
  -- tree / condensed fold
  let (finalDigests, components) ←
    match policy.cyclePolicy with
    | .reject => do
        checkAcyclic v true
        let tree ← treeDigests policy v dimensions localDigs
        pure (tree, (#[] : Array ComponentHash))
    | .nonRecursiveGraphEdges => do
        checkAcyclic v false
        let tree ← treeDigests policy v dimensions localDigs
        pure (tree, (#[] : Array ComponentHash))
    | .condenseScc => do
        let (nf, comps) ← condensedDigests policy v dimensions localDigs
        pure (nf, comps)
  -- graph.full per dimension
  let mut graphDigests : DimDigests := []
  for dimension in dimensions do
    let dig ← graphDigestForDimension policy g v finalDigests dimension
    graphDigests := graphDigests ++ [(dimension, dig)]
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
  }

end Hash
end Riffcat
