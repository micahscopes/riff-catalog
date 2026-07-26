# The chapter pool, and how to draft an arc

The deck is a **pool** of chapters plus an **arc** that orders them. The pool is
`CH` in `demo/app/app.js`; an arc is one entry in `ARCS`, and `?arc=<key>`
selects it. An arc only names the beats it wants: whatever it leaves out lands in
a final "for follow-up questions" section, so a short arc never loses a chapter.

To draft one, write a list of sections. Nothing else:

```js
mine: {
  label: "mine",
  sections: [
    ["the problem", ["many forms", "broken reference"]],
    ["the payoff",  ["recognized", "sniff it out"]],
    ["the ask",     ["what we need"]],
  ],
},
```

Order inside a section is the order it steps. A nav name that is not in the pool
throws on load, loudly, rather than silently dropping a slide.

## The pool

`live` = calls the wasm engine in the browser on this run. `real` = precomputed
by `tools/gen-*.mjs` from real verified sources, never hand-authored. `drawn` =
an illustration with no data behind it, honest as setup, never as evidence.

| nav | what is on the slide | |
| --- | --- | --- |
| many forms | solc and fe pipelines side by side, plus model, trace, verified source | drawn |
| stranded | four findings, each pinned to the one form it came from | drawn |
| broken reference | a proof naming IR node 184, then the node is gone | drawn |
| facets | five small functions; loosen the facet and watch them merge | live |
| the riff | a motif and its variants, then chords parsed from real notation | live |
| address it | keep structure/types/origins, derive one address | drawn |
| recognized | verified Sourcify contracts, functions colored by the library they match | real |
| twins | one library function, everywhere it recurs across contracts | real |
| dedup | ten contracts: how much is known shape, how much is new to audit | real |
| sniff it out | the ERC-4626 inflation shape across verified vaults, patch vs bug | real |
| modified | edited forks that exact match and text search miss | real |
| the compiler too | the Yul a contract compiles to, fingerprinted | live |
| provenance | origin edges through lowering; compare two lowerings of one contract | live |
| two fingerprints | Sourcify's metadata hash vs the structural one, as you edit | live |
| prove it | the obligation a match hands a verifier, per tool | real |
| anchors | pin a fact to an address, watch where it rides and where it rides wrong | live |
| prior art | URIs, Lurk, Ix, Forte: the same move, reached separately | cited |
| locality runs out | cycles and context-dependent instantiation, the two real limits | real |
| what we sampled | every number in the deck, what it is, and what we still owe | real |
| what we need | the three legs: compiler, riffcat, verifier | drawn |
| a shared block | the library surface offered for co-design | real |
| structure vs meaning | the facet lattice, and why it stops short of meaning | live |
| the fold | how a node digest folds bottom-up into an address | live |
| the cheap yes | hevm's byte-identical fast path, generalized to a facet | real |
| seat filled | a cited EquiVM Lean proof taking the open seat (not run here) | cited |
| the proofs check | the kernel-checked proof that the normal form is canonical | cited |
| the engine checks too | Rust and Lean hashing one corpus in lockstep (a plan) | cited |
| two instantiations | one generic, two ways: where structure agrees, types part | live |
| main vs meta | code and metadata interleaved in one bytecode strip | real |
| forte catalog | eight chords landing on the published catalog, A/B and all | live |
| interval vector | six numbers that survive transposition and inversion | live |
| three rungs | one chord, three rungs of forgetting | live |

## Layouts a new slide can use

A hand-written slide does not need a component. These CSS layouts are already in
`index.html` and take plain markup, so a new beat is a few lines of `body`:
`compiler-paths` (labelled pipelines), `result-attachments` (things pinned to
forms), `broken-reference` (before/after pair), `address-proposition`
(choice → derived address), `meaning-grid` (three questions, three answers),
`role-split` (two roles side by side), `identity-boundary` (two or three kinds of
identity), `shared-reference` (one center, many consumers), `limit-pair` (two
honest limits), `possibility-grid` (four short possibilities), `lockin-visual`,
`private-maps`, `figure` + `table` (a captioned table).

## The arcs that exist

- **talk** (default): problem, the choice, on real code, through the compiler,
  from match to knowledge, honesty and the ask. Every payoff beat is live or
  real; the three drawn slides are setup only.
- **short** (`?arc=short`): the same spine in ten beats, for a short slot. One
  problem slide, one mechanism, two payoffs, the compiler beat, the boundary, the
  ask. Everything else is one click away in the appendix.
- **storybook** (`?arc=storybook`): the domain-coherent order the deck grew up in
  (all music together, all code together). Good for browsing, not for a talk.

Earlier draft-4 experiments (three abstract arcs, hand-drawn IR and bytecode
columns) are in git at `7552b5e` if a framing there is worth reviving. The
authored-data components they leaned on were removed, on purpose: in a room with
compiler and verification people, a drawn stand-in for something the engine can
actually compute costs more than it buys.
