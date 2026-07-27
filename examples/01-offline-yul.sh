#!/usr/bin/env bash
# First success, fully offline: no network, no solc.
# riffcat's own Yul parser ingests a .yul file directly; --units fn,object
# skips the solc-backed SSA pipeline, so nothing external is invoked.
set -euo pipefail
cd "$(dirname "$0")"
RIFFCAT="${RIFFCAT:-riffcat}"
CORPUS="${CORPUS:-${TMPDIR:-/tmp}/riffcat-examples/offline}"

"$RIFFCAT" --corpus "$CORPUS" ingest twins.yul --units fn,object

# names-blind: function and variable names ignored, structure kept.
# sum_to and total_upto land in one class; scale stays alone.
"$RIFFCAT" --corpus "$CORPUS" bucket --unit yul-fn --facet names-blind

# At the full facet (names included) the twins separate again:
# same three graphs, zero classes of size >= 2.
"$RIFFCAT" --corpus "$CORPUS" bucket --unit yul-fn --facet all

# An order-independent merkle root over the corpus's facet addresses.
"$RIFFCAT" --corpus "$CORPUS" root
