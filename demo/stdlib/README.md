# Vendored Solidity standard libraries + recognition catalog

Pinned, canonical source for OpenZeppelin, Solady, and Solmate, vendored so the
recognition catalog is reproducible and does not depend on scratch dirs or the
live network. This is what lets riffcat say "that function is OpenZeppelin
`Strings.toString`" about a real contract, by shape, even when the contract
flattened the file or stripped the import path.

## Layout

- `openzeppelin/`, `solady/`, `solmate/`: the upstream source trees at the
  versions pinned in `manifest.json` (each keeps its upstream `LICENSE`).
- `manifest.json`: repos, refs, prunes, licenses, fetch date.
- `refresh.sh`: re-vendors every library from its GitHub release tarball
  (the npm CDNs are blocked here; codeload.github.com is not). Idempotent.
- `build-catalog.mjs`: compiles each tree to AST, fingerprints every function
  at `sol-fn` (names-blind + structure) with the locally built wasm engine, and
  writes `catalog.json`.
- `catalog.json`: the recognition index, `fingerprint -> {library, version,
  contract.function, file}`. A real contract's function is "recognized" when its
  names-blind fingerprint is a key here.

## Licenses

Third-party source, redistributed under each project's own license (kept in
each subtree): OpenZeppelin MIT, Solady MIT, Solmate AGPL-3.0-only. These files
are not riffcat's; they are vendored unmodified for fingerprinting.

## Refreshing / re-pinning

Edit the refs in `refresh.sh` (and `manifest.json`), run `./refresh.sh`, then
`bun build-catalog.mjs`. To recognize multiple major versions of a library
(e.g. OZ v4.9 and v5.0, whose shared functions drift), vendor each version into
its own dir and union the catalogs.
