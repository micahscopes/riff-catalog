#!/usr/bin/env bash
# Regenerate lean/golden/vectors.json from the Rust engine (one cargo build).
# Run from anywhere; resolves the repo root relative to this script.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"
cargo test -p riff-catalog-core --test lean_vectors -- --ignored --nocapture
echo "regenerated lean/golden/vectors.json"
