/-
Weisfeiler-Leman color refinement inside one SCC, mirroring
`crates/riff-catalog-core/src/hash/wl.rs`.

`refine`: from an initial color per member (its per-dimension local-derived
digest), refine to a fixpoint. The signature per member is a `wl.signature`
record over the member's own color, its sorted outgoing edge records, and its
sorted incoming edge records. Round cap is |members| (the partition only ever
splits). Stop when the distinct-class count is stable.

`component_digest`: a `wl.component` record over the member count, the sorted
multiset of final colors (duplicates kept), and the byte-sorted quotient edge
multiset over the final colors.

Colors keyed by `u32` id are held as association lists (the components are
small); `colorOf` mirrors the `&color[&id]` lookups, which only fail on a logic
bug in Rust.
-/

import Riffcat.Schema
import Riffcat.Policy
import Riffcat.Encode
import Riffcat.Hash.Types

namespace Riffcat
namespace Hash

open Encode

/-- An internal edge of a component, in id space. -/
structure InternalEdge where
  role : String
  label : String
  ordinal : UInt32
  src : UInt32
  dst : UInt32
deriving Inhabited

/-- A color map keyed by node id. -/
abbrev ColorMap := List (UInt32 × Digest)

def ColorMap.get (m : ColorMap) (id : UInt32) : Digest :=
  match m.find? (fun (k, _) => k == id) with
  | some (_, d) => d
  | none => ⟨ByteArray.empty⟩

/-- Number of distinct colors (distinct digests by hex). -/
private def distinctCount (m : ColorMap) : Nat :=
  (m.map (fun (_, d) => d.toHex)).eraseDups.length

/-- The edge record fed into a WL signature: role, label, ordinal, neighbor
color. -/
private def edgeRecord (edge : InternalEdge) (neighborColor : Digest) : ByteArray :=
  let r := empty
  let r := pushStr r edge.role
  let r := pushStr r edge.label
  let r := pushU32 r edge.ordinal
  pushDigest r neighborColor

/-- One refinement round: recompute every member's color from its current color
plus sorted out/in edge records. -/
private def refineRound (policy : HashPolicy) (dimension : Dimension)
    (members : Array UInt32) (internal : Array InternalEdge) (color : ColorMap)
    : Except String ColorMap := do
  let mut next : ColorMap := []
  for member in members do
    let mut outRecords : List ByteArray := []
    let mut inRecords : List ByteArray := []
    for edge in internal do
      if edge.src == member then
        outRecords := outRecords ++ [edgeRecord edge (color.get edge.dst)]
      if edge.dst == member then
        inRecords := inRecords ++ [edgeRecord edge (color.get edge.src)]
    let dig ← digestRecord policy dimension "wl.signature" fun b =>
      let b := pushDigest b (color.get member)
      let b := pushSortedRecords b outRecords
      pushSortedRecords b inRecords
    next := next ++ [(member, dig)]
  return next

/-- Refine colors to a fixpoint (round cap |members|). -/
def refine (policy : HashPolicy) (dimension : Dimension)
    (members : Array UInt32) (internal : Array InternalEdge) (init : ColorMap)
    : Except String ColorMap := do
  let rec loop (color : ColorMap) (classes : Nat) (fuel : Nat) : Except String ColorMap := do
    match fuel with
    | 0 => return color
    | fuel + 1 =>
      let next ← refineRound policy dimension members internal color
      let nextClasses := distinctCount next
      if nextClasses == classes then
        return next
      else
        loop next nextClasses fuel
  loop init (distinctCount init) members.size

/-- Digest of one component: member count, the sorted multiset of final colors
(duplicates kept), and the byte-sorted quotient edge multiset over colors. -/
def componentDigest (policy : HashPolicy) (dimension : Dimension)
    (members : Array UInt32) (internal : Array InternalEdge) (color : ColorMap)
    : Except String Digest := do
  let colors := (members.map (fun m => color.get m)).qsort (fun a b => byteLt a.bytes b.bytes)
  digestRecord policy dimension "wl.component" fun b =>
    Id.run do
      let mut b := pushU32 b (UInt32.ofNat members.size)
      for d in colors do
        b := pushDigest b d
      let records : List ByteArray := (internal.map (fun edge =>
        let r := empty
        let r := pushStr r edge.role
        let r := pushStr r edge.label
        let r := pushU32 r edge.ordinal
        let r := pushDigest r (color.get edge.src)
        pushDigest r (color.get edge.dst))).toList
      b := pushSortedRecords b records
      return b

end Hash
end Riffcat
