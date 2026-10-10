# Lean check: details

Reference for [README.md](README.md): what both implementations compute, what the check compares, the laws, the
modules and the limits. Words such as recipe, facet and keys are defined in the top-level
[README](../README.md#words). A digest is a 32-byte BLAKE3 hash.

## What is computed

Rust: `crates/riff-catalog-core/src/hash/`. Lean: `Riffcat/Hash.lean` and `Riffcat/Hash/`. Both take
a graph, a policy (a recipe name and a keys mode) and a set of dimensions, and compute:

1. **Local digests**, per node and dimension: the node's fields in that dimension (sorted), the node
   kind (structure dimension only) and, with bound keys only, the node key. With free keys no key
   bytes enter any digest.
2. **One fold for every graph.** Strongly connected components of the child and `dependency` edges
   are processed, reached components first. Inside a component, colour refinement runs (defined
   below). A node's edges to other components enter its starting colour; edges from other
   components into it do not. The result is one digest per node and dimension.
3. **`graph.full`**, one digest per dimension. Nodes start from their digest from step 2. Colour
   refinement runs over the whole skeleton: child edges (with label and ordinal) and every edge
   except `origin` (with role and label), in both directions. The digest hashes the sorted final
   colours and the sorted edge records written with final colours. The **bare-shape digest** is the
   same computation with no facts and no node kinds.
4. **`mixed_pairs`**, a 32-bit mask with one bit per pair of distinct dimensions (ten pairs for five
   dimensions). Nodes with the same final colour in the bare shape form a group. A pair is flagged
   when, inside one group, both dimensions give different colours to different nodes. A facet that
   contains a flagged pair cannot be answered by combining per-dimension digests: two graphs can
   agree in each dimension and still pair the facts differently.
5. **A joint digest** for a set of dimensions: the graph is rebuilt with the facts of those
   dimensions moved into one dimension (each fact name prefixed by its dimension; node kinds kept
   only if the set contains structure), and `graph.full` of that dimension is taken. One is stored
   for every set of two or more computed dimensions that contains a flagged pair, under a key such
   as `names+types`.
6. **A facet address.** A facet is a policy id and a set of dimensions. Its address is combined
   from the per-dimension `graph.full` digests or, where `mixed_pairs` flags the facet, built from
   the joint digest. The two kinds are hashed under different tags.

Colour refinement: every node has a colour, which is a digest. A round replaces each node's colour
by a digest of its own colour, the sorted records of its outgoing edges and the sorted records of
its incoming edges. A record holds the edge's role, label and ordinal and the colour at the other
end. Rounds stop when the number of distinct colours stops growing, or after as many rounds as
there are nodes (inside a component, members). Graphs that colour refinement cannot tell apart get
equal digests. `Riffcat/Hash/Wl.lean` is named after Weisfeiler-Leman, the usual name for this
method.

Each digest is the hash of a record that begins with the string `riffcat`, the schema version, the
recipe, the dimension name (or `shape`), the keys mode and a record tag. `SCHEMA_VERSION` is 3 in
`crates/riff-catalog-core/src/lib.rs` and in `Riffcat/Policy.lean`. Policy ids, facet ids and facet
addresses begin with `riffcat`, the schema version and a tag.

## Running the check

```sh
cd lean
lake build       # builds the library: every module under Riffcat/, including the laws
lake exe check   # runs the comparison
```

Run `lake exe check` from `lean/`: it reads `golden/vectors.json` relative to the current directory.
It prints the failing comparisons and exits with status 1 if any fails. A passing run ends with:

```
BLAKE3 self-KATs: PASS (all baked vectors)

lockstep: 571 checks PASS, 0 FAIL
ALL GREEN: Lean spec matches the Rust corpus byte for byte.
```

571 is the count on this tree. It changes when the test graphs or the comparisons change.

## What the check covers

The test graphs are the 11 vectors in `golden/vectors.json`. Each is hashed under two policies
(recipe `test/1`; keys `bound` and keys `free`), which gives 22 results.

| Compared | Checks |
| --- | --- |
| BLAKE3 against the 8 values baked into `Hash/Blake3Test.lean` (one check for all 8) | 1 |
| BLAKE3 against the Rust `blake3` crate, 8 input lengths | 8 |
| policy ids | 2 |
| `graph.full` digest, 5 dimensions x 22 results | 110 |
| bare-shape digest | 22 |
| `mixed_pairs` mask | 22 |
| number of stored joint digests | 22 |
| each stored joint digest | 32 |
| facets, 4 per result (88), 4 checks each | 352 |
| total | 571 |

The facets are `full` (all five dimensions), `names_blind` (all but names), `structure_only` and
`names_types`. Each gets four checks: its id; Lean's choice between combined and joint equals the
recorded one; the function that builds a combined address refuses exactly when a joint digest is
needed; the address digest.

The test graphs: `empty`; `single_node`; `golden_fixture` (parent, child, `reference` edge);
`dup_children_1_2` and `dup_children_2_1` (a parent with two children under the same label and
ordinal, constants 1, 2 versus 2, 1); `dep_cycle` and `symmetric_scc_with_tail` (2-cycles of
`dependency` edges, the second with an edge leaving the cycle); `mixed_values` (text, negative
`i64` and bytes values, an `origin` edge, which `graph.full` leaves out, and a `call` edge);
`joint_pairs_a`, `joint_pairs_b` and `joint_three_way` (children that the bare shape cannot tell
apart, differing in names and types, or in kind, names and a constant). `u64` and `bool` values
occur in `single_node`.

Under free keys the last three graphs have masks 32, 32 and 19. The `full` and `names_types`
facets (first two) and the `full` and `names_blind` facets (third) are answered by the joint digest.
Under bound keys the keys tell the children apart, and every mask is 0, as it is for all other
results.

Not compared: the per-node digests and component data. Lean computes them, but `vectors.json` does
not record them; they reach the compared digests only through `graph.full`. Also not compared, and
with no Lean counterpart: units and JSON reading and writing, recipe declarations, indexes, set
roots, claims, and `ArtifactRef` URIs.

## Laws

`Riffcat/Laws.lean` and `Riffcat/Laws/Sorting.lean` contain `theorem`s, which Lean has checked, and
named propositions (`def ... : Prop`), which say what is expected to hold and are not proved. A
proposition asserts nothing; the files contain no `axiom`. `lake exe check` does not use these
files; `lake build` compiles them. The law numbers are the section headings in `Laws.lean`.

### Proved

`#print axioms` for every theorem in the two files lists only `propext` and `Quot.sound`, or
nothing. None of them depends on `sorryAx`.

- `isort_perm_invariant` (Law 1, `Sorting.lean`): insertion sort (`isort`) with a total, transitive,
  antisymmetric comparator returns the same list for any two permutations of an input. Helper
  lemmas in the same file: `ins_perm`, `isort_perm`, `sorted_singleton`, `mem_ins`, `Sorted_ins`,
  `Sorted_isort`, `le_refl_of_total`, `sorted_head_bound`, `sorted_tail`, `sorted_perm_eq`.
- `determinism_records`, `determinism_fields`, `determinism_nodes` (Law 1): the same statement for
  lists of byte arrays, of `Field` and of `Node`.
- `facet_refinement` (Law 2): if two per-dimension digest maps agree on every dimension of a list,
  they agree on every dimension of any sublist.
- `facet_refinement_projection` (Law 2): then the lists of (dimension, digest) pairs over the
  sublist are equal.
- `transport_factors` (Law 3): a function on digest maps that gives equal results for maps that
  agree on the dimensions of a facet factors through the quotient by "agree on those dimensions"
  (`Quot.lift`). The proof is `rfl`.
- `transport_respects` (Law 3): conversely, a function that factors through that quotient gives
  equal results for maps that agree on those dimensions.

None of these mentions `digestGraph` or the encoding functions. They are about `isort` with an
arbitrary comparator, lists of per-dimension digests and `Quot`. The engine sorts with
`Array.qsort`, not `isort`. The connection to the engine is described in comments in `Laws.lean`;
no proof establishes it.

### Stated, not proved

- `FreeKeysRenameInvariant` (Law 4): with free keys, renaming a graph's node keys by an injective
  function leaves every per-dimension graph digest unchanged.
- `RefinementStabilizes` (Law 5a): a recolouring step that only ever splits colour classes leaves
  the classes unchanged at some round no later than the number of members.
- `ComponentDigestOrderFree` (Law 5b): `componentDigest` returns the same result for two orderings
  of the member list, with the same colours and edges. It says nothing about which graphs colour
  refinement can tell apart; there are different graphs it gives the same colours.
- `HeaderSeparates` (Law 6): two record headers with the same bytes have the same schema version,
  recipe, bin, keys mode and record tag.
- `CollisionFreeOn inputs`: no two byte strings in `inputs` have the same digest. This is the hash
  assumption, to be taken as a hypothesis about a stated set of inputs. Over all byte strings it is
  false for any hash of fixed size.

Earlier versions of the file gave these as axioms. Two were false as written (the refinement law
had a counterexample, so the axioms were inconsistent together; the key-renaming law lacked a
hypothesis), and the encoding law followed from the hash axiom without saying anything about the
encoding. The statements above replace them.

## Regenerating the vectors

```sh
lean/scripts/regen-vectors.sh
```

The script runs, from the repository root,
`cargo test -p riff-catalog-core --test lean_vectors -- --ignored --nocapture`, which rewrites
`lean/golden/vectors.json`. The test graphs are defined in `corpus()` in
`crates/riff-catalog-core/tests/lean_vectors.rs`. Each graph is written to the file as a list of
build operations (`node`, `field`, `child`, `edge`), which `Main.lean` replays to rebuild the graph.
For each policy the file also holds the digests listed in the table above, and it holds the BLAKE3
values computed by the `blake3` crate.

The test is marked `#[ignore]`, so `cargo test --workspace` neither regenerates nor reads the file.
After a change to the Rust digest code, regenerate the file and run the check. A change in the
digests means the encoding changed; `crates/riff-catalog-core/tests/golden.rs` states that this
requires a `SCHEMA_VERSION` bump, which `Riffcat/Policy.lean` must follow.

## Modules

Rust paths are relative to `crates/riff-catalog-core/src/` unless they start with `crates/`.

| Lean module | Mirrors |
| --- | --- |
| `Riffcat/Schema.lean` | `crates/riff-catalog-schema/src/{dimension,value,text,key,graph}.rs` |
| `Riffcat/Policy.lean` | `policy.rs`; `SCHEMA_VERSION` from `lib.rs` |
| `Riffcat/Encode.lean` | `encode.rs`; `HashPolicy::policy_id` from `policy.rs` |
| `Riffcat/Hash.lean` | `hash/mod.rs` |
| `Riffcat/Hash/Types.lean` | `DimensionDigests` in `hash/mod.rs` |
| `Riffcat/Hash/View.lean` | `hash/view.rs` |
| `Riffcat/Hash/Local.lean` | `hash/local.rs` (step 1) |
| `Riffcat/Hash/Scc.lean` | `hash/scc.rs` |
| `Riffcat/Hash/Wl.lean` | `hash/wl.rs` |
| `Riffcat/Hash/Condensed.lean` | `hash/condensed.rs` (step 2) |
| `Riffcat/Hash/GraphDigest.lean` | `hash/graph_digest.rs` (step 3) |
| `Riffcat/Hash/Acyclic.lean` | `hash/acyclic.rs` |
| `Riffcat/Reference.lean` | `reference.rs`; parts of `hash/mod.rs` |
| `Riffcat/Hash/Blake3.lean` | none; `encode.rs` calls the `blake3` crate |
| `Riffcat/Hash/Blake3Test.lean` | none |
| `Riffcat/Laws.lean`, `Riffcat/Laws/Sorting.lean` | none |
| `Main.lean` | counterpart of `crates/riff-catalog-core/tests/lean_vectors.rs` |
| `Riffcat.lean` | none |

Not mirrored: in `crates/riff-catalog-schema/src/`, `recipe.rs`, `record.rs`, `unit.rs`, `error.rs`,
`serde_pairs.rs`; in `hash/mod.rs`, `DigestRequest`, `check_acyclic` and `check_recipe`; in
`reference.rs`, `ArtifactRef`; and `index.rs`, `meta.rs`, `root.rs`. The Lean `Facet`,
`FacetAddress` and `AddressValue` are in `Reference.lean`, together with the `needs_joint` and
`facet_address` functions that `hash/mod.rs` defines.

- `Scc.lean` is the iterative Tarjan of the Rust code. The fold relies on its emission order:
  every component a component reaches is emitted before it.
- `Hash/Blake3.lean` is unkeyed 32-byte BLAKE3 only. `Hash/Blake3Test.lean` holds the 8 baked
  values (input lengths 0, 1, 64, 1023, 1024, 1025, 2048, 2049).
- `Main.lean` is the check program and contains its own small JSON reader.
- The check uses `Main`, `Schema`, `Policy`, `Encode`, `Hash` (with `View`, `Types`, `Local`,
  `Condensed`, `Scc`, `Wl`, `GraphDigest`), `Reference`, `Hash/Blake3` and `Hash/Blake3Test`.
- `Hash/Acyclic.lean` has a cycle check. No module imports it and the check does not use it;
  `lake build` compiles it because the library includes every module under `Riffcat/`.
- `Laws.lean` and `Laws/Sorting.lean` are imported by `Riffcat.lean`, not by the check.

## Limits

- Byte equality on the test graphs is evidence about those graphs only.
- The check compares Lean with the committed `vectors.json`, not with a running Rust engine. It
  does not read the file's `schema_version` or `magic` entries.
- The test graphs are small: at most 3 nodes, and cycles only of length 2. None has a field in the
  structure dimension (only node kinds). They use one recipe name, node keys of the `entity` kind
  only (not `derived`), edge roles `reference`, `dependency`, `call` and `origin` only (not
  `graph`, `control`, `data`), and always request all five dimensions.
- The BLAKE3 known-answer inputs end at 2049 bytes (three chunks).
- The Lean code uses lists and linear scans: `refineAndDigest` in `Riffcat/Hash/GraphDigest.lean`
  filters the whole edge list for each node in each round. Only the small test graphs are run.
- Nothing proves that the Lean functions have the properties in the laws file (see Laws).
