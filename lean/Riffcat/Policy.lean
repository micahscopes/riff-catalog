/-
The hashing policy, mirroring `crates/riff-catalog-core/src/policy.rs`.

Mirrors: Algorithm, ViewMode, CyclePolicy, HashPolicy, policy_id, SCHEMA_VERSION.

`policy_id` is computed in Encode (it goes through `digest_meta`); here we hold
only the policy data and the `as_str` spellings the encoding commits to.
-/

import Riffcat.Schema

namespace Riffcat

/-- Version of the canonical encoding contract (invariant I8). Must equal the
Rust `SCHEMA_VERSION`. -/
def SCHEMA_VERSION : UInt32 := 2

inductive Algorithm where
  | blake3_256
  | sha2_256
deriving DecidableEq, Repr, Inhabited

def Algorithm.asStr : Algorithm → String
  | .blake3_256 => "blake3-256"
  | .sha2_256   => "sha2-256"

inductive ViewMode where
  | identityBound
  | anonymousShape
deriving DecidableEq, Repr, Inhabited

def ViewMode.asStr : ViewMode → String
  | .identityBound  => "identity_bound"
  | .anonymousShape => "anonymous_shape"

inductive CyclePolicy where
  | reject
  | nonRecursiveGraphEdges
  | condenseScc
deriving DecidableEq, Repr, Inhabited

def CyclePolicy.asStr : CyclePolicy → String
  | .reject                 => "reject"
  | .nonRecursiveGraphEdges => "non_recursive_graph_edges"
  | .condenseScc            => "condense_scc"

/-- The encoding contract a digest was computed under. Note: no dimension set
(invariant I9). -/
structure HashPolicy where
  schemaVersion : UInt32
  algorithm : Algorithm
  level : Name
  viewMode : ViewMode
  cyclePolicy : CyclePolicy
deriving Inhabited

def HashPolicy.new (level : String) (viewMode : ViewMode) (cyclePolicy : CyclePolicy)
    : Except String HashPolicy := do
  let level ← Name.new level "policy level"
  return {
    schemaVersion := SCHEMA_VERSION
    algorithm := .blake3_256
    level := level
    viewMode := viewMode
    cyclePolicy := cyclePolicy
  }

/-- `check_supported`: only schema version 2 + blake3-256 hash. -/
def HashPolicy.checkSupported (p : HashPolicy) : Except String Unit := do
  if p.schemaVersion != SCHEMA_VERSION then
    .error s!"unsupported schema version {p.schemaVersion}"
  if p.algorithm != Algorithm.blake3_256 then
    .error s!"unsupported digest algorithm {p.algorithm.asStr}"
  return ()

end Riffcat
