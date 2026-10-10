/-
The hashing policy, mirroring `crates/riff-catalog-core/src/policy.rs`.

Mirrors: Keys, HashPolicy, policy_id, SCHEMA_VERSION.

`policy_id` is computed in Encode (it goes through `digest_meta`); here we hold
only the policy data and the `as_str` spellings the encoding commits to.
-/

import Riffcat.Schema

namespace Riffcat

/-- Version of the canonical encoding contract (invariant I8). Must equal the
Rust `SCHEMA_VERSION`. -/
def SCHEMA_VERSION : UInt32 := 3

/-- Whether node keys take part in digests: `bound` pairs nodes by key, `free`
compares graphs up to renaming of keys. -/
inductive Keys where
  | bound
  | free
deriving DecidableEq, Repr, Inhabited

def Keys.asStr : Keys → String
  | .bound => "bound"
  | .free  => "free"

/-- What a digest was computed under: format version, recipe, keys mode. No
dimension set (invariant I9). -/
structure HashPolicy where
  schemaVersion : UInt32
  recipe : Name
  keys : Keys
deriving Inhabited

def HashPolicy.new (recipe : String) (keys : Keys) : Except String HashPolicy := do
  let recipe ← Name.new recipe "policy recipe"
  return { schemaVersion := SCHEMA_VERSION, recipe := recipe, keys := keys }

/-- `check_supported`: only the current schema version. -/
def HashPolicy.checkSupported (p : HashPolicy) : Except String Unit := do
  if p.schemaVersion != SCHEMA_VERSION then
    .error s!"unsupported schema version {p.schemaVersion}"
  return ()

end Riffcat
