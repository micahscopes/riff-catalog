# Formal-verification artifacts: try the proofs yourself

Two independent witnesses to riffcat's core, each with a one-command entry point.

- [Lean byte-oracle](../../lean/EXAMPLES.md): `lake exe check` holds the Lean
  digest spec to the Rust engine byte for byte (254 checks), and proves Laws 1, 2,
  3 (transport soundness).
- [Cubical law-prover](../../cubical/EXAMPLES.md): `agda Riffcat.agda` typechecks
  the facet as a set-quotient, the anchors theorem as its recursion principle, and
  transport that computes.
