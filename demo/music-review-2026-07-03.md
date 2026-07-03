# Music chapters review, 2026-07-03

Independent review of the MUSIC-chapter redesign (commits `00f937c`, `7d0233f`,
`a406e64`, plus `e3a7dd0`/`639c645` just before). All math below re-derived by
hand, then baked into Rust tests so the checks are durable. Local review file,
per standing practice; nothing posted anywhere.

Reviewed surface: `crates/riff-catalog-music/src/set_theory.rs`,
`crates/riff-catalog-music/src/chord.rs`, `crates/riff-catalog-wasm/src/lib.rs`,
the music chapters of `demo/app/app.js` (chord-fp, forte-catalog,
ivf-fingerprint, three-rungs, anchor-transport, verified-attest skimmed;
metadata-axes / byte-wall / shared-block skimmed), `cubical/Riffcat/Music.agda`,
`cubical/Riffcat.agda`, `lean/` (no music witness exists there; checked).

## (a) Categorical / correctness findings

### 1. `prime_form` is genuine Rahn. CORRECT.

`normal_order_zeroed` compares rotations by span (last element) first, then
inward from the right, smaller wins: that is Rahn's most-compact rule.
Hand-checks against the published catalog, all confirmed and all now asserted
in `tests/demo_forte_catalog.rs`:

- major {0,4,7}: rotations zeroed [0,4,7]/[0,3,8]/[0,5,9], spans 7/8/9, normal
  order [0,4,7]; inversion {0,5,8} normalizes to [0,3,7]; prime = [0,3,7] = 3-11.
- minor 7th {0,4,7,9}: rotations [0,4,7,9]/[0,3,5,8]/[0,2,5,9]/[0,3,7,10],
  spans 9/8/9/10, normal order [0,3,5,8]; inversion {0,3,5,8} also [0,3,5,8];
  prime = [0,3,5,8] = 4-26. The old lex-least code returned [0,2,5,9]; the
  regression test in `set_theory.rs` guards the fix, and the exhaustive orbit
  test below guards the whole space.
- dominant 7th {2,5,7,11}: normal order [0,3,6,8] (span 8 beats 9/9/10);
  inversion {1,5,7,10} normalizes to [0,2,5,8]; tie at span 8 broken inward
  from the right (6 vs 5), prime = [0,2,5,8] = 4-27.
- major 7th {0,4,7,11}: two span-8 rotations [0,3,7,8] and [0,1,5,8]; inward
  tie-break (7 vs 5) picks [0,1,5,8] = 4-20. Augmented [0,4,8] = 3-12,
  diminished [0,3,6] = 3-10: all as published.

Note on naming: the engine implements Rahn's packing; Forte's original differs
on a handful of classes (5-20, 6-Z29, 6-31, 7-20, 8-26 territory), none of
which the demo displays, so every rendered value coincides with the published
Forte table. The wasm comment that called this "the Forte prime form" now says
Rahn most-compact.

### 2. One normal-order rule, not two. CORRECT, now test-guarded.

The A/B letter derives from comparing `transposition_normal_form` (min 12-bit
bitmask over rotations) with `prime_form` (built on `normal_order_zeroed`,
most-compact). If those two canonicalizations ever diverged, the letter would
be unsound. They cannot: the minimal mask must contain pc 0 (otherwise
transposing down one semitone strictly decreases the integer), and comparing
equal-cardinality masks as integers is exactly lexicographic comparison of the
descending pc sequences, i.e. span first, then inward from the right, which is
`more_compact` verbatim. `normal_order_and_min_bitmask_are_the_same_rule_exhaustively`
asserts equality over all 4095 nonempty pitch-class sets. Passes.

### 3. `prime_form`'s final pick. CORRECT, one observation.

The code picks between the two zeroed normal orders by left-lexicographic `<`,
while strict Rahn uses the same right-inward packing order for this pick too.
The two candidate normal orders always share a span (inversion reverses the
cyclic gap sequence, preserving the maximal gap, and span = 12 minus max gap),
but for equal-span vectors left-first and right-first comparison can in
principle disagree. Exhaustively they never do here:
`prime_form_is_the_min_bitmask_of_the_tni_orbit_exhaustively` pins
`prime_form` to the definitional Rahn reference (min bitmask over all 24
transformations of the orbit) on all 4095 sets, and
`prime_form_canonicalizes_the_tni_orbit_exhaustively` re-proves idempotence
and orbit-constancy. All pass, so the left-lex shortcut is safe over the whole
space and any future edit that breaks this dies in CI. No code change needed.

### 4. The A/B derivation in `forteLabel`. CORRECT.

- letter A iff Tn-type == prime form, B otherwise: matches the standard
  lettered convention (3-11A = [0,3,7] minor, 3-11B = [0,4,7] major; 4-27A =
  [0,2,5,8] half-diminished, 4-27B = [0,3,6,8] dominant). Since the prime form
  is always one of the two Tn-types, exactly one side of an asymmetric pair
  gets A.
- no letter iff Tn-type(set) == Tn-type(inversion): that equality holds iff
  the set is TnI-symmetric, which is precisely when A and B coincide. Correct.

### 5. Displayed labels. ALL CORRECT (and now pinned in tests).

C = 3-11B, G = 3-11B, Am = 3-11A, Caug = 3-12, Bdim = 3-10, Cmaj7 = 4-20,
G7 = 4-27B, Am7 = 4-26. Symmetry worked out by hand: inv{0,4,8} = {0,4,8};
inv{2,5,11} = {1,7,10} = T11{2,5,11}; inv{0,4,7,11} = {0,1,5,8} =
T1{0,4,7,11}; inv{0,4,7,9} = {0,3,5,8} = T8{0,4,7,9}. So exactly the
augmented, diminished, major-seventh, and minor-seventh cards carry no letter,
as rendered. Invert toggle: inv(C) and inv(G) land on 3-11A, inv(Am) on
3-11B, inv(G7) on 4-27A, symmetric four fixed. All asserted in
`tests/demo_forte_catalog.rs`.

### 6. `FORTE_NAMES` keys. CORRECT.

The six keys ("0 3 7", "0 4 8", "0 3 6", "0 1 5 8", "0 3 5 8", "0 2 5 8") are
exactly the prime-form strings the engine emits for the displayed chords: no
dead entry, no card that would silently lose its Forte number.
`forte_names_keys_are_exactly_the_engine_prime_forms_of_the_demo_chords`
guards the correspondence.

### 7. "Cdo" and the chapter-2 badge. CORRECT.

The grammar parses "Cdo" as root C + quality token "do"; the quality tables
treat it as a plain major triad, so it maps to [0,4,7] (unit test in
`chord.rs`, re-asserted in the demo test). Not a parse bug. The badge reads
`Tn [0 4 7] · prime [0 3 7]`, so the [0 3 7] under a major triad reads as the
fold target, and the caption says exactly that ("the major triads sit at Tn
[0 4 7] and the minor at [0 3 7], yet all of them fold to the one prime
[0 3 7]"). The claimed grouping (C = Cdo = Am = F at set class; Caug, Bdim
alone) is true and now asserted in `chord_fp_chapter_grouping_matches_its_prose`.

### 8. Interval vectors. ALL CORRECT.

Hand counts match published values and the live rendering: major/minor
<001110>, augmented <000300>, diminished <002001>, maj7 <101220>, dom7
<012111>, min7 <012120>. Asserted per chord in the demo test.

### 9. BUG (comment, fixed): the FORTE_CHORDS comment block said "the dominant
and minor sevenths collapse to 4-27". The minor seventh is 4-26; 4-27's
partner is the half-diminished seventh. A future editor reading that comment
would have re-baked a wrong fact. Rewritten.

### 10. BUG (cross-witness, fixed): `cubical/Riffcat/Music.agda` normalized by
plain left-packed lexicographic order and its header called that "the Forte
form". That is the exact convention `00f937c` removed from Rust: on the minor
seventh it computes [0,2,5,9] where the engine and the published catalog say
[0,3,5,8], and its Tn normal form of a major triad was [0,3,8], not the
demo's A/B form [0,4,7]. Nothing previously proved was false (the triad facts
checked are representative-independent, and lex-least agrees with Rahn on
3-11/3-12), but the witness embodied the old convention and would have
diverged from the engine on the first tetrachord anyone added. Ported the
normalization to the engine's rule (minimal as a 12-bit integer, pc 11 most
significant), moved the A/B projection from position 8 to position 4 (the Tn
forms are now [0,4,7] vs [0,3,7] on the nose), and added `Aminor7-prime :
primeForm {0,4,7,9} == [0,3,5,8]` by refl, surfaced as a headline in
`Riffcat.agda`. Typechecks green (exit 0). The refl proofs computing is an
independent machine confirmation of the hand math in finding 1.

No Lean music witness exists (`lean/` grepped), so nothing to reconcile there;
the verified-attest chapter cites the external polyphonotopes-math Lean
development and is explicitly labeled a static placeholder.

## (b) Intent-alignment findings

- Compute, don't bake: the redesign holds. `FORTE_NAMES` is the single baked
  fact (a published name, keyed on engine output), and `forteLabel` derives
  id/fold/letter/symmetry from the engine pair. Two residual bakes found and
  removed: three-rungs carried hard-coded "3-11B"/"3-11" badge strings and
  hard-coded Tn/prime vectors in its caption (now derived via
  `fingerprint_pcs` + `forteLabel`), and forte-catalog grouped/colored by a
  JS-joined Tn-type string rather than the engine's `transposition_normal`
  facet address (now the address itself, which is also truer to the "content-
  addressed by the same facet machinery" claim in its own caption).
- MISALIGNMENT (fixed): the forte-catalog chapter lede still told the
  pre-redesign story ("watch major and minor fall into one class") which the
  A/B redesign deliberately made false at that stop. Rewritten to the A/B
  story.
- Stale scaffolding (fixed): three-rungs still shipped the "this rung is
  awaiting an engine field" fallback and a comment pointing at engine_needs;
  `fingerprint_pcs`/`transposition_normal` landed, so that is dead. Removed.
- Presentability gap (fixed, flag for sign-off): after the redesign, no two of
  the seven forte-catalog cards shared a group at either stop, so the page's
  grouping-and-hover gesture (the demo's central "collapse" move) could never
  fire. Added G major: C = G at one 3-11B Tn-address (transposition folded)
  while Am sits apart as the mirror, which is exactly the lesson the stop
  teaches. If an eighth card is unwanted, revert the FORTE_CHORDS entry and
  the "Eight named chords" lede sentence (tests cover both shapes).
- Small reference fix: chord-fp's caption said "the next chapter" splits the
  fold into A/B; the forte-catalog chapter is ten chapters later. Now "later
  in the tour".
- No em-dashes: zero U+2014 in app.js, the music/wasm crates, or the cubical
  files, before and after my edits.
- Register: prose stays in plain public vocabulary (standard music-theory
  terms plus the demo's facet language); nothing internal found.
- Protected paths: `demo/corpus`, `demo/stdlib`, `corpus/`, the schema/facade
  crates, and all trace/ingest surfaces untouched.
- Pre-existing, deliberately untouched: the metadata-axes comment attributes
  the Main-vs-Meta framing to @kuzdogan (#1643); that sits on the held
  epigraph sign-off list and is not this review's to move.

## (c) What was tightened (commits, newest first)

- `64a1723` cubical: normalization ported to Rahn most-compact (shared with
  the engine), A/B projection updated, 4-26 refl witness added, README
  updated. Typechecked, exit 0.
- `6fe4789` demo: forte-catalog lede rewritten to the A/B redesign; wrong
  4-26/4-27 comment fixed; grouping/color keyed on the engine's
  transposition_normal address; symmetric cards labeled "Tn-type = prime";
  G major added so the grouping gesture has a real collapse; readout uses
  notations; three-rungs de-baked (badges and caption values derived from the
  engine) and its stale fallback removed; chord-fp "next chapter" fixed; wasm
  comment names Rahn. Rust demo test extended to the eighth card.
- `6d1d352` music tests: three exhaustive sweeps (one-normal-order-rule,
  Rahn-orbit-minimum, canonicalization laws) plus the demo-catalog
  integration test replaying `forteLabel` in Rust against the published
  catalog, including the invert toggle and the FORTE_NAMES key correspondence.

## (d) Verification

- `cargo test -p riff-catalog-music`: 19 passed (15 unit incl. the three
  exhaustive sweeps over all 4095 sets, 4 integration), 0 failed.
- `cargo test -p riff-catalog-wasm`: 3 passed. No binding signatures changed
  (one comment edit), so no wasm rebuild was required; `demo/app/dist/app.js`
  re-synced with the edited source (dist is gitignored).
- `cargo test --workspace`: green after all edits (every suite ok, 0 failed,
  cargo exit 0; one pre-existing ignored test, unrelated to music).
- `agda Riffcat.agda` (cubical prototype): exits 0 after the Music.agda port;
  every refl in the module recomputed under the new rule.
- `bun build --no-bundle demo/app/app.js`: syntax clean.
- Live screenshots: not taken; the host-side Chrome debug endpoint is down
  (needs `garden-host-chrome` on the host) and no dev server was up. Rendered
  strings were verified statically and the label math is test-pinned instead.
