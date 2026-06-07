# riff-catalog lightning demo — plan (draft for ultraplan refinement)

## 1. Context and goals

Seven-minute live demo + Q&A for the Argot team. The backdrop is the "too
many compilers" funding tension: the pitch is NOT another compiler or
another IR — it's the substrate that makes the compiler count stop being a
blocking question, because artifacts interoperate at the level of *witnessed
structural identity* instead of shared toolchains.

Walk out with:
1. The team understanding "equal at facet F" in their bones (from seeing it,
   not from definitions).
2. The distributed-systems person engaged on the canonical-encoding and
   conformance story (and probing it — that's engagement).
3. The Sourcify person seeing at least one concrete product feature they
   want ("similar contracts" / partial-verification triage).
4. Zero people scared that we're building a new IR, a database, or a
   standards body.

Language discipline (hard rule, learned the hard way): no category theory,
no HoTT, no "quotient", no "kernel". Even "content addressing" gets
introduced through the demo result, not as a slogan. The words we do use:
*fingerprint, facet, rhyme, twin, witness, claim*.

## 2. Audience map

| persona | background | the hook | the trap to avoid |
|---|---|---|---|
| Argot generalists | application-focused, funding-anxious | "your compilers already emit the same code constantly — watch 536 functions collapse to 216" | abstraction talk before the table renders |
| Solidity member, **distributed-systems** background | consensus, replication, determinism | the conformance loop: two independent ingestion paths, byte-equal canonical form, drift detector that *visibly fails* when perturbed; WL-refined hashing of cyclic graphs (Merkle alone can't); self-describing references with schema versioning | overclaiming: 1-WL ≠ isomorphism — say it before they do |
| Sourcify **frontend** dev | verified-contract corpus, product UX | live fetch of ENS PublicResolver → 39 structural twins against a local build; pitch: "similar contracts" tab, helper-provenance badges, and Constants-blind bytecode matching for *unverified* contract triage | API/scale promises we can't keep yet (be explicit the store is JSONL today) |

Bonus thread for whoever is close to the new codegen pipeline: the SSA
level consumes `yulCFGJson` as a first-class citizen — their newest work is
load-bearing here, not bypassed.

## 3. Narrative arc (three acts, 7 minutes)

**Act I — "Your compilers rhyme" (2 min).** No slides. Terminal with a
pre-ingested corpus (all 9 rosetta contracts, both optimizer settings, all
four levels). Run `bucket`, let the dedup table land, name what they're
seeing: every digest is computed *per dimension* — structure, names,
constants, types — so "names-blind" or "constants-blind" is a query-time
choice, not a re-hash. Then `overlap` ERC20↔AMM at names-blind:
`external_fun_balanceOf` twins `external_fun_swapAForB`. One sentence on
units: functions, contracts, whole objects, SSA CFGs, raw bytecode.

**Act II — "And we can prove the fingerprints mean something" (2.5 min).**
For the distsys listener. Two *independent* paths produce each Yul artifact
(solc's JSON AST vs our text parser; Solidity→SSA-CFG vs ir-text→solc-as-
Yul→SSA-CFG). `conformance` runs all three levels: GREEN. Then the money
shot: perturb one line of literal canonicalization in a scratch build (or
play the 15-second recording of doing so) and show conformance go red with
a named digest mismatch. "Canonical-encoding drift is the #1 way systems
like this die silently; ours dies loudly." Mention in passing: loops and
recursion hash via SCC condensation + Weisfeiler-Leman refinement — Merkle
trees alone can't fingerprint cyclic structure; and anonymous equality is
"WL-equivalent", *not* isomorphic — known, documented, and exactly what the
claims layer is for.

**Act III — "Real world + things hashing can't see" (2.5 min).** Fetch is
pre-cached: `ingest --sourcify 1:0x231b…` (ENS PublicResolver, exact_match,
recompiled locally) then `overlap ":sf" "yulir:ERC20"` → 39 shared classes
between a mainnet-verified contract and a local build. Pivot to the
Sourcify pitch (one breath): same machinery, Constants dimension absorbs
PUSH immediates — so structure-facet matching of *unverified* bytecode
against the verified corpus is a triage feature: "94% standard helpers,
review these 3 novel functions." Close with claims: add a deliberately
bogus equivalence with a witness, `bucket --claims` merges 84→83, `claim
list` shows exactly who asserted what on which witness — *claims are
inputs, not discoveries; validity is the auditor's job, attributability is
ours.* Then one attestation (`verified-total`, witness `lean-proof`) and
`bucket --require verified-total` excludes 534 rows: guarantees gate;
structure stays silent. Name-drop the in-house witness sources: hevm, act,
yul-isabelle.

**Coda (30 s) — what this is NOT.** Not an omnilingual compiler. Not a
shared internal IR (every compiler keeps its own; only the interchange form
is shared). Not a database (JSONL + an existing KV store later). Not a
standards proposal (it has to be load-bearing for *us* first). The fe
sentence, exactly once: "fe-emitted Yul ingests through the same two doors
— the compilers meet at the artifact level, which is the point."

## 4. Run sheet

| t | beat | command (pre-staged corpus) |
|---|---|---|
| 0:00 | cold open, no intro | `riffcat bucket --unit yul-fn --mode shape --facet all` |
| 0:45 | facet flip | `… --facet structure` (84 classes / 84.3%) |
| 1:15 | twins | `riffcat overlap "ERC20.sol" "SoliditySimpleAmm" --facet names-blind` |
| 2:00 | survival matrix, fast | `riffcat diff …ir:noopt …iropt:noopt --name fun_transfer` |
| 2:30 | conformance GREEN | `riffcat conformance rosetta-fe/examples --optimize off` |
| 3:30 | drift demo (live or recording) | perturbed `canon_number` build → red FAIL |
| 4:30 | sourcify | `ingest --sourcify 1:0x231b…` (cached) + `overlap ":sf" "yulir:ERC20"` |
| 5:30 | claims + gating | `claim add … && bucket --claims` ; `attest add … && bucket --require …` |
| 6:30 | coda: what this is NOT | (only slide, 4 bullets) |
| 7:00 | Q&A | — |

## 5. Prep checklist (the actual work — ultraplan: refine/estimate these)

**P0 — must exist before the demo**
- [x] `demo/` directory in-repo — DONE: `stage.sh` (idempotent staging,
      both binaries, cache warming) + `runsheet.md` (the full show,
      offline after staging).
- [x] Output polish — overlap member lists truncate for projectors;
      remaining nice-to-have: colored DIFF cells (P2 below).
- [x] Drift-demo packaging — DONE as `demo/drift.patch` (switch-case
      reorder in the parser: fires on every dispatcher, level-0 red) +
      `target-drift` build in stage.sh. Still TODO: record the 15-second
      fallback screencast.
- [ ] Dry run against a clean checkout + recorded full-run fallback video
      (projector/Wi-Fi insurance).
- [ ] The one slide ("what this is NOT") + a one-page handout: the five
      layers (canonical form → digests → facets → claims → references),
      the dedup table screenshot, repo link.

**P1 — strongly raises the demo's ceiling**
- [x] Multi-version solc resolver — DONE: `SolcBinResolver` downloads the
      exact pinned build from binaries.soliditylang.org (cached forever),
      `PinnedOrDownload` prefers a matching local install. Verified live:
      Permit2 (pinned 0.8.17) fetches, recompiles with the verified
      compiler, and ingests. Demo-able as an Act III beat.
- [ ] Visual companion for Act I/III: revive the facet-hashing playground
      (the React bipartite-overlap view) fed by `riffcat overlap --json` /
      `bucket --json` over the real corpus instead of its toy parser.
      Frontend bait for the Sourcify dev — and it's *their* stack. Scope:
      static HTML + the existing JSX, a `riffcat … --json > data.json`
      pipe, no server.
- [ ] `riffcat bucket --by-contract` summary view: "contract X is N%
      ecosystem-standard helpers / M novel functions" — the verification-
      triage pitch as a live command rather than a sentence.

**P2 — nice, cuttable**
- [ ] Colored TTY output (green `=` / red `DIFF` in the survival matrix).
- [ ] A second sourcify contract pre-cached (e.g. another ENS-adjacent
      resolver) so the ecosystem-overlap claim isn't a single data point.
- [ ] `riffcat explain <selector>` — print one unit's per-dimension digests
      with the facet URIs, for the "what exactly is a reference" question.

## 6. Anticipated Q&A (prepare honest answers, not deflections)

- **"Why not IPLD / git / plain content hashes?"** (distsys) — single-hash
  models give one identity per blob; we need *one identity per facet* with
  the facet traveling in the reference, plus claim-gated equivalence on
  top. Could layer on IPLD for storage; the schema/semantics are the
  contribution, not the KV store.
- **"WL isn't graph isomorphism."** — Correct, and documented: anonymous
  equality means WL-equivalent-under-policy; isomorphic graphs never split;
  adversarial regular graphs can collide; the claims layer exists for
  precisely the residual. (Say this *before* they do if possible.)
- **"yulCFGJson is experimental, you built on sand."** — The level string
  `yul-ssa-cfg/1` fences it; schema churn bumps to `/2`; old digests stay
  valid strangers (every reference is schema-versioned). Churn is priced in.
- **"Scale? Sourcify has millions of contracts."** — Today: JSONL, demo
  scale. The index types are flat records designed to land in any KV store;
  hashing is blake3 over per-function graphs (microseconds); the open
  engineering is corpus-store choice, deliberately deferred (the "don't
  accidentally build a database" rule).
- **"Optimized code matches nothing — isn't that a failure?"** — It's the
  honest answer: optimization changes structure, and the survival matrix
  *shows you which facets survive*. Cross-optimization equivalence is
  claim territory (witness: translation validation / hevm equivalence run),
  not hash territory.
- **"What do the compilers have to agree on?"** — Exactly one thing: the
  canonical interchange form + facet vocabulary (versioned). Not internals,
  not IRs, not storage. That contract is ~2,400 lines with golden tests.
- **"Who owns the contract?"** — Open team decision, on purpose (it's in
  the interop plan as the political crux). The demo's job is to make the
  question worth having.

## 7. Follow-up asks (the last 30 seconds of Q&A, per persona)

- Sourcify dev: a pilot — run the ingester over one chain's verified set,
  publish dedup stats; co-spec the "similar contracts" JSON the UI needs.
- Distsys Solidity member: review the canonical-encoding spec + WL section
  (PLAN.md invariants I1–I10); poke holes in the conformance harness.
- Team broadly: agree the facet vocabulary is worth versioning together;
  nominate the neutral home for the contract.

## 8. Open questions for refinement (ultraplan: resolve or task these)

1. Live drift demo vs recording — live is stronger, riskier; needs the
   scratch-build script to be bulletproof.
2. Does the playground revival make the cut for a *lightning* slot, or is
   it the follow-up-meeting artifact?
3. Which second sourcify contract demos best (want: 0.8.x caret pragma,
   exact_match, structurally near something we build locally)?
4. Should `fe`-emitted Yul appear live (one `riffcat ingest demo.yul`
   beat), or stay a sentence? Depends on whether a current fe build emits
   clean Yul text on the day.
5. Handout: one page or three? (Layer diagram + invariants table + Q&A
   crib, or just the first.)
