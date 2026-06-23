#!/usr/bin/env bash
# Re-vendor the pinned Solidity standard libraries used by riffcat's recognition
# catalog. Idempotent: wipes and refetches each library's source tree at the
# version pinned in manifest.json. Run from anywhere.
#
# Source route: GitHub release tarballs (codeload.github.com). The npm CDNs
# (jsdelivr, unpkg) are blocked from this environment, GitHub raw/codeload is not.
set -euo pipefail
cd "$(dirname "$0")"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

vendor() { # <name> <repo> <ref> <tarball-subdir> <dest> [prune-dir...]
  local name="$1" repo="$2" ref="$3" sub="$4" dest="$5"; shift 5
  echo "vendoring $name ($repo@$ref)"
  curl -sSL "https://codeload.github.com/$repo/tar.gz/refs/tags/$ref" -o "$TMP/$name.tgz" \
    || curl -sSL "https://codeload.github.com/$repo/tar.gz/refs/heads/$ref" -o "$TMP/$name.tgz"
  mkdir -p "$TMP/$name" && tar xzf "$TMP/$name.tgz" -C "$TMP/$name"
  local top; top="$(ls "$TMP/$name")"
  rm -rf "$dest" && cp -r "$TMP/$name/$top/$sub" "$dest"
  for p in "$@"; do rm -rf "$dest/$p"; done
  # keep the license alongside the source
  for L in LICENSE LICENSE.txt LICENSE.md; do
    [ -f "$TMP/$name/$top/$L" ] && cp "$TMP/$name/$top/$L" "$dest/LICENSE" && break
  done
  echo "  $(find "$dest" -name '*.sol' | wc -l) .sol files"
}

vendor openzeppelin OpenZeppelin/openzeppelin-contracts v5.0.2     contracts openzeppelin mocks
vendor solady       Vectorized/solady                v0.0.245      src       solady
vendor solmate      transmissions11/solmate          v7            src       solmate     test

echo "done. rebuild the catalog with: bun build-catalog.mjs"
