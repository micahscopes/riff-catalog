# Try it yourself: the cubical law-prover

A Cubical Agda prototype of riffcat's core: the facet as a set-quotient, the
anchors theorem as that quotient's recursion principle, and transport along a
quotient path that computes.

## One command

```sh
cd cubical && agda Riffcat.agda
```

It prints five "Checking ..." lines and exits 0 (clean, no errors, no warnings).
Typechecking the top module green is a single green light over the whole
prototype. Agda 2.8.0 with the cubical library; to force a from-scratch recheck,
remove the local `_build/` first.

## What typechecking establishes

- A facet is a set-quotient: the identification `~_F` is declared as a type
  (`Facet D = Term D / FacetRel D`, via `Cubical.HITs.SetQuotients`), not encoded
  after the fact.
- The anchors principle is that quotient's recursion principle: a fact out of the
  facet is exactly a function on the carrier that respects the identification
  (`anchor = rec`, computation rule `anchor-β`).
- Transport along a quotient path computes: riding an anchored fact along
  `eq/ a b r` reduces, by `refl`, to the supplied respect-proof
  (`transport-computes`, `Concrete.parity-rides`). In Lean 4, `Quot.sound` is
  opaque and the same transport is inert; here it reduces.

## One honest line

The hash is abstract in this prototype (`Digest`, `hashRecord` are the only
postulates), so this witness covers the laws, not the bytes. The Lean
development (`../lean/`) owns the byte oracle: it re-implements BLAKE3 and matches
the Rust engine byte for byte. Different witness, different job.
