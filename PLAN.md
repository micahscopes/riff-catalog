# riff-catalog — facet-relative content addressing for Argot compilers

## Context

The exported design conversation (~/Downloads, June 6) produced an interop plan: compilers stop fighting over languages by lowering into a shared content-addressed store where the only contract is "equal at facet F" — Merkle hashing on acyclic structure, Weisfeiler-Leman (WL) refinement inside SCCs, facets as named omission sets, witnessed claims for what hashing can't see, self-describing references. The prototype (`shape-address`, 2333 lines, fe repo branch `origin-overhaul-phased`) proved dimension-pure hashing but has known flaws and is welded to fe via `common::origin::OriginExportKey` (salsa).

This plan builds **riff-catalog** (a catalog of riffs; also cata(morphism)+log) — a fresh standalone workspace embodying the conversation's design, working for fe *and* a solc demo (Solidity + Yul, both forms) *and* Sourcify. Everything is Argot dogfooding: solc, Sourcify, fe are all argotorg projects; the demo targets the collective's "too many compilers" pain.

**Key discoveries during planning** (verified against installed solc 0.8.31-pre):
- `yulCFGJson` standard-json output = Moritz (clonker)'s actively-developed Yul **SSA CFG** (PRs #16646–#16767). Works for **both** `language:"Solidity"` (viaIR) and `language:"Yul"` (direct input — which `--ir-ast-json` does NOT support). Full SSA: `PhiFunction` aligned with block `entries`, `LiteralAssignment`, typed exits, liveness, per-function blocks, jump targets → real back-edges → SCCs. SSA values (`v0,v1…`) erase names *by construction*. Optimizer-sensitive (verified: 17 helpers unopt vs fully-inlined opt). solc already content-addresses helper names (`revert_error_<keccak>…`) — riff-catalog generalizes what they started.
- Existing Yul parsers rejected: revive-yul (drags inkwell/LLVM), yultsur (2018), yul-parser (2022), solang/solar (inline-assembly only). Our own ~450-line parser is in-repo (not a third-party dep) and powers the conformance test.
- Full Yul irAst + Solidity AST node inventories verified across all 9 rosetta contracts (compile cleanly viaIR, opt on/off). Number literal spellings preserved verbatim in JSON (`0x40` vs `64` both occur → canonicalization required in lowering).

## Decisions made with Micah

- New standalone repo: `/home/micah/hacker-stuff-2023/fe-stuff/riff-catalog` (git init, own workspace, edition 2024)
- Name: **riff-catalog**; crates `riff-catalog-*` (crates.io: riff, catalog both free)
- Must-haves: WL refinement in SCCs; claims + witnesses. Deferred: persistent store (JSONL instead), facet-lattice-as-data
- Prefer Argot-maintained tooling throughout (solc is the only external binary; no third-party compiler deps)
- Yul "both forms": direct .yul (fe's output) AND Solidity→IR, unopt + opt

## Design invariants (checked against the interop conversation + this session)

**Identity & partition:**
- **I1 — Context commitment.** Digest equality is meaningful only under identical (schema_version, algorithm, level, view_mode, cycle_policy, dimension); every digest record's header commits to all of it. A bare hash can never overclaim. [convo layer 5]
- **I2 — One-directional dimension purity.** Editing a field tagged dimension *d* moves only *d*'s digests. Topology edits move every dimension (all fold over the same skeleton); labels/ordinals/roles/kinds enter Structure payloads only.
- **I3 — Compositionality (Merkle).** A node's digest depends only on its subtree (acyclic) or its SCC + reachable components — context-free; sub-references stable; equal subtrees collide across artifacts (the overlap feature).
- **I4 — SCC atomicity.** No context-free identity finer than the SCC inside a cyclic region; member digests = f(component digest, WL color). [convo hazard #3]
- **I5 — Anonymity soundness.** In AnonymousShape mode no node-key-derived bytes (keys, key-orderings, Tarjan indices, internal u32 ids) may influence any digest — WL colors replace key order. The prototype violated this 3× (all have named regression tests). Anonymous equality = "WL-equivalent under policy": isomorphic ⇒ equal; pathological regular graphs can over-merge (1-WL incompleteness, documented). The equivalence IS the kernel of our function, versioned — not a claim of isomorphism.
- **I6 — Construction-order independence.** Field/edge insertion order never matters; child *ordinal* is semantics and always matters.

**References & schema:**
- **I7 — Self-description.** Reference = (schema_version, policy_id, facet|dimension, digest). The facet travels with the digest. [convo: loose references, not 1:1 pointers]
- **I8 — Explicit evolution.** Encoding changes ⇒ SCHEMA_VERSION bump; old digests are strangers to new; migration explicit (Cambria-style lenses later), never silent. Golden fixture tests enforce mechanically. [convo hazard #4]
- **I9 — Policy id = encoding contract, exactly.** Covers precisely what affects bytes (dimension-set removed from it — it never affects per-dimension payloads).
- **I10 — The lowering is part of the contract.** Any normalization an ingestion path applies is identified by the versioned policy `level` string (`"yul-ast/1"`, `"yul-ssa-cfg/1"`, `"sol-ast/1"`, `"evm/1"`). Two producers at the same level must lower identically — that's what conformance tests check. [convo hazard #1; the playground's canonicalize-commutative/positional-access facets live here, not in dimensions]

**Claims:**
- **I11 — Claims are inputs, not discoveries.** Closure never inspects witness payloads; a wrong witness merges, attributably. Validity belongs to the witness auditor; attributability belongs to us. [playground]
- **I12 — Facet scoping.** A claim at facet F never affects facet G. No implication/subset reasoning in v1. [convo hazard #2: guarantee flattening]
- **I13 — Claims both merge and gate.** Binary Equivalent claims only coarsen (union-find merge). Unary **Attestations** (`subject attests property P, witnessed by W`) only filter: a guarantee-bearing query excludes unattested artifacts ("the unattested one is correctly excluded" — interop Phase 3). v1 ships both.
- **I14 — Auditability.** Direct (structural) and via-claims results reported separately, with supporting claim ids.
- **I17 — Corpus roots.** A corpus commits to its contents via a canonical, order-independent merkle root over its distinct facet addresses (`set_root`: sorted-set leaves, domain-separated leaf/node tags, odd nodes promoted). Names, owners, units, and ingestion order never enter; twin rows collapse to one leaf. `riffcat root --check` recomputes and fails loudly on mismatch. The zk-socket: aggregate claims verify against roots someday; zero prover code here, ever.
- **I18 — Conditional claims (claims schema v2, adopted from Ixon's `assumptions`).** A claim may carry `assumptions: Option<Digest>` — a merkle root over an assumption set; the claim holds *modulo* that set. A conditional claim NEVER merges unless the querier explicitly accepts its exact root (`--assume <root>`), and acceptance stays attributable (I14). Readers refuse claim records from schema versions they don't know — an assumptions-blind reader must not silently merge a conditional claim unconditionally.

**Ingestion:**
- **I15 — Volatile producer artifacts never enter payloads.** solc numeric `id`s, `src`/`nativeSrc`, block ids, SSA value numbers are dropped or mapped to stable structure (def-use as positional/edge encoding, never `v17`-as-text). Mirror of I5.
- **I16 — Coarse granularity.** Function-level graphs (+ contract/object level); recursion at callgraph-SCC granularity. [convo: "content-addressing pays at coarse units"]

**Honest scope cuts vs the conversation:** v1 facet space is the Boolean lattice over 5 dimensions × view_mode × level (the convo wanted non-disjoint lattices — richer facets enter via level strings and claims for now). Identity-as-a-facet-projection not formalized (ViewMode stays a policy axis; rustdoc documents the door). 1-WL incompleteness accepted and named.

## Workspace layout

```
riff-catalog/
├── Cargo.toml          # workspace, edition 2024, resolver 3
│   # deps: blake3 1, serde 1(derive), serde_json, thiserror 2, clap(derive),
│   #       anyhow, ureq(rustls,json), semver, hex, sha2 (cli cache keys)
├── crates/
│   ├── riff-catalog-core/      # graph model + canonical encoding + hashing + WL + index
│   ├── riff-catalog-claims/    # claims, attestations, union-find closure, gated lookup
│   ├── riff-catalog-solc/      # solc standard-json driver (no core dep)
│   ├── riff-catalog-yul/       # Yul AST + lexer/parser + tree lowering + SSA-CFG lowering
│   ├── riff-catalog-solidity/  # generic nodeType profile walker + Solidity profile
│   ├── riff-catalog-evm/       # bytecode → graph (~100 lines)
│   ├── riff-catalog-sourcify/  # fetch verified contracts → reconstruct std-json → solc
│   └── riff-catalog-cli/       # bin "riffcat": ingest/bucket/overlap/diff/conformance/claim
└── testdata/{yul/, fixtures/}  # hand-written .yul + checked-in JSON fixtures for offline tests
```

Core is ONE crate (not graph+hash): encoding primitives stay `pub(crate)`; a crate boundary would force the encoder public — the exact drift surface the design exists to prevent. Claims separate: different cadence, operates only on digests/facets.

## riff-catalog-core (≈2,400 LOC incl. tests)

Modules: `error, text, key, value, dimension, policy, graph, encode(pub(crate)), reference, index, hash/{mod,view,local,acyclic,scc,wl,condensed,graph_digest}`.

Port-with-rename from shape-address (`fe-worktrees/origin-overhaul-phased/crates/shape-address/src/lib.rs` + its SPEC.md golden matrix): `Name` (validated, no '\u{1f}'), `Value{Text,Bool,U64,I64,Bytes}`, `Dimension{Structure,Names,Constants,Types,TraceEvents}` (closed enum — open dimensions are drift bait), `EdgeRole{Graph,Control,Data,Reference,Call,Dependency,Origin}` (Dependency-only recursive), graph model + `GraphSink` + validate, local digests, acyclic Merkle (made iterative), graph_digest, `DigestIndex`, encode helpers (magic `"riffcat"`). `EntityKey{kind,owner,local}` replicates OriginExportKey validation (common/src/origin.rs:59-158), zero deps — fe maps at its boundary. `Digest` = `[u8;32]`, serde as 64-hex.

Fixes over the prototype (4 known flaws + 3 found during design):
1. **WL replaces key-ordered SCC hashing** (prototype lib.rs:1307-1507). Per dimension: colors init from local digests; iterate color ← digest_record("wl.signature", own color + sorted multisets of (role,label,ordinal→Structure-only, neighbor color) over in/out internal edges); stop when class count stabilizes (signatures include own color ⇒ split-only ⇒ count-stable = partition-stable; cap |M| rounds). Component digest ("wl.component") = sorted multiset of final colors + (Structure) quotient-edge multiset. Condensation DAG ("component.tree") hashed with byte-sorted records — no keys, no indices. Member digest ("node.component_context") = key?(identity) + local + final color + component digest. Headline test: isomorphic recursive SCCs with different key spellings hash equal anonymously.
2. No salsa/fe coupling.
3. Claims layer exists (separate crate).
4. ViewMode kept as policy axis (identity-as-dimension rejected: keys appear in every record kind; tradeoff in rustdoc).
5. Component *index* removed from node.component_context (Tarjan-order anonymity leak, prototype lib.rs:973).
6. Child *ordinals* carried into SCC edge records (prototype dropped them under CondenseScc).
7. `dimensions` removed from HashPolicy/policy_id (was in the id but never in payloads → incomparable references). Dimension set lives in `DigestRequest` (execution) and `Facet{policy_id, dimensions}` (contract) with `facet_id()`; `FacetAddress{facet, digests}.address_digest()` — "equal at facet F" = address_digest equality. `ArtifactRef` uri: `riffcat:1:<policy>:<dim>:<digest>`. Named constructors say what they *forget*: `names_blind() = ALL \ {Names}` (core agent's {Structure}-only version was wrong — forgot constants/types too).
8. Cycle policies actually distinct: Reject (children+Dependency acyclic) / NonRecursiveGraphEdges (children acyclic; Dependency cycles tolerated as flat records) / CondenseScc (WL+condensation). Prototype's first two were behaviorally identical.
9. Algorithms run on u32 ids over an `IndexedGraph` view; **invariant: no u32 id ever enters a hash payload**. Tarjan + tree walks iterative (solc ASTs get deep).

Record tags frozen by golden tests (hex constants; change ⇒ SCHEMA_VERSION bump): `riffcat.policy`, `riffcat.facet`, `riffcat.facet_address`, `riffcat.claim`, `node.local`, `node.tree`, `wl.signature`, `wl.component`, `component.tree`, `node.component_context`, `graph.full`. `DigestIndex` + `FacetIndex` are flat entry records, JSONL-ready.

## riff-catalog-claims (≈800 LOC incl. tests)

`Relation::Equivalent` (#[non_exhaustive]); `Witness{kind: Name, payload: Vec<(String,String)> /*ordered*/}`; `Claim{schema_version, relation, facet, left/right: FacetAddress, witness, note /*excluded from claim_id*/}`; `ClaimSet` (dedup by claim_id) → `closure_for_facet()` (exact facet_id match only) → `FacetClosure` (interned union-find, deterministic min-digest representative, `supporting_claims` per class); `ClaimGatedIndex.lookup(address) → {direct, via_claims, supporting_claims}`.

Plus (per I13) unary **`Attestation{subject: FacetAddress, property: Name, witness, note}`** with `attestation_id()`; `AttestationSet::subjects_with(property) → BTreeSet<Digest>`; gated query = structural bucket ∩ attested. Tests: wrong-witness-still-merges; facet scoping; transitivity; note-excluded-from-id; unattested-correctly-excluded.

## Ingestion

### riff-catalog-solc (~500 LOC, no core dep)
`Pipeline{Legacy,ViaIr}`, `CompileOptions{pipeline, optimize, runs, evm_version}`; `solidity_input()` (outputSelection: `"":["ast"]`, `"*":["evm.bytecode.object","evm.deployedBytecode.object"]` + viaIR: `"ir","irAst","irOptimized","irOptimizedAst","yulCFGJson"`); `yul_input()` (evm.* + `yulCFGJson` — verified available for language:Yul); `with_output_selection()` for sourcify verbatim settings. `SolcRunner::locate` (explicit > $RIFFCAT_SOLC > $FE_SOLC_PATH > PATH), `version()` (semver, strip pre-release), `compile()` (spawn --standard-json, stdin pipe — pattern from fe's solc-runner). `CachedSolc` keyed sha256(canonical-input + version) → offline re-runs. `SolcOutput` = thin typed accessors over raw Value (`check_errors`, `source_ast`, `ir`, `ir_ast`, `ir_optimized_ast`, `yul_cfg_json`, `bytecode`, …). `SolcResolver` trait; v1 `InstalledSolc{accept: "^0.8"}`; argotorg/solc-bin downloader implements it later.

### riff-catalog-yul (~1,400 LOC) — two levels, one crate
**Level "yul-ast/1" (tree).** One shared typed AST (`ast.rs`, 16 structs, serde `#[serde(tag="nodeType")]` matching solc irAst JSON exactly — verified quirks: `parameters`/`returnVariables` keys omitted when empty; `YulCase.value` = literal object or bare string `"default"`; `Literal.type` is `""` (untyped EVM dialect) → `Option` via custom deserializer; string literals carry `hexValue`; `YulData` has no `name` key). Two front doors, NO trait: `serde_json::from_value::<Object>` for solc JSON; own lexer (~150 lines) + recursive-descent parser (~300 lines, grammar: object/code/data, block, fundef, let, assignment, if, switch/case/default, for, break/continue/leave, call, ident, literal w/ optional `:type`) for text (.yul files, fe output, `ir` text). Derived `PartialEq` (no src fields in the model) ⇒ **conformance level 0 is `ast_from_json == ast_from_text`**, before graphs even exist. `canon.rs`: `canon_number` (dec/hex → minimal 0x-hex via simple base-10→bytes accumulation, no bigint dep — 77-digit decimals verified in corpus), `canon_string` (unescape → 0x-hex; JSON path uses hexValue), shared by the ONE lowering. `builtins.rs`: sorted EVM-dialect builtin table + `verbatim_` prefix. `lower.rs` dimension table: object/code/block/function/let/assign/if/for/switch/case/break/continue/leave/call/ident/lit → kinds `yul.*`; names→Names, canon literals→Constants, literal kind + builtin callee→Structure, `:type` suffixes→Types; non-builtin calls get `Reference` edges to same-tree callee fn nodes (name→key post-pass) so recursion forms SCCs. Emits one `yul-object` graph + one `yul-fn` graph per FunctionDefinition (I16).
**Level "yul-ssa-cfg/1" (SSA).** `ssa.rs`: serde structs for yulCFGJson (objects → blocks/functions/subObjects; Block{id, instructions{op,in,out,literalArgs}, exit{type,cond,targets,returnValues}, entries, liveness}; functions{arguments, entry, numReturns, blocks}). Lowering: block → `yul.ssa.block` node; instruction → ordered child, op→Structure (builtin) or Names+Call-edge (helper call resolved via function registry); literal `in` entries and `literalArgs`→Constants; SSA value wiring encoded positionally (operand k of instruction i references the defining instruction node via `Data` edge — never `vN` text, per I15); jump targets → `Dependency` edges (block-level cycles → SCC → WL — the showcase); **phi canonicalization: PhiFunction args paired with `entries` predecessors as labeled (pred-edge, value) pairs, not raw order**; exits → Structure fields + edges; liveness ignored (derived data). Emits `yul-ssa-fn` graphs + object graph. No parser involved — works for BOTH Solidity-viaIR and direct .yul via solc. Caveat (documented): output is experimental and churning under Moritz's active PRs — the `/1` level string + golden fixtures fence it; schema drift = bump to `yul-ssa-cfg/2`, old digests remain valid strangers (I8).

### riff-catalog-solidity (~600 LOC)
**Generic profile walker, decisively** (typed structs = ~150 version-fragile structs; verified instance: `isSimpleCounterLoop` appeared mid-0.8.x). `NodeSpec{kind, children(field→label, declared order = ordinals), names, structure, constants(Extract), types(Extract), graph_root}`; walker: DFS over Value, unknown fields never read, unknown nodeType = error in `--strict` / `sol.unknown.<type>` fallback otherwise; `null` and absent lower identically; `TupleExpression` null components → `sol.hole` placeholder (arity preserved). `id` recorded only in a side map; `referencedDeclaration` → **`Reference` edges** to path-keyed nodes (recompilation-stable; gives WL real connectivity), never fields. Globally skipped: id, src, nameLocation(s), scope, documentation, typeDescriptions-other-than-typeString, selectors, isSimpleCounterLoop, etc. Full profile table for the verified corpus inventory (SourceUnit, ContractDefinition, FunctionDefinition, Block, UncheckedBlock, IfStatement, ForStatement, Assignment, BinaryOperation, UnaryOperation, Conditional, TupleExpression, FunctionCall(+names→argname fields), MemberAccess, IndexAccess, Identifier, Literal(canon via shared rules), Elementary/Array/Mapping/UserDefined type names, Struct/Enum/Event/ErrorDefinition, InlineAssembly→embedded Yul rows, + sourcify-proofing rows: Modifier*, While/DoWhile, Try, ImportDirective, UsingFor, FunctionCallOptions, IndexRangeAccess, UDVT) — exact field→dimension assignments as designed by the ingestion agent (full table in its report; lands in `profile.rs`). `typeDescriptions.typeString`→Types everywhere: `uint256→address` flips Types while Structure survives — a great diff row. Emits `sol-contract` + `sol-fn` graphs.

### riff-catalog-evm (~100 LOC)
`lower_bytecode(owner, Creation|Runtime, bytes)`: `evm.instruction` child per pc; opcode→Structure `"0x{op:02x}"`; PUSH immediates→Constants. Replicates fe's codegen/src/shape.rs precedent.

### riff-catalog-sourcify (~400 LOC)
`SourcifyClient` (ureq+rustls, base `https://sourcify.dev/server`), `fetch(chainId, address)` cache-first (`$XDG_CACHE_HOME/riff-catalog/...`); v2 endpoint `GET /v2/contract/{chainId}/{address}?fields=sources,metadata,stdJsonInput,compilation`, legacy `/files/any/...` fallback (endpoints isolated in api.rs, re-verify at impl). `to_standard_json`: stdJsonInput verbatim if present, else reconstruct from metadata; force our outputSelection. Compile via `SolcResolver`; version mismatch → clean skip message. Default = verified settings; `--force-ir` additionally compiles viaIR for IR facets (labeled as not-the-verified-artifact).

## riff-catalog-cli (bin `riffcat`, ~1,300 LOC)

JSONL corpus (`corpus/<artifact_id>.jsonl`): `artifact` / `graph` / `digest` / `claim` / `attestation` records. Exactly **two hash policies at ingest** per level: `identity` (IdentityBound) and `shape` (AnonymousShape), all dimensions, CondenseScc — query-time facets are just dimension subsets compared as tuples (free facet exploration, I9). Node-key scheme (I15-stable): owners `sol:<unit>:<Contract>`, `yulir:<unit>:<Contract>:<ir|iropt>`, `yulssa:...`, `yul:<file-stem>`, `evm:...:<legacy|viair>:<opt|noopt>`, `sf:<chainId>:<addr>:<Contract>` — pipeline/opt coordinates live in the owner (identity), never fields. Locals are structural paths (`o.0/fn:transfer/c:0.3`, `ct:ERC20/fn:function:transfer(address,uint256)`, `pc:14`) — never solc ids; stable across recompilation/comment edits (tested).

Commands: `ingest <paths|--sourcify chain:addr> [--pipeline both] [--optimize both] [--units fn,contract,ssa,evm] [--strict] [--label]` · `bucket [--unit][--mode][--facet][--claims][--min-size][--top]` (dedup classes table + %) · `overlap <A> <B>` (bipartite twin table, Jaccard) · `diff <A> <B> [--name fn]` (per-dimension survival matrix) · `conformance <dir>` (level 0: AST equality text-vs-JSON; level 1: every digest equal across both paths; level 2: Solidity→yulCFGJson vs ir-text→solc-as-Yul→yulCFGJson; nonzero exit on drift) · `claim add/list` + `attest add/list` (wrong-witness demo: claim two different fns equivalent → bucket --claims shows basis=claim merge; diff prints "claimed-equal, digest-distinct"). Global `--json`.

## Implementation sequence

| # | Build | Gate |
|---|---|---|
| M1 | core: scaffolding, primitives, encode, golden policy_id | golden hex constants frozen |
| M2 | core: graph model | dup/missing/validate tests |
| M3 | core: acyclic hashing | ported suite: dimension purity, order (in)sensitivity, identity/anon separation, cycle policies distinct |
| M4 | core: SCC+WL+condensation | isomorphic-SCCs-different-keys equal anonymously; non-isomorphic differ; orbit determinism; ordinal regression |
| M5 | core: references+indexes; claims crate (incl. attestations) | JSONL round-trip; wrong-witness-merges; facet scoping; unattested-excluded |
| M6 | solc driver | rosetta ERC20 compiles both pipelines; ast/irAst/ir/yulCFGJson present |
| M7 | yul ast.rs + JSON path | all 9 rosetta × opt on/off deserialize clean; fixtures checked in |
| M8 | yul lexer+parser | **conformance level 0** green on 9 contracts × {ir, irOptimized} × opt; testdata/*.yul round-trip |
| M9 | yul canon+lower (tree level) | conformance level 1 (hash equality) on erc20+math; first bucket smoke: `panic_error_0x41` identical across contracts |
| M10 | solidity profile+walker | 9 ASTs lower --strict; stability: recompile → identical identity digests; comment insertion → unchanged; var rename → only Names moves |
| M11 | cli ingest/bucket/overlap/diff + evm | full rosetta matrix ingested; cross-contract helper dedup % measured; opt-vs-noopt survival matrix renders |
| M12 | **SSA level** (ssa.rs + lowering) | yulCFGJson ingested for .sol AND direct .yul; loop fn (rosetta math) produces SCC, WL exercised; conformance level 2 green |
| M13 | conformance command wiring | `riffcat conformance rosetta-fe/examples` exits 0; perturbing canon_number locally makes it fail loudly |
| M14 | sourcify | fetch one verified 0.8.x mainnet ERC20; second run offline; overlap vs rosetta ERC20 at shape facet |
| M15 | claims demo | scripted wrong-witness narrative end-to-end |

LOC estimate: core ~2,400 · claims ~800 · solc ~500 · yul ~1,400 · solidity ~600 · evm ~100 · sourcify ~400 · cli ~1,300.

## Verification

- Per-milestone `cargo test`; golden digest fixtures freeze the encoding contract (I8)
- Three-level conformance harness on the rosetta corpus = the convo's hazard-#1 drift detector, demonstrably failing when perturbed (M13)
- Demo narratives: cross-contract solc-helper dedup at names-blind facet; unopt-vs-opt per-dimension survival; fe-relevant direct-Yul ingestion via SSA level; sourcify dogfood; claims merge + attestation gating + wrong-witness honesty
- fe-fit check (no fe code in scope): OriginExportKey maps losslessly into EntityKey at the boundary

## Reference material

- Core + ingestion design reports: agents' full outputs (this plan condenses them; the Solidity profile table and claims API details live in the ingestion/core agent reports respectively — both reproduced faithfully in crate docs at impl time)
- Prototype: `fe-worktrees/origin-overhaul-phased/crates/shape-address/{src/lib.rs,SPEC.md}`; key flaws at lib.rs:1307-1507, :973, :1532
- solc driver prior art: `fe-worktrees/origin-overhaul-phased/crates/solc-runner/src/lib.rs`; bytecode lowering: `crates/codegen/src/shape.rs`
- Moritz's SSA CFG: argotorg/solidity PRs #16646–#16767; printer #16709, callgraph #16714
- Corpus: `~/hacker-stuff-2023/fe-stuff/rosetta-fe/examples/*/sol/*.sol` (9 contracts, all verified compiling viaIR both opt levels)
