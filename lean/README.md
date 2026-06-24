# Lean 4 riffcat: the digest engine, in lockstep with Rust

This is a Lean 4 port of riffcat's critical core: the deterministic, pure
function that turns a `Graph` into per-dimension digests and facet addresses. It
exists to be held to the shipping Rust engine byte for byte, on a corpus, in CI,
so the two implementations cannot silently drift. It implements Phase 1 of the
plan in `/workspace/lean-riffcat-lockstep-spike-2026-06-24.md` (the executable
spec plus the golden-vector lockstep).

The same discipline riffcat applies to other people's code is turned inward on
its own core: the digest function is written twice, once in Rust (the engine
that ships) and once here in Lean, and a corpus of graphs is hashed by both with
the bytes required to match exactly.

## What is verified today (all green)

Run `lake exe check` (see below). As of this writing it reports
`254 checks PASS, 0 FAIL`, covering:

- **Full byte-for-byte digest equality.** Every per-dimension `graph.full`
  digest, every `policy_id`, every `facet_id`, and every `address_digest`, over
  a corpus of 8 input graphs under up to 3 policies, matches the Rust output
  exactly. This is true byte equality, not just structural-encoding equality,
  because the hash is the same on both sides (see BLAKE3 below).
- **Pure-Lean BLAKE3, verified two ways.**
  1. Against the official BLAKE3 known-answer test vectors baked into
     `Riffcat/Hash/Blake3Test.lean` (empty input, 1, 64, 1023, 1024, 1025, 2048,
     2049 byte standard inputs, exercising single-block, multi-block,
     multi-chunk, and parent-tree paths).
  2. Against the exact `blake3` crate riffcat ships: the Rust dumper hashes the
     same reference inputs with `blake3::hash` and pins them in
     `golden/vectors.json`, and the Lean side recomputes and compares. So the
     two BLAKE3 implementations are pinned to each other, not just to a spec.
- **The load-bearing canonical encoding (invariant I1).** Because the digests
  match, the full pre-hash byte sequence, the `digest_record` /  `digest_meta`
  framing (magic, schema version, algorithm, level, dimension, view mode, cycle
  policy, record tag), the length-prefixed `push_str`, the little-endian
  integers, the tagged `push_value`, and the byte-lexicographic
  `push_sorted_records`, is reproduced exactly. The structural encoding is the
  thing that had to be right; the matching hash is the proof that it is.
- **The full fold across both policy families.** Acyclic Merkle fold
  (`Reject`), and SCC condensation with WL color refinement (`CondenseScc`),
  including the adversarial shapes the SCHEMA_VERSION 2 bump fixed: duplicate
  `(ordinal, label)` children with swapped constants (`f(1,2)` vs `f(2,1)`),
  a symmetric SCC with an asymmetric external tail, dependency cycles, mixed
  value kinds (negative `i64`, raw bytes), and an `Origin` edge that
  `graph.full` must skip.

The build itself typechecks cleanly (one informational linter note aside) under
Lean 4.29.0 with only Lean core (no Mathlib, no Batteries).

## What is deferred (and why)

- **The property proofs (Phase 2).** This deliverable is the *executable* spec
  and the lockstep. The theorems the spike scopes (facet refinement lattice,
  anonymity / key-renaming invariance, dimension transport, WL termination,
  encoding injectivity with BLAKE3 as an axiom) are a later, research-grade
  phase. The Lean here is written in a functional, structural-recursion style
  that those proofs can be layered onto, but no theorems are stated yet.
- **BLAKE3 collision-resistance is an assumption, not a theorem.** As the spike
  states plainly: everything above is "the construction matches, *given* the
  hash." BLAKE3's collision-resistance is taken on faith. What the pure-Lean
  BLAKE3 buys is that the *functional behavior* of the hash is now a checked,
  dependency-free artifact (KAT-verified), so the lockstep needs no FFI and no
  system library, which this environment does not have.
- **Tying the Rust binary to the spec (Phase 3).** Aeneas/Kani/hax are out of
  scope here; the lockstep corpus is the drift-catcher.
- **Corpus coverage.** Byte equality on a corpus is point evidence, strong on
  the covered shapes (deterministic hash, no tolerance) but only as broad as the
  corpus. The corpus deliberately includes the known-adversarial shapes; it can
  be widened by deriving inputs from real lowerer output later.

## Layout (mirrors the Rust module boundaries)

| Lean module | Rust source |
| --- | --- |
| `Riffcat/Schema.lean` | `crates/riff-catalog-schema` (dimension, value, text, key, graph) |
| `Riffcat/Policy.lean` | `crates/riff-catalog-core/src/policy.rs` |
| `Riffcat/Encode.lean` | `crates/riff-catalog-core/src/encode.rs` (the canonical encoding, I1) |
| `Riffcat/Hash/View.lean` | `hash/view.rs` (`IndexedGraph::build`) |
| `Riffcat/Hash/Local.lean` | `hash/local.rs` (`local_node_digests`) |
| `Riffcat/Hash/Acyclic.lean` | `hash/acyclic.rs` (cycle check, `tree_digests`) |
| `Riffcat/Hash/Scc.lean` | `hash/scc.rs` (iterative Tarjan) |
| `Riffcat/Hash/Wl.lean` | `hash/wl.rs` (`refine`, `component_digest`) |
| `Riffcat/Hash/Condensed.lean` | `hash/condensed.rs` (`condensed_digests`) |
| `Riffcat/Hash/GraphDigest.lean` | `hash/graph_digest.rs` (`graph.full`) |
| `Riffcat/Hash.lean` | `hash/mod.rs` (`digest_graph`) |
| `Riffcat/Reference.lean` | `reference.rs` (facets, facet addresses) |
| `Riffcat/Hash/Blake3.lean` | the `blake3` crate, reimplemented in pure Lean |
| `Riffcat/Hash/Blake3Test.lean` | BLAKE3 known-answer tests |
| `Main.lean` | the lockstep comparator (`lake exe check`) |

A note on faithfulness: the Rust hashing walks (Tarjan, the tree fold, the
cycle DFS) are written iteratively to survive deep ASTs. The Lean mirrors keep
the iterative Tarjan (its emission order is load-bearing for the WL fold) and
write the tree fold as a memoized structural recursion. Agreement is established
by the golden vectors, not by matching the stack machine, exactly as the spike
recommends.

## How to build and run the check

```sh
cd lean
lake build          # typecheck + build the library and the `check` exe
lake exe check       # run the lockstep: BLAKE3 KATs + every golden vector
```

`lake exe check` exits nonzero on any mismatch, so it is a drop-in CI gate. It
reads `golden/vectors.json` relative to the `lean/` directory.

The toolchain is pinned in `lean-toolchain` to `leanprover/lean4:v4.29.0` so
`elan` does not fetch a different toolchain.

### Regenerating the golden vectors (one Rust build)

The corpus is produced by an additive Rust test (no crate source or Cargo.toml
changes):

```sh
# from the repo root
cargo test -p riff-catalog-core --test lean_vectors -- --ignored --nocapture
```

This rewrites `lean/golden/vectors.json`. The dumper lives at
`crates/riff-catalog-core/tests/lean_vectors.rs`. It serializes each input graph
as a list of build "ops" (the same calls a producer makes) so the Lean side
reconstructs the identical `Graph`, then records the Rust-computed policy ids,
per-dimension graph digests, facet ids, and facet address digests, plus the
BLAKE3 KAT cross-check section.

If a value in `vectors.json` changes without a deliberate `SCHEMA_VERSION` bump,
the encoding drifted: that is the same breaking event the in-repo `golden.rs`
test guards, now also enforced against an independent Lean implementation.

## The lockstep design in one paragraph

The hash is deterministic and every ordering inside it is canonical (BTreeMap
order, sorted canonical keys, or byte-lexicographic record sort), so the
cross-language contract can be stated at the byte level. The Rust engine and the
Lean spec are two independent implementations; the corpus is hashed by both and
the bytes must match exactly. The equivalence between the Lean spec and the Rust
bytes is itself the kind of anchored equivalence riffcat is built to record: a
claim, at a stated facet (byte identity of the digest function over this
corpus), against a corpus root, modulo one named assumption (BLAKE3).
