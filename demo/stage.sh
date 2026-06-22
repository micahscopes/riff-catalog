#!/usr/bin/env bash
# Stage the lightning-demo corpus. Idempotent; safe on a borrowed laptop.
# Usage: demo/stage.sh [path-to-rosetta-examples]
#   The rosetta examples dir may also be given via $ROSETTA_EXAMPLES; otherwise
#   a sibling ../rosetta-fe/examples checkout is auto-detected.
set -euo pipefail

cd "$(dirname "$0")/.."

# Resolve the rosetta examples directory without a hard-coded personal path.
ROSETTA="${1:-${ROSETTA_EXAMPLES:-}}"
if [ -z "$ROSETTA" ]; then
    for candidate in ../rosetta-fe/examples ../../rosetta-fe/examples; do
        if [ -d "$candidate" ]; then ROSETTA="$candidate"; break; fi
    done
fi
if [ -z "$ROSETTA" ] || [ ! -d "$ROSETTA" ]; then
    echo "error: rosetta examples dir not found." >&2
    echo "  pass it as an argument or set ROSETTA_EXAMPLES:" >&2
    echo "    demo/stage.sh /path/to/rosetta-fe/examples" >&2
    exit 1
fi

CORPUS="demo/corpus"
# Release build: the cold-open queries load the full corpus, so a debug binary
# turns the demo's "instant" beats into tens of seconds.
RIFFCAT="target/release/riffcat"

echo "== building riffcat (release) =="
cargo build --release -p riff-catalog-cli

echo "== building the drifted binary (Act II) =="
git apply demo/drift.patch
cargo build --release -p riff-catalog-cli --target-dir target-drift
git apply -R demo/drift.patch

echo "== staging corpus at $CORPUS =="
rm -rf "$CORPUS"

sol_files=$(find "$ROSETTA" -path '*/sol/*.sol' | sort)
[ -n "$sol_files" ] || { echo "no rosetta sources under $ROSETTA"; exit 1; }
# shellcheck disable=SC2086
"$RIFFCAT" --corpus "$CORPUS" ingest $sol_files --units fn,object,ssa,evm

echo "== direct yul (the fe ingestion path) =="
cat > demo/double.yul <<'YUL'
object "Double" {
    code {
        function double(x) -> y { y := add(x, x) }
        sstore(0, double(calldataload(0)))
    }
}
YUL
"$RIFFCAT" --corpus "$CORPUS" ingest demo/double.yul --units fn,object,ssa

echo "== sourcify: Seaport 1.6 (cache-first; needs network + pinned solc 0.8.24 on first run) =="
# Seaport's hand-written assembly defines the same helper name (usr$gcd) in
# three different scopes; ingest now keys functions by lexical scope chain, so
# they stay distinct instead of colliding. Pin 0.8.24 must be resolvable
# (binaries.soliditylang.org, or pre-cached under <cache>/solc-bin); 0.8.24
# predates the SSA pipeline (~0.8.29+), so no yulssa units land — expected.
"$RIFFCAT" --corpus "$CORPUS" ingest \
    --sourcify "1:0x0000000000000068F116a894984e2DB1123eB395" || \
    echo "WARN: sourcify fetch failed (offline?) — Act III falls back to the recording"

echo "== warming the conformance cache (Act II timing) =="
"$RIFFCAT" conformance "$ROSETTA/math" --optimize off > /dev/null

echo
echo "staged. Dry-run the show with: demo/runsheet.md"
