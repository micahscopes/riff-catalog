# Building the wasm demo

`demo/app` compiles the riffcat fingerprinting crates to `wasm32-unknown-unknown`
via [Trunk] and runs them in the browser, no backend. The core crates
(`riff-catalog-core`/`yul`/`solidity`/`evm`) port to wasm unchanged: pure deps
(blake3, serde, serde_json, thiserror), no getrandom/petgraph/fs/process.

## The normal path (any machine)

```sh
cargo install trunk wasm-bindgen-cli   # or your distro's packages
rustup target add wasm32-unknown-unknown
cd demo/app && trunk build             # or: trunk serve --open
```

That is all most setups need. wasm linking uses `rust-lld`, which ships with
standard rustup toolchains.

## This box only: the wasm linker shim

The dev box here runs a **source-built rustc that ships no `rust-lld`**, and the
Nix store is read-only (no installing packages), so `trunk build` fails with:

```
error: linker `lld` not found
```

A separate `rustc-bootstrap` derivation in the store *does* contain `rust-lld`,
but it is a prebuilt dynamically-linked binary whose interpreter and libs are
off the default loader path. The fix is a shim that runs that `rust-lld` under
glibc's `ld-linux` with the right `--library-path` (libLLVM, glibc, gcc-lib,
zlib, zstd), defaulting the LLD flavor to `wasm`.

`tools/wasm-linker-shim.sh` reconstructs it (idempotent; discovers every store
path dynamically, so it survives garbage-collection churn):

```sh
./tools/wasm-linker-shim.sh                                   # writes ~/.cache/riff-catalog/bin/{rust-lld.real,lld}
PATH="$HOME/.cache/riff-catalog/bin:$PATH" trunk build        # picks up the shim as `lld`
```

The shim is scratch under `~/.cache`, not committed: it points at machine-local
`/nix/store` paths that mean nothing elsewhere. The script is the reproducible
artifact, not its output. None of this is needed once a toolchain with `rust-lld`
(or a system `lld`) is on PATH.

[Trunk]: https://trunkrs.dev
