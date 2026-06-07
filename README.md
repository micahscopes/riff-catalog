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

## Quick demo (rosetta corpus, real measured output)

```console
$ riffcat ingest examples/erc20/sol/ERC20.sol examples/math/sol/SolidityFullMath.sol \
    examples/amm/sol/SoliditySimpleAmm.sol

$ riffcat bucket --unit yul-fn --mode shape --facet all
# 536 yul-fn graphs -> 216 classes (59.7% dedup): solc's generated helpers
# (allocate_unbounded, revert_error_*, abi_decode_*) collide across all
# contracts. At --facet structure: 84 classes, 84.3% dedup.

$ riffcat overlap "ERC20.sol" "SoliditySimpleAmm" --unit yul-fn --facet names-blind
# 36 shared classes, Jaccard 0.434 — external_fun_balanceOf structurally
# twins external_fun_swapAForB.

$ riffcat diff "yulir:ERC20.sol:ERC20:ir:noopt" "yulir:ERC20.sol:ERC20:iropt:noopt" \
    --unit yul-fn --name fun_transfer
# per-dimension survival matrix: what the optimizer preserved, per facet.

$ riffcat conformance examples/
# level 0: parse(ir text) == deserialize(irAst JSON)
# level 1: both paths hash identical on every digest, both view modes
# level 2: Solidity→yulCFGJson  vs  ir text→solc-as-Yul→yulCFGJson
# GREEN over the corpus; nonzero exit on any drift.

$ riffcat claim add --left "ERC20:ir:noopt::=fun_transfer_72" \
    --right "SoliditySimpleAmm:ir:noopt::=fun_swapAForB_69" \
    --facet structure --witness-kind note --witness "reason=demo"
$ riffcat bucket --unit yul-fn --facet structure --claims
# classes 84 -> 83: the (deliberately bogus) claim merged two genuinely
# different functions. Claims are inputs, not discoveries — validity is the
# witness auditor's problem; `riffcat claim list` keeps it attributable.

$ riffcat attest add --subject "ERC20:ir:noopt::=fun_transfer_72" \
    --property verified-total --witness-kind lean-proof --witness theorem=transfer_total
$ riffcat bucket --unit yul-fn --facet structure --require verified-total
# 534 rows correctly excluded: guarantees gate, structure stays silent.

$ riffcat ingest --sourcify 1:0x231b0Ee14048e9dCcD1d247744d114a4EB5E8E63
# ENS PublicResolver (exact_match, 0.8.17) fetched, recompiled, ingested:
# 3331 records. Second run is fully offline (cache).
$ riffcat overlap ":sf" "yulir:ERC20" --unit yul-fn --facet names-blind
# 39 shared classes between a mainnet-verified contract and a local build.
```

Direct Yul (`riffcat ingest demo.yul`) works too — through our own parser
at the `yul-ast` level and through solc's `yulCFGJson` at the SSA level
(this is the ingestion path for fe-emitted Yul).

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
