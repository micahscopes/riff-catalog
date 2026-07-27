#!/usr/bin/env bash
# Five unrelated Sourcify-verified mainnet deployments (all pinned to solc
# 0.8.28), one census: which function and contract shapes recur across
# deployments, and how much does the corpus collapse under each facet?
#
# Needs network; downloads the pinned solc 0.8.28 build once (cached).
set -euo pipefail
RIFFCAT="${RIFFCAT:-riffcat}"
CORPUS="${CORPUS:-${TMPDIR:-/tmp}/riffcat-examples/census}"

"$RIFFCAT" --corpus "$CORPUS" ingest \
  --sourcify 1:0xfe570d4ae08cb742327743f6fd8d32512bd7f6a1 \
  --sourcify 1:0x59ea41c5fece9b74a0cb3a319bc6731f547073c0 \
  --sourcify 1:0x7e7f0d97e5097444a5ee5a15c4dd2ac9f8454698 \
  --sourcify 1:0xcb5e279db7060f70e927f1a6fbad256fa0377733 \
  --sourcify 1:0x842979fb52b0c19629ae3186838fccbd06484697

# Source functions, strict: every dimension must agree, names included.
"$RIFFCAT" --corpus "$CORPUS" bucket --unit sol-fn --facet all --top 10

# Source functions, names-blind: renamed twins join their classes.
"$RIFFCAT" --corpus "$CORPUS" bucket --unit sol-fn --facet names-blind --top 10

# Whole contracts, names-blind.
"$RIFFCAT" --corpus "$CORPUS" bucket --unit sol-contract --facet names-blind --top 10

# One order-independent commitment over the whole corpus.
"$RIFFCAT" --corpus "$CORPUS" root
