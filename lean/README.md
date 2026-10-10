# The hashing, written a second time in Lean

The Rust engine turns a graph into hashes. This directory does the same thing in Lean 4, independently. If the
two ever disagree on a test graph, the check fails.

## Run it

Needs [Lean](https://lean-lang.org/install/) (the version in `lean-toolchain` is fetched automatically).

```sh
cd lean
lake exe check
```

The first run builds for a few minutes. A passing run ends with:

```
lockstep: 571 checks PASS, 0 FAIL
```

## What it checks

- 11 small test graphs, each hashed with bound keys and with free keys.
- For each: the hash per bin, the bare-shape hash, and the addresses of four facets.
- The Lean version of BLAKE3 against known values and against the Rust `blake3` crate.

All of it must match the Rust output byte for byte. The Rust output is the file `golden/vectors.json`.

## What it does not show

- It is evidence about those 11 graphs only. They are small: at most three nodes.
- It compares against the saved file, not a running Rust engine. After changing the hashing in Rust, regenerate
  the file:

  ```sh
  lean/scripts/regen-vectors.sh
  ```

## Laws

`Riffcat/Laws.lean` holds statements about the hashing.

**Proved:**

- Sorting gives the same result whatever order the input was in. (The hashing sorts before it hashes.)
- If two graphs agree in every bin of a facet, they agree in every bin of a smaller facet.
- A fact can be read off a facet address exactly when it agrees on graphs the facet treats as the same.

**Stated, not proved yet:**

- With free keys, renaming a graph's nodes changes no hash.
- The colouring step inside the hash stops splitting groups within as many rounds as there are nodes.
- A group's hash does not depend on the order its members are listed in.
- The header of every hashed record pins down the recipe, bin and keys mode.

The unproved ones are written as plain propositions. They assert nothing, so they cannot make anything else
wrong.

## More

[DETAILS.md](DETAILS.md) has the full account: each step of the computation, the table of comparisons, the
theorem names, the module-by-module map to the Rust code, and the limits.
