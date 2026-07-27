# Measurements behind the demo

Scrubbed provenance for the shipped numbers, replacing several working notes
that named specific live deployments and colleagues. Keeps the counts, the
method, and the dates; drops addresses, contract names, and any person's name.
The bug in both cases is public and long disclosed; only the identification of
specific deployments is removed.

## ERC-4626 first-deposit inflation shape (`sniff it out`)

Measured 2026-06-23, over Sourcify's `public_compiled_contracts_sources`. Every
`ERC4626.sol` file, grouped by which library's fingerprint it carries.

| family | distinct source variants | Sourcify exact-match reach (largest single variant) | riffcat shape reach (sum of all variants sharing the structure fingerprint) | extra reach |
|---|---|---|---|---|
| OpenZeppelin (canonical vulnerable ternary) | 8 | 153 | 228 | +75 |
| solmate (vulnerable `convertToShares`) | 27 | 588 | 1,116 | +528 |
| combined | 35 | 741 | 1,344 | +603 |

Method: population counted by one BigQuery scan (a `source_hash` is one exact
file, so classifying one witness per variant classifies every compilation of it
exactly, not an extrapolation); one deployed witness address resolved per
variant via a two-step join; each witness's conversion function pulled from the
Sourcify v2 API, compiled through a harness that preserves each reference's AST
node kind, and fingerprinted at the structure facet by the built wasm engine.

Coverage: OZ 4,593 / 4,595 compilations classified (100.0%, one anomalous file
excluded); solmate 1,116 / 1,116 (100%). Both counts are a floor on the true
population: files not named exactly `ERC4626.sol` (flattened contracts,
`ERC4626Upgradeable.sol`) are not in this population, and the shape-minus-exact
gap only widens with more variants. Two further vulnerable-shaped OZ variants
exist outside the canonical fingerprint (108 and 28 compilations), and the
patched shape dominates the OZ population at 4,227 compilations.

Honest read: the OZ gap is real but modest, exact match already finds most of
them. The solmate gap is large, exact-source-on-one-file misses roughly half the
vulnerable population because the identical body is vendored under many
directory layouts.

## Edited Multicall forks (`modified`)

Measured 2026-06-24. The vehicle: OpenZeppelin's `Multicall` combined with
`ERC2771Context`, whose delegatecall loop trusts a forwarder-supplied sender,
inherited into integrator contracts that customize it (overridden modifiers,
hand-rolled forwarder handling), so real edited-but-vulnerable bodies exist on
chain. (An earlier ECDSA-recovery candidate was tried first and rejected: real
deployments were all verbatim, so it never exercised the fuzzy path.)

Kernel: canonical vulnerable `multicall` is 49 nodes; the OZ patch is 76 nodes
(weighted containment against the vulnerable kernel, 0.365). Null calibration
against unrelated loop-shaped functions (a per-item call fanout, an array-fill
loop, a transfer loop, an arithmetic loop, plus decode/copy/verify functions)
puts the coincidence ceiling at approximately 0.33.

40 distinct `Multicall.sol` variants in the vulnerable combination resolved to
35 deployed witnesses. Six scored clearly above the null and are not caught by
exact or Sourcify byte-identical matching: two at Cw 0.819 (49 nodes, the only
edit a dropped `virtual` modifier, which flips the whole-function digest while
leaving the vulnerable loop intact), four at Cw 0.568 (84 nodes, a genuinely
restructured body adding forwarder-detection locals and a branch inside the
loop). Two further real customized bodies land in-band near the null (0.358,
0.251) and are reported as marginal, not certified, since the score alone
cannot distinguish them from a coincidental loop shape at that range. Eight
verbatim matches and a handful trivially modified (one modifier or one library
alias) are caught by exact matching already and are not part of this count.

Verdict: this vehicle was chosen over the ECDSA candidate specifically because
real integrators edit it, which is the property the `modified` chapter needs to
demonstrate, and the two illustrated bodies above are the two clearest cases of
an edit defeating exact match while weighted containment still recognizes the
shape.
