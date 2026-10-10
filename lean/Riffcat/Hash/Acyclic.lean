/-
Cycle checks, mirroring `crates/riff-catalog-core/src/hash/acyclic.rs`.
`includeDependency` selects between Reject (children + Dependency) and
NonRecursiveGraphEdges (children only, deduped successor set). The digests
themselves come from the one condensed fold for every policy.
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

end Hash
end Riffcat
