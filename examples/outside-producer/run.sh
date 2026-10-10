#!/bin/sh
# Admit the Python producer's units with the recipe-free tool and ask it
# three questions. Usage: run.sh [path to riffcat-core]
set -eu
here=$(cd "$(dirname "$0")" && pwd)
tool=${1:-riffcat-core}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

python3 "$here/producer.py" "$work"
core() { "$tool" --corpus "$work/corpus" "$@"; }

core check --declaration "$work/calc-1.json" "$work/left.jsonl" "$work/right.jsonl"
core add --declaration "$work/calc-1.json" --world left "$work/left.jsonl"
core add --world right "$work/right.jsonl"

echo "== same expression, everything compared"
core bucket --unit calc-expr --facet all
echo "== same expression, variable names ignored"
core bucket --unit calc-expr --facet names-blind
echo "== same expression, structure only"
core bucket --unit calc-expr --facet structure
echo "== the commitment to everything admitted"
core root
