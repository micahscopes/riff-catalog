# riffcat examples

Three runnable workflows, two of them against real Sourcify-verified
mainnet deployments. Every output block below is captured from the
scripts as committed; where a table is long, rows are elided with `...`
and the elision is noted, nothing is edited.

Setup, once:

```console
$ cargo build --release -p riff-catalog-cli
$ export PATH="$PWD/target/release:$PATH"
```

Each script accepts two environment overrides: `RIFFCAT` (path to the
binary, default `riffcat` on PATH) and `CORPUS` (corpus directory,
default under `/tmp/riffcat-examples/`). Corpora are plain JSONL; delete
the directory to start over.

## 1. Offline first success (no network, no solc)

`01-offline-yul.sh` ingests [`twins.yul`](twins.yul) through riffcat's
own Yul parser. `--units fn,object` skips the solc-backed SSA pipeline,
so nothing external is invoked. The file has two functions with the same
body shape and different names (`sum_to`, `total_upto`) plus one
genuinely different one (`scale`).

```console
$ examples/01-offline-yul.sh
ingested twins.yul: 13 records
class             size  artifacts  members
----------------  ----  ---------  ------------------
d89b1379ac0c633e  2     1          sum_to, total_upto

3 yul-fn graphs -> 2 classes at facet names-blind (shape); dedup 33.3%
class  size  artifacts  members
-----  ----  ---------  -------

3 yul-fn graphs -> 3 classes at facet all (shape); dedup 0.0%
corpus root 525b6ff4ba57b7f0a4926317761fc90712f10fc622c19b820ab67f94852b5a88 (8 distinct addresses over 8 rows; unit: all, mode: all)
```

The point in one contrast: at the `names-blind` facet (every dimension
except names) the twins share a class; at `all` (names included) the
same three graphs stop merging entirely. Facets are query-time dimension
subsets over digests the corpus already stores, so trying another facet
costs nothing.

## 2. The same function across two verified contracts

`02-same-function-across-contracts.sh` pulls two unrelated mainnet
deployments from Sourcify, recompiles each with its pinned compiler
(0.8.28, downloaded once from binaries.soliditylang.org and cached), and
asks which functions have the same shape in both. Needs network.

- `MerkleDistributor` 1:0xfe570d4ae08cb742327743f6fd8d32512bd7f6a1 (exact_match)
- `Airdrop` 1:0xcb5e279db7060f70e927f1a6fbad256fa0377733 (exact_match)

Source level first (`--unit sol-fn --facet names-blind`), full table:

```console
$ examples/02-same-function-across-contracts.sh
fetched MerkleDistributor (exact_match, compiler 0.8.28+commit.7893614a)
WARN: contracts/MerkleDistributor.sol:MerkleDistributor: solc emitted no yulCFGJson (the SSA pipeline needs a recent solc, ~0.8.29+); no SSA units staged
ingested 1:0xfe570d4ae08cb742327743f6fd8d32512bd7f6a1: 1332 records
fetched Airdrop (exact_match, compiler 0.8.28+commit.7893614a)
WARN: contracts/Airdrop.sol:Airdrop: solc emitted no yulCFGJson (the SSA pipeline needs a recent solc, ~0.8.29+); no SSA units staged
ingested 1:0xcb5e279db7060f70e927f1a6fbad256fa0377733: 969 records
class             A: 0xfe570d4ae08cb742327743f6fd8d32512bd7f6a1           B: 0xcb5e279db7060f70e927f1a6fbad256fa0377733
----------------  ------------------------------------------------------  ---------------------------------------------
00715ed59ab5281b  IERC1363.transferFromAndCall, IERC20.transferFrom       IERC20.transferFrom
00c86279e38f711f  Context._msgSender                                      Context._msgSender
295ad35a541cbd91  IERC20.balanceOf                                        IERC20.balanceOf, IERC20Permit.nonces
324bec43328e5332  Ownable.renounceOwnership                               Ownable.renounceOwnership
7368b4361e2d6d1a  IERC1363.approveAndCall, IERC1363.transferAndCall (+2)  IERC20.approve, IERC20.transfer
78f62cc6478f7ccc  Ownable.owner                                           Ownable.owner
a912757d6102294f  Context._contextSuffixLength                            Context._contextSuffixLength
bc9f7325ee7c6cab  Context._msgData                                        Context._msgData
cbc586b1bad492b0  Ownable._transferOwnership                              Ownable._transferOwnership
cce6a7c0aff686b3  IERC20.allowance                                        IERC20.allowance
e9cd29e43652dc31  Ownable.onlyOwner                                       Ownable.onlyOwner
fca799f39b45cfa8  IERC20.totalSupply                                      IERC20.totalSupply

A: 56 classes, B: 48 classes, shared: 12, Jaccard 0.130
```

The two codebases share their OpenZeppelin vocabulary function by
function, and names-blind matching also pairs functions no name
comparison would: `IERC20.balanceOf` with `IERC20Permit.nonces`,
`IERC1363.approveAndCall` with `IERC20.transfer`.

Then the same question at the IR level, over the unoptimized Yul solc
emitted for each deployment (`--unit yul-fn`); 8 of the 49 shared
classes shown:

```console
class             A: MerkleDistributor:ir:                                                            B: Airdrop:ir:
----------------  ----------------------------------------------------------------------------------  ----------------------------------------------------------------------------------
04474cdf800431ea  fun_owner_67                                                                        fun_owner_40
300d24af7cc12e77  fun__msgSender_997                                                                  fun__msgSender_944
415ea8d9fea5d060  modifier_onlyOwner_88                                                               modifier_onlyOwner_58
7dd5c56e7450b616  fun_rescue_2352, fun_transferOwnership_126                                          fun_setBeneficiary_1118, fun_setReleaseCaller_1202 (+2)
887886008489241c  checked_add_t_uint256                                                               checked_add_t_uint256
8939da5b88097176  allocate_unbounded                                                                  allocate_unbounded
a8e4ff412509eea9  fun_renounceOwnership_98                                                            fun_renounceOwnership_68
f1c55866e40c9ca4  fun__transferOwnership_146                                                          fun__transferOwnership_111
...
A: 86 classes, B: 117 classes, shared: 49, Jaccard 0.318
```

The library functions land in the same classes despite solc's
per-compilation AST-id suffixes (`fun_owner_67` vs `fun_owner_40`), the
compiler's own generated helpers (`allocate_unbounded`,
`checked_add_t_uint256`) collide as they should, and one class holds
four differently named user functions from the two codebases
(`fun_rescue`, `fun_transferOwnership`, `fun_setBeneficiary`,
`fun_setReleaseCaller`): the same guard-then-act shape written twice by
different authors.

This is a function-level structural axis on top of what byte-level
match levels can say about a deployment pair.

## 3. A shared-shape census over five deployments

`03-shared-shape-census.sh` ingests five unrelated mainnet deployments,
all pinned to solc 0.8.28 (four exact_match, one match), and buckets the
whole corpus at three granularities. Needs network. Trimmed to the
highlights:

```console
$ examples/03-shared-shape-census.sh
fetched MerkleDistributor (exact_match, compiler 0.8.28+commit.7893614a)
ingested 1:0xfe570d4ae08cb742327743f6fd8d32512bd7f6a1: 1332 records
fetched VestingWallet (match, compiler 0.8.28+commit.7893614a)
...
321 sol-fn graphs -> 215 classes at facet all (shape); dedup 33.0%
class             size  artifacts  members
----------------  ----  ---------  -------------------------------------------------------------------------------------------------------------------------------------
7368b4361e2d6d1a  16    8          IERC1363.approveAndCall, IERC1363.transferAndCall, IERC20.approve, IERC20.transfer
e9cd29e43652dc31  7     6          Ownable.onlyOwner, Pausable.whenNotPaused, Pausable.whenPaused
295ad35a541cbd91  6     6          IERC20.balanceOf, IERC20Permit.nonces
00c86279e38f711f  5     5          Context._msgSender
575f14c4fe797354  5     2          IUniswapV2Factory.feeTo, IUniswapV2Factory.feeToSetter, IUniswapV2Pair.factory, IUniswapV2Pair.token0 (+1)
...
321 sol-fn graphs -> 180 classes at facet names-blind (shape); dedup 43.9%
class             size  artifacts  members
----------------  ----  ---------  ---------------
873924c8b0bd04a7  5     5          Context
e59a82bf5402d5f7  5     5          IERC20
db3d4e196caa5069  4     4          Ownable
...
45 sol-contract graphs -> 25 classes at facet names-blind (shape); dedup 44.4%
corpus root 5b50c089cc0454292fba350e0dc33015f2b325089c598071a44bc81a975055aa (2983 distinct addresses over 3680 rows; unit: all, mode: all)
```

Reading it:

- Strict facet (`all`, names included): 33.0% of the 321 source
  functions across five deployments are exact repeats, the vendored
  OpenZeppelin base recognized function by function.
- `names-blind` adds the renamed twins and lifts dedup to 43.9%. The
  standout class: `Ownable.onlyOwner` shares a shape with
  `Pausable.whenNotPaused` and `Pausable.whenPaused`, three modifiers
  from two libraries, one check-and-revert riff.
- At `sol-contract` granularity, whole vendored contracts collapse:
  `Context` and `IERC20` are one class each across all five deployments.
- `riffcat root` is an order-independent commitment over every facet
  address in the corpus: two parties who ingested the same deployments
  can compare one hash.

These counts are corpus-relative: they say what recurred among these
five deployments, nothing more.

## Where to go next

`riffcat overlap --help` and `riffcat bucket --help` document selections
(substring match on artifact owner or unit name, `owner::name` to split)
and facets (`all`, `names-blind`, or any `+`-joined subset of structure,
names, constants, types, trace_events). `riffcat diff` compares one
selection pair per dimension; `riffcat conformance` runs the dual-path
drift detectors over local `.sol` files (needs solc on PATH or
`--solc`).
