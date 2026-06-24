/-
Shared digest-container types, mirroring the structs in
`crates/riff-catalog-core/src/hash/mod.rs`.

`DimDigests` mirrors `DimensionDigests` (a per-dimension digest map). Rust uses a
`BTreeMap<Dimension, Digest>`, iterated in `Dimension` order; we keep an
association list built in canonical `Dimension` order so iteration matches.
-/

import Riffcat.Schema

namespace Riffcat
namespace Hash

/-- A per-dimension digest map. Invariant: entries are in canonical `Dimension`
order (the order `Dimension.all` produces), matching the Rust `BTreeMap`. -/
abbrev DimDigests := List (Dimension × Digest)

/-- Lookup a dimension's digest. Returns the first match. -/
def DimDigests.get (m : DimDigests) (d : Dimension) : Option Digest :=
  (m.find? (fun (k, _) => decide (k = d))).map (·.2)

/-- Lookup or panic-default (mirrors the `.expect(...)` calls in Rust, which only
fire on a logic bug). -/
def DimDigests.getD (m : DimDigests) (d : Dimension) : Digest :=
  (m.get d).getD ⟨ByteArray.empty⟩

end Hash
end Riffcat
