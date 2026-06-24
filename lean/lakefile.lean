import Lake
open Lake DSL

/-!
Lean 4 port of riffcat's critical core.

No external dependencies on purpose: only Lean core is used (no Mathlib, no
Batteries). The whole point of the lockstep spec is to mirror the Rust digest
engine with as little trusted surface as possible, and blake3 is implemented in
pure Lean (see `Riffcat/Hash/Blake3.lean`), so the hash matches Rust byte for
byte without any FFI or system library.
-/

package «riffcat» where
  -- Keep the build light: no extra leanc flags, no precompiled native code.
  leanOptions := #[
    ⟨`autoImplicit, false⟩
  ]

@[default_target]
lean_lib «Riffcat» where
  -- Globs the umbrella `Riffcat` module and every module under Riffcat/.
  globs := #[.andSubmodules `Riffcat]

/-- Reads `lean/golden/vectors.json`, recomputes every digest, and diffs against
the Rust-produced expectations. Run with `lake exe check`. -/
lean_exe «check» where
  root := `Main
