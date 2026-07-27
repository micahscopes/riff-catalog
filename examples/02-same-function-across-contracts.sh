#!/usr/bin/env bash
# Two unrelated Sourcify-verified mainnet deployments, one question:
# which functions are structurally the same in both?
#
# Needs network (sourcify.dev and binaries.soliditylang.org) and downloads
# the pinned solc 0.8.28 build once (cached under ~/.cache/riff-catalog).
set -euo pipefail
RIFFCAT="${RIFFCAT:-riffcat}"
CORPUS="${CORPUS:-${TMPDIR:-/tmp}/riffcat-examples/twins}"

# Both verified on Sourcify as exact_match, both compiled with 0.8.28.
MERKLE_DISTRIBUTOR=0xfe570d4ae08cb742327743f6fd8d32512bd7f6a1
AIRDROP=0xcb5e279db7060f70e927f1a6fbad256fa0377733

"$RIFFCAT" --corpus "$CORPUS" ingest \
  --sourcify "1:$MERKLE_DISTRIBUTOR" \
  --sourcify "1:$AIRDROP"

# Source level: which function-body shapes appear in both deployments,
# names ignored.
"$RIFFCAT" --corpus "$CORPUS" overlap "$MERKLE_DISTRIBUTOR" "$AIRDROP" \
  --unit sol-fn --facet names-blind

# IR level: the unoptimized Yul solc emitted for each deployment.
# ("MerkleDistributor:ir:" selects the noopt pipeline; iropt is separate.)
"$RIFFCAT" --corpus "$CORPUS" overlap "MerkleDistributor:ir:" "Airdrop:ir:" \
  --unit yul-fn --facet names-blind
