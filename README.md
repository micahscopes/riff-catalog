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
| `riff-catalog-view` | parse, identify, and materialize declarative graph views |
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

## Solc SSA observations

The optional solc `yulssatrace` tool emits `solc-ssa-observation/1` JSONL.
Riff-cat lowers every transform and stack-layout snapshot, assigns normal
identity and shape addresses, and stores the accompanying cost metrics:

```console
$ yulssatrace input.yul > trace.jsonl
$ riffcat ingest-ssa-trace trace.jsonl
$ riffcat observations --function many
```

The query exposes live-in, live-out, operation liveness, spills, maximum
block-entry stack depth, shuffle count, shuffle gas, duration, and the
snapshot's shape address.
Use `--owner` during ingestion when a caller already has a stable identity
namespace.

### SSA context Lego pilot

The paired real-solc pilot asks whether one ordered computation keeps its
address when moved from straight-line Yul into a loop, a guarded loop, and a
loop-carried SSA context. It also checks optimized top-level code and a changed
constant as a negative control:

```console
$ RIFFCAT_SOLC=/path/to/solc cargo run -p riff-catalog-yul --example ssa_lego_pilot
```

It reports instruction-local, instruction-subtree, whole-body, SCC, and
whole-function comparisons. The whole-body projection keeps ordered payload
children and internal data edges while treating edges across the region
boundary as ports.

## Solc stack decisions

The SSA stack backend can emit one stream containing target construction,
shuffle results, join candidates, spill closure, and layout iterations. Riff-cat
lowers each event while preserving stack-slot equality without retaining SSA
value numbers:

```console
$ yulssatrace --stack-event-output stack.jsonl input.yul > stages.jsonl
$ riffcat ingest-stack-trace stack.jsonl
$ riffcat observations --stage-kind stack-event
```

`--compiler-event-output` adds SSA snapshots, emitted shuffle playback, and the
final bytecode size and hash to the same stream. `ingest-compiler-trace` is an
alias for the same ingester. A `solc-stack-in-policy/1` file can then replay a
candidate selected from a content-addressed join context through code generation.

Problem and solution are separate graph subtrees. Declarative views can
therefore assign one address to a search problem even when different heuristics
produce different results:

```console
$ riffcat view examples/views/solc-shuffle-problem.riffview <selector> --unit solc-shuffle
$ riffcat view examples/views/solc-join-context.riffview <selector> --unit solc-stack-in-candidate
$ riffcat view examples/views/solc-stack-in-tradeoff.riffview <selector> --unit solc-stack-in-tradeoff
$ riffcat stack-policy <tradeoff-address> --selector policy-default > policy.json
```

The tradeoff view normalizes each join candidate against solc's default choice.
It can therefore group the same local compromise, such as paying two gas now to
remove one stack slot, even when the surrounding layouts and absolute costs
differ.
`stack-policy` closes the loop by resolving every matching profile back to its
object, graph, block, iteration, and candidate coordinates in a replayable
`solc-stack-in-policy/1` document.

The corresponding solution view is
[`examples/views/solc-shuffle-solution.riffview`](examples/views/solc-shuffle-solution.riffview).

## Declarative graph views

The `view` command materializes a content-addressed graph projection from a
`.riffview` file. The first view retains only the reachable computation in a
standalone Yul SSA function:

```console
$ riffcat view examples/views/yul-reachable.riffview <selector> --unit yulssa-fn
```

The view plan digest is part of the output hash policy. Changing the view's
roots, traversal, or retained dimensions therefore creates a distinct facet.
See [`examples/views/yul-reachable.riffview`](examples/views/yul-reachable.riffview)
for the `riffcat-view/1` syntax.

[`examples/views/yul-reachable-cfg.riffview`](examples/views/yul-reachable-cfg.riffview)
defines a second facet without Rust changes. It retains only reachable blocks
and their control-flow structure.

The `language` directive reserves an explicit syntax version. Other ways of
defining facets can be evaluated later without changing the graph contract.

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
