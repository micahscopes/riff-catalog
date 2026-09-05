import Lean.Data.Json
import Riffcat.ShuffleOracle

open Lean
open Riffcat.ShuffleOracle

def solveLine (line : String) : Except String String := do
  let value ← Json.parse line
  let input : OracleInput ← fromJson? value
  let result ← solve input
  return (toJson result).compress

def main (args : List String) : IO Unit := do
  let [path] := args
    | throw <| IO.userError "usage: lake exe shuffle_oracle -- <input.jsonl>"
  let content ← IO.FS.readFile path
  for line in content.splitOn "\n" do
    if !line.trimAscii.isEmpty then
      match solveLine line with
      | .ok output => IO.println output
      | .error message => throw <| IO.userError message
