import Lean.Data.Json
import Riffcat.ArtifactCensus

open Lean Riffcat.ArtifactCensus

structure CoverageCase where
  bytes : Nat
  ranges : List (Nat × Nat)
  covered : Nat
  outside : Nat
  deriving FromJson

structure BridgeInput where
  cases : List CoverageCase
  short_bytes : Nat
  long_bytes : Nat
  deriving FromJson

def checkBridge (input : BridgeInput) : Except String Nat := do
  for test in input.cases do
    for (start, stop) in test.ranges do
      unless start < stop && stop ≤ test.bytes do throw "invalid interval"
    let mask := (List.range test.bytes).map fun offset =>
      test.ranges.any fun (start, stop) => decide (start ≤ offset ∧ offset < stop)
    unless covered mask == test.covered && uncovered mask == test.outside do
      throw "coverage vector mismatch"
  unless (render shortCopy).utf8ByteSize == input.short_bytes &&
    (render longCopy).utf8ByteSize == input.long_bytes do throw "text byte vector mismatch"
  return input.cases.length

def main (args : List String) : IO Unit := do
  let [path] := args | throw <| IO.userError "usage: census_bridge <census-bridge.json>"
  let source ← IO.FS.readFile path
  let result : Except String Nat := do
    let json ← Json.parse source
    let input : BridgeInput ← fromJson? json
    checkBridge input
  match result with
  | .ok count => IO.println s!"checked {count} coverage vectors and the text-cost counterexample"
  | .error message => throw <| IO.userError message
