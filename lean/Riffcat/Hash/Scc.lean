/-
Iterative Tarjan SCC, mirroring `crates/riff-catalog-core/src/hash/scc.rs`.

Components are emitted in reverse topological order of the condensation (every
component a node can reach is emitted before the node's own), with members
sorted by node id. Emission order and indices are never hashed; they only order
computation and reporting (invariant I5), but the order is load-bearing for the
WL fold (successors emitted first), so we reproduce the iterative algorithm
faithfully rather than substituting a different SCC routine.
-/

namespace Riffcat
namespace Hash

/-- Internal mutable state of the iterative Tarjan walk. -/
private structure TarjanState where
  index : Array Nat        -- UNDEF marker = "undef" via the `defined` array
  defined : Array Bool
  lowlink : Array Nat
  onStack : Array Bool
  stack : Array Nat
  nextIndex : Nat
  components : Array (Array Nat)

/-- Sort a component's members ascending (Rust `sort_unstable` on u32 ids). -/
private def sortAsc (xs : Array Nat) : Array Nat := xs.qsort (· < ·)

/-- The Tarjan walk in `Nat` id space (cleaner for array indexing). -/
partial def stronglyConnectedComponentsNat (succ : Array (Array UInt32)) : Array (Array Nat) :=
  Id.run do
    let n := succ.size
    let mut st : TarjanState := {
      index := Array.replicate n 0
      defined := Array.replicate n false
      lowlink := Array.replicate n 0
      onStack := Array.replicate n false
      stack := #[]
      nextIndex := 0
      components := #[]
    }
    for root in [0:n] do
      if st.defined[root]! then
        continue
      -- explicit frame stack of (node, nextSuccPos)
      let mut frames : Array (Nat × Nat) := #[(root, 0)]
      while _h : frames.size > 0 do
        let (node, pos) := frames[frames.size - 1]!
        if pos == 0 then
          st := { st with
            index := st.index.set! node st.nextIndex
            defined := st.defined.set! node true
            lowlink := st.lowlink.set! node st.nextIndex
            nextIndex := st.nextIndex + 1
            stack := st.stack.push node
            onStack := st.onStack.set! node true }
        if pos < succ[node]!.size then
          let next := (succ[node]![pos]!).toNat
          frames := frames.set! (frames.size - 1) (node, pos + 1)
          if !st.defined[next]! then
            frames := frames.push (next, 0)
          else if st.onStack[next]! then
            let m := min st.lowlink[node]! st.index[next]!
            st := { st with lowlink := st.lowlink.set! node m }
        else
          if st.lowlink[node]! == st.index[node]! then
            -- pop the component
            let mut component : Array Nat := #[]
            let mut go := true
            while go do
              let member := st.stack.back!
              st := { st with stack := st.stack.pop
                              onStack := st.onStack.set! member false }
              component := component.push member
              if member == node then
                go := false
            st := { st with components := st.components.push (sortAsc component) }
          frames := frames.pop
          if frames.size > 0 then
            let (parent, _) := frames[frames.size - 1]!
            let m := min st.lowlink[parent]! st.lowlink[node]!
            st := { st with lowlink := st.lowlink.set! parent m }
    return st.components

/-- Strongly connected components over the successor lists, in id space.
Components are returned with `UInt32` member ids (the id space the WL/edge
layer uses), in Tarjan emission order, members sorted ascending. -/
partial def stronglyConnectedComponents (succ : Array (Array UInt32)) : Array (Array UInt32) :=
  (stronglyConnectedComponentsNat succ).map (fun c => c.map UInt32.ofNat)

end Hash
end Riffcat
