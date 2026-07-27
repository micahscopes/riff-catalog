# Try it yourself: the Lean byte-oracle

The Lean development re-implements riffcat's digest engine, BLAKE3 and all, and
holds it to the shipping Rust engine byte for byte on a corpus.

## One command

```sh
cd lean && lake exe check
```

The first run may rebuild the library (a few minutes under Lean 4.29.0, Lean core
only, no Mathlib). The tail of a green run is exactly:

```
BLAKE3 self-KATs: PASS (all baked vectors)

lockstep: 254 checks PASS, 0 FAIL
ALL GREEN: Lean spec matches the Rust corpus byte for byte.
```

`lake exe check` exits nonzero on any mismatch, so it is a drop-in CI gate. It
reads `golden/vectors.json` relative to `lean/`.

## Read this proof

Open `lean/Riffcat/Laws.lean`. The transport (anchor) soundness result is Law 3,
proved in both directions with no `sorry` and no `axiom`:

- `transport_factors`: a fact that respects the facet congruence `~_F` factors
  through the facet quotient (`Quot.lift`, holds by `rfl`). A fact attached at a
  facet may be transported along that facet when it respects the facet's
  identification.
- `transport_respects`: the converse (via `Quot.sound`). If a fact rides the
  facet, it must respect `~_F`. So "rides facet F" is exactly "`~_F` refines the
  fact's kernel", an iff, not a one-way implication.

Other fully proved laws in the same file: `determinism_records`,
`determinism_fields`, `determinism_nodes` (Law 1, on the kernel
`isort_perm_invariant` in `Riffcat/Laws/Sorting.lean`), and `facet_refinement`
with `facet_refinement_projection` (Law 2).

## Proved vs targeted, honestly

Proved theorems today (Lean's standard axioms only; `#print axioms` touches no
`sorryAx` and none of the targets below):

- Law 1 determinism, Law 2 facet refinement, Law 3 transport (`transport_factors`,
  `transport_respects`).

Stated as clearly-labelled TARGET axioms (named, greppable, `TODO(phase2)`, never
masquerading as proofs):

- Law 4, names-blind AnonymousShape: `anonymousShape_name_blind`
- Law 5a, WL termination: `wl_refine_terminates`
- Law 5b, WL coloring soundness (WL-equivalence, not isomorphism):
  `wl_coloring_sound`
- Law 6, encoding injectivity modulo the hash: `encoding_injective`

The one named cryptographic assumption, an explicit axiom kept separate and never
proved:

- `blake3_collision_free`

The byte-level lockstep above is real evidence over the covered corpus; the Law
4/5/6 proofs are research-grade and left for a later pass.
