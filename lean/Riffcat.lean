/-
Lean 4 port of riffcat's critical core: the digest engine.

Module map (mirrors the Rust crate boundaries):
  Riffcat.Schema       <- riff-catalog-schema (dimension, value, text, key, graph)
  Riffcat.Policy       <- core/src/policy.rs
  Riffcat.Encode       <- core/src/encode.rs (the canonical byte encoding, I1)
  Riffcat.Hash.*       <- core/src/hash/* (view, local, acyclic, scc, wl,
                          condensed, graph_digest, mod)
  Riffcat.Reference    <- core/src/reference.rs (facets, facet addresses)
  Riffcat.Hash.Blake3  <- the one external primitive, implemented in pure Lean

See lean/README.md for the lockstep design and what is verified today.
-/

import Riffcat.Schema
import Riffcat.Policy
import Riffcat.Encode
import Riffcat.Hash
import Riffcat.Reference
import Riffcat.Hash.Blake3
import Riffcat.Hash.Blake3Test
import Riffcat.Laws.Sorting
import Riffcat.Laws
