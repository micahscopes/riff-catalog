/-
Exact costs and constructive witnesses for the simplest EVM stack-shuffle
domain: fully prescribed, same-size stacks of distinct values, using only SWAP
operations that exchange the top with one of the 16 reachable slots.

After alpha-normalization the source is [0, 1, ..., n-1] and the target is a
permutation. The exact star-transposition distance is computed from the target's
cycle decomposition. An independent constructive pass emits a witness, and the
executable checks that replay reaches the target with exactly that cost.
-/

import Lean.Data.Json

open Lean

namespace Riffcat.ShuffleOracle

def swapPositions (stack : Array Nat) (left right : Nat) : Array Nat :=
  let leftValue := stack[left]!
  let rightValue := stack[right]!
  (stack.set! left rightValue).set! right leftValue

theorem swapPositions_preserves_height (stack : Array Nat) (left right : Nat) :
    (swapPositions stack left right).size = stack.size := by
  simp [swapPositions]

def applyWitness (source depths : Array Nat) : Except String (Array Nat) := do
  let mut stack := source
  for depth in depths do
    if stack.isEmpty || depth == 0 || depth >= stack.size || depth > 16 then
      throw s!"invalid SWAP depth {depth} for stack size {stack.size}"
    let top := stack.size - 1
    stack := swapPositions stack top (top - depth)
  return stack

def isPermutation (target : Array Nat) : Bool :=
  let expected := Array.range target.size
  target.size <= 16 &&
    target.all (fun value => value < target.size) &&
    expected.all target.contains

/-- Construct a shortest star-transposition witness from identity to target. -/
def optimalWitness (target : Array Nat) : Except String (Array Nat) := do
  if !isPermutation target then
    throw "target is not a distinct permutation of [0, n) with n <= 16"
  if target.isEmpty then
    return #[]
  let top := target.size - 1
  let mut inverse := Array.replicate target.size 0
  for position in [:target.size] do
    inverse := inverse.set! target[position]! position
  let mut visited := Array.replicate target.size false
  let mut depths := #[]
  for start in [:target.size] do
    if !visited[start]! then
      let mut cycle := #[]
      let mut position := start
      while !visited[position]! do
        visited := visited.set! position true
        cycle := cycle.push position
        position := target[position]!
      if cycle.size > 1 then
        if cycle.contains top then
          let mut current := inverse[top]!
          while current != top do
            depths := depths.push (top - current)
            current := inverse[current]!
        else
          let anchor := start
          depths := depths.push (top - anchor)
          let mut current := inverse[anchor]!
          while current != anchor do
            depths := depths.push (top - current)
            current := inverse[current]!
          depths := depths.push (top - anchor)
  let result ← applyWitness (Array.range target.size) depths
  if result != target then
    throw "constructed witness did not reach target"
  return depths

/-- Exact word length for a permutation under EVM top-SWAP generators. -/
def optimalCost (target : Array Nat) : Except String Nat := do
  if !isPermutation target then
    throw "target is not a distinct permutation of [0, n) with n <= 16"
  if target.isEmpty then
    return 0
  let top := target.size - 1
  let mut visited := Array.replicate target.size false
  let mut total := 0
  for start in [:target.size] do
    if !visited[start]! then
      let mut current := start
      let mut length := 0
      let mut containsTop := false
      while !visited[current]! do
        visited := visited.set! current true
        length := length + 1
        containsTop := containsTop || current == top
        current := target[current]!
      if length > 1 then
        total := total + if containsTop then length - 1 else length + 1
  return total

structure SolcWitness where
  swaps : Nat
  witness_depths : Array Nat
deriving FromJson, ToJson

structure OracleInput where
  schema : String
  problem_address : String
  size : Nat
  source : Array Nat
  target : Array Nat
  solc : SolcWitness
deriving FromJson, ToJson

structure OracleResult where
  schema : String
  problem_address : String
  size : Nat
  optimal_swaps : Nat
  oracle_witness_depths : Array Nat
  solc_swaps : Nat
  regret : Int
  max_stack_height : Nat
  reachable_depth_limit : Nat
  evm_stack_limit : Nat
  no_underflow : Bool
  no_overflow : Bool
  oracle_witness_valid : Bool
  solc_witness_valid : Bool
  verification_status : String
deriving ToJson

def solve (input : OracleInput) : Except String OracleResult := do
  if input.schema != "riffcat-shuffle-oracle-input/1" then
    throw s!"unsupported input schema {input.schema}"
  if input.size != input.target.size || input.source != Array.range input.size then
    throw "source must be the declared-size identity permutation"
  let cost ← optimalCost input.target
  let witness ← optimalWitness input.target
  let oracleStack ← applyWitness input.source witness
  let solcStack ← applyWitness input.source input.solc.witness_depths
  let oracleValid := oracleStack == input.target && witness.size == cost
  let solcValid := solcStack == input.target && input.solc.swaps == input.solc.witness_depths.size
  let regret := (input.solc.swaps : Int) - (cost : Int)
  let status :=
    if !oracleValid then "invalid-oracle-witness"
    else if !solcValid then "invalid-solc-witness"
    else if regret < 0 then "solc-below-oracle"
    else "verified"
  return {
    schema := "riffcat-shuffle-oracle-result/1"
    problem_address := input.problem_address
    size := input.size
    optimal_swaps := cost
    oracle_witness_depths := witness
    solc_swaps := input.solc.swaps
    regret
    max_stack_height := input.size
    reachable_depth_limit := 16
    evm_stack_limit := 1024
    no_underflow := true
    no_overflow := input.size <= 1024
    oracle_witness_valid := oracleValid
    solc_witness_valid := solcValid
    verification_status := status
  }

end Riffcat.ShuffleOracle
