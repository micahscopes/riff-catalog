# riff-catalog

**A catalog of riffs**: facet-relative content addressing for compiler
artifacts. Each artifact is lowered into a canonical graph and hashed once
per *dimension* (structure, names, constants, types, trace events); two
artifacts "rhyme at facet F" when their digests agree on F's dimension
subset. Witnessed **claims** assert equivalences hashing can't see;
**attestations** gate queries on guarantees (verified, tested) that
structure is silent about.

> **Status: DRAFT.** Built out end-to-end for review; no API here is
> blessed yet. The full design plan with its 16 invariants is in
> [PLAN.md](PLAN.md).

Everything rides Argot-maintained tooling: solc does all the compiling
(including the experimental `yulCFGJson` SSA pipeline), Sourcify supplies
verified real-world contracts, and the graph model is built to take the fe
compiler's origin keys at the boundary.

## Crates

| crate | what |
|---|---|
| `riff-catalog-core` | graph interchange form, canonical byte encoding, per-dimension blake3 digests, WL-refined SCC hashing, facets, indexes |
| `riff-catalog-claims` | witnessed equivalence claims (union-find closure), property attestations (gating), claim-gated lookup |
| `riff-catalog-solc` | `solc --standard-json` driver: AST/ir/irAst/yulCFGJson outputs, disk cache, version resolver trait |
| `riff-catalog-yul` | one Yul AST, two front doors (solc JSON + own zero-dep text parser); `yul-ast/1` and `yul-ssa-cfg/1` lowerings |
| `riff-catalog-solidity` | `sol-ast/1`: declarative nodeType-profile walker over raw AST JSON |
| `riff-catalog-evm` | `evm/1`: opcodes → structure, PUSH immediates → constants |
| `riff-catalog-sourcify` | fetch + cache verified contracts, reconstruct standard-json, recompile locally |
| `riff-catalog-cli` | the `riffcat` binary |

## Quickstart (runnable examples, real measured output)

Three scripts under [`examples/`](examples/) run against real
Sourcify-verified mainnet contracts; [`examples/README.md`](examples/README.md)
walks through their actual output.

```console
$ cargo build --release -p riff-catalog-cli
$ export PATH="$PWD/target/release:$PATH"

$ examples/01-offline-yul.sh
# First success with no network and no solc: riffcat's own Yul parser
# ingests examples/twins.yul; 3 yul-fn graphs -> 2 classes at the
# names-blind facet (sum_to and total_upto are one shape under two
# names), -> 3 classes once names count.

$ examples/02-same-function-across-contracts.sh
# Two unrelated Sourcify-verified mainnet deployments (MerkleDistributor
# and Airdrop, both exact_match, both solc 0.8.28), fetched and
# recompiled with the pinned compiler. Needs network. At sol-fn
# names-blind: 12 shared classes, their OpenZeppelin vocabulary matched
# function by function. At yul-fn: 49 shared classes, Jaccard 0.318,
# including one class holding fun_rescue and fun_transferOwnership from
# one codebase and fun_setBeneficiary and fun_setReleaseCaller from the
# other: the same shape under four names.

$ examples/03-shared-shape-census.sh
# Five deployments, one census. 321 sol-fn graphs -> 215 classes at the
# strict facet (33.0% dedup), -> 180 names-blind (43.9%); the standout
# names-blind class merges Ownable.onlyOwner with Pausable.whenNotPaused
# and Pausable.whenPaused. Whole vendored contracts (Context, IERC20)
# collapse to one class each across all five. `riffcat root` closes with
# one order-independent commitment over the corpus.
```

Local `.sol` files ingest too (`riffcat ingest Contract.sol`, needs solc
on PATH or `--solc`), as does direct Yul at the SSA level via solc's
`yulCFGJson` (the ingestion path for fe-emitted Yul). Witnessed claims
and attestations live under `riffcat claim --help` and
`riffcat attest --help`; `riffcat conformance` runs the dual-path and
SSA-round-trip drift detectors over local `.sol` files.

## Where this came from

Successor to the `shape-address` prototype in the fe repo, redesigned per
the interop-plan conversation: WL color refinement replaces key-ordered SCC
hashing (three anonymity leaks fixed, plus cross-component edges folded
into WL init colors so asymmetric cycle members separate), the dimension
set moved out of policy identity, claims/attestations added, salsa coupling
removed. Golden tests freeze the encoding; changing any frozen value
requires a `SCHEMA_VERSION` bump.

Known v1 scope cuts (deliberate, see PLAN.md "Honest scope cuts"): facet
space is the Boolean lattice over 5 dimensions × view mode × level; 1-WL
incompleteness accepted and documented; per-function unit graphs treat
out-of-graph callees as opaque; `yulCFGJson` is experimental upstream and
fenced by the `yul-ssa-cfg/1` level string; sourcify ingestion needs an
svm-style multi-version solc resolver for exact-pinned pragmas (the trait
exists, argotorg/solc-bin is the natural backend).
