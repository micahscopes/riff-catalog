# riffcat vuln-shape reach vs Sourcify exact-match: the ERC-4626 first-deposit inflation shape

Measured 2026-06-23. Defensive-security research only; the bug is public and disclosed.

This file answers one question with a concrete, honest number: how many deployed contracts carry
the ERC-4626 first-deposit-inflation VULNERABLE SHAPE that a Sourcify exact-source match (keyed on
one file's `source_hash`) would NOT group together, but riffcat's structure fingerprint WOULD.

## TL;DR headline

| family | distinct vulnerable source variants | Sourcify exact-match reach (largest single source_hash) | riffcat shape reach (sum of all variants with the same structure fingerprint) | extra contracts riffcat catches (shape - exact) |
|---|---|---|---|---|
| OpenZeppelin (canonical f7a9 ternary) | 8 | 153 | 228 | **+75** |
| solmate (e14a) | 27 | 588 | 1,116 | **+528** |
| combined (these two shapes) | 35 | 741 (153+588) | 1,344 (228+1,116) | **+603** |

Plain English: on the OZ side the gap is real but modest. The single largest exact-source variant
covers 153 compilations; the same vulnerable structure recurs across 8 distinct `source_hash`
variants (reformatted / re-vendored / different directory layout) for 228 total, so riffcat's shape
match adds **75** contracts that an exact-source search keyed on the dominant file misses.

On the solmate side the gap is large. The largest exact-source variant is 588 compilations, but the
identical vulnerable `convertToShares` body recurs across all 27 distinct solmate `ERC4626.sol`
variants for 1,116 total, so the shape match adds **528** (nearly doubling the reach). solmate vaults
are vendored under many directory layouts and reformatted often, so exact-source-on-one-file badly
under-counts the vulnerable population while the structure fingerprint sees through it.

## Caveat that strengthens the OZ number (a SECOND and THIRD vulnerable OZ shape exist)

The "canonical f7a9" OZ shape (the `_initialConvertToShares(...)` ternary, OZ ~v4.4 to v4.8) is not
the only vulnerable-by-shape OZ body. Two structurally distinct but still-vulnerable first-deposit
ternaries also appear in the OZ population and are NOT grouped under f7a9:

- `697d2701e10a` (3 variants, **108** compilations): OZ ~v4.2 to v4.3 era, where the empty-vault
  branch is `assets.mulDiv(10**decimals(), 10**_asset.decimals(), rounding)` instead of routing
  through `_initialConvertToShares`. Same vulnerable first-deposit shape, no decimal-offset / virtual
  -assets mitigation; different structure fingerprint.
- `bd8725d019aa` (4 variants, **28** compilations): the original OZ v4.0/v4.1 *draft* `convertToShares`
  (public, conversion inlined, raw `*`/`/` math, no `mulDiv`). Same vulnerable ternary; different
  fingerprint.

So riffcat would surface these as TWO MORE vulnerable clusters (a strength: it separates vulnerable
*families* rather than smearing them). They do not add to the f7a9 gap above (different fingerprint),
but they mean the total OZ vulnerable-shaped population is larger than the f7a9 row alone:
228 (f7a9) + 108 (697d) + 28 (bd87) = **364** OZ compilations carrying SOME vulnerable first-deposit
ternary, versus 4,227 carrying the v4.9+ patched `_decimalsOffset` shape (435c).

## Method (how each number was produced)

1. **Population (cheap path/source_hash counting on `public_compiled_contracts_sources`).** Files
   named exactly `ERC4626.sol`, grouped by library token in the path. Single ~2.3 GiB scan:
   - openzeppelin: **4,595** compilations across **58** distinct `source_hash` variants
   - solmate: **1,116** compilations across **27** variants
   - solady: 651 / 8 (mitigated by design; not measured per-shape here)
   - other (no library token in path): 921 / 135 (not classified; see floor note)

2. **One witness address per variant (two-step join, pruned to stay under the 6500 MiB cap).**
   - Step 1 (one ~3.3 GiB path scan): map every `ERC4626.sol` `source_hash` -> one `compilation_id`.
   - Step 2 (~4.9 GiB, keyed on those specific compilation_ids): join `verified_contracts` ->
     `contract_deployments` for one `(chain_id, address)` per variant. The one-shot join blows the
     cap (~8.2 GiB); the two-step keeps each query under it.
   - All 85 OZ+solmate variants resolved to a deployed address (0 missing).

3. **Fetch + fingerprint (no BigQuery content scan).** For each variant's witness, pull `ERC4626.sol`
   via the Sourcify v2 API (`/server/v2/contract/{chain}/{addr}?fields=sources`), extract the
   conversion function (`_convertToShares`, or the public `convertToShares` for inlined / solmate
   layouts) by brace-matching, drop it into a faithful solc-0.8.33 harness that stubs the
   dependencies (Math/FixedPointMathLib, totalSupply/totalAssets/decimals/`_initialConvertToShares`),
   compile to AST, and read the `sol-fn` **structure** facet from the built wasm engine in
   `demo/app/dist`. The harness preserves the AST node-type of each reference exactly as written
   (solmate's bare `totalSupply` state-var read stays an Identifier; OZ's `totalSupply()` stays a
   FunctionCall) so the fingerprints line up with the canonical ones.

4. **Canonical fingerprints re-derived from `tools/gen-vulndata.mjs` to be safe:**
   OZ vulnerable = `f7a9aba964fc`, solmate vulnerable = `e14ab6a80e50`, OZ patched = `435cac3f39fe`.

   Because a `source_hash` IS one exact file content, every compilation of a given variant carries a
   byte-identical body, so classifying one witness per variant classifies ALL its compilations
   exactly (not an extrapolation).

## Coverage / is this a floor?

- OZ: **4,593 / 4,595** compilations classified (100.0%), 57 / 58 variants. The one unclassified
  variant (`0xf783cf49...`, 2 compilations) is an anomalous custom file whose `ERC4626.sol` has the
  conversion function stripped out; negligible.
- solmate: **1,116 / 1,116** compilations classified (100%), all 27 variants.

So within the OZ-path and solmate-path `ERC4626.sol` populations these numbers are the FULL count,
not a sample. They ARE a floor on the true vulnerable-shape population for two reasons:
- The 921 "other" path compilations (135 variants, no library token in the path, e.g. flattened or
  oddly-vendored files) were not classified; some carry the same shapes.
- This only counts files named exactly `ERC4626.sol`. Flattened single-file contracts (where the
  vault body lives inside `MyVault.sol`) and `ERC4626Upgradeable.sol` are not in this population.
The shape-vs-exact GAP itself is also a floor: more variants only widens it.

## Per-shape detail

### OpenZeppelin vulnerable, canonical f7a9 (`_initialConvertToShares` ternary)
- 8 distinct source variants, 228 compilations total.
- largest single variant: 153 (`0x9b9930e3...`) = Sourcify exact-match reach.
- riffcat shape reach: 228. Extra over exact: **+75**.
- variant counts: 153, 39, 13, 11, 6, 2, 2, 2.

### solmate vulnerable, e14a (`supply == 0 ? assets : assets.mulDivDown(...)`)
- 27 distinct source variants, 1,116 compilations total.
- largest single variant: 588 (`0x06df4ff9...`) = Sourcify exact-match reach.
- riffcat shape reach: 1,116. Extra over exact: **+528**.
- the body is byte-identical across many directory layouts (`lib/solmate/src/mixins/`,
  `lib/solmate/src/tokens/`, `solmate/src/mixins/`, `project:/contracts/solmate/mixins/`, ...),
  which is exactly why exact-source-on-one-file under-counts and the shape recovers it.

### OZ patched, 435c (v4.9.0+ `_decimalsOffset`) -- the comparison anchor
- 41 distinct source variants, 4,227 compilations total. Largest single: 986 (`0xbc3eb01a...`).
- Confirms the patched shape dominates the OZ population (4,227 patched vs 364 vulnerable-shaped),
  and that the patched fingerprint cleanly separates from all three vulnerable shapes.

## Honest read

- The OZ gap is REAL but small in absolute terms (+75 over the 153 dominant exact variant): the bulk
  of vulnerable OZ deployments do cluster on a single source_hash, so for OZ the headline is "exact
  match already finds most of them; riffcat adds ~49% more (75 on top of 153) by catching the 7 minor
  re-vendored variants, AND it surfaces two ADDITIONAL distinct vulnerable OZ shapes (108 + 28) that
  exact-source-on-the-dominant-file would never connect to the same bug."
- The solmate gap is LARGE (+528, nearly 2x): here exact-source-on-one-file genuinely misses about
  half the vulnerable population, and the structure fingerprint roughly doubles the reach. solmate is
  the stronger demo beat for the "shape beats exact match" point.
- Combined, exact-match-on-the-two-dominant-files reaches 741 contracts; the two structure
  fingerprints reach 1,344; riffcat adds **603** contracts (about +81%) that an exact-source search
  keyed on the dominant file of each library misses.
