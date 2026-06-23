# riffcat vulnerability-clone demo candidates

Researched 2026-06-23. Defensive security research: identifying PUBLIC, already-disclosed
historical vulnerabilities that propagated across many deployed contracts via copy-paste or
forking, so we can demo riffcat "fingerprint a vulnerable chunk, find every clone that still
has it." No new exploits here; everything is grounded in public disclosures and the live
Sourcify dataset.

## Method and what "verified" means here

- Web evidence: each non-obvious factual claim has a source link inline. Anything I could not
  confirm is marked "unverified."
- Live Sourcify counts: the Sourcify playground Cloud Function endpoint
  (`https://europe-west1-sourcify-project.cloudfunctions.net/bigquery-api-prod/bigquery`)
  **is reachable from this box today** (the 2026-06-12 access notes said proxy 403; that is now
  stale). It executes BigQuery SQL against `sourcify.public_*` with a server-side **6.5 GiB
  per-query byte cap**.
  - Consequence: I **cannot** full-text scan the `content` column of `public_sources` (90 GB,
    estimate ~90 GiB, over cap) or the `compiled_contracts_signatures` join (35 GiB). So I
    could not directly grep all sources for a vulnerable function body.
  - What works cheaply (column pruning keeps scans small): filtering `public_compiled_contracts`
    on `name`, `language`, `compiler`, `version` (a `name=` filter scans ~60 MiB). All
    contract-name and compiler-version counts below are **live, measured** numbers from this
    endpoint on 2026-06-23, not estimates.
  - The full join spine works (`verified_contracts` -> `compiled_contracts` -> per-compilation
    `compiled_contracts_sources` -> `sources`), and pulling sources for ONE compilation is cheap
    because the compilation_id filter prunes the scan. So we **can** feed real verified source to
    riffcat per-contract; what we cannot do is mass-grep all 35M contracts' bodies through this
    capped endpoint. For a corpus build at scale, use the Parquet export or a real BigQuery
    subscription (no cap), per the access notes.
- riffcat capability grounding (from /workspace/riff-catalog/README.md and PLAN.md): per-dimension
  blake3 digests (structure, names, constants, types, trace) at facets (full / names-blind /
  structure-only), at levels `sol-ast/1` (source AST, stable across solc versions),
  `yul-ast/1` and `yul-ssa-cfg/1` (Yul, drifts with compiler version), and `evm/1` (bytecode).
  Units: function, contract, whole object, SSA CFG, raw bytecode. Sourcify ingest door fetches +
  recompiles verified contracts. This is exactly the machinery the demos below assume.

Sourcify scale context (measured today): 35,010,245 verified-contract rows; 5,099,182 distinct
compiled_contracts.

---

## RANKED SHORTLIST

### 1. ERC-4626 vault first-depositor / inflation attack  (TOP PICK) — demo-readiness: HIGH

1. **The vulnerability.** A tokenized-vault share-math rounding flaw. When the vault is empty
   (`totalSupply() == 0`), the first depositor mints shares 1:1, then an attacker "donates" assets
   directly to the vault (bypassing `deposit`), inflating the assets-per-share ratio so the next
   depositor's `convertToShares` rounds down toward zero and their deposit is effectively captured.
   It lives in a localized, well-known chunk: `_convertToShares` / `_convertToAssets` (and the
   `previewDeposit`/`previewMint` that call them). Disclosure / mitigation discussion:
   OpenZeppelin issue [#3706](https://github.com/OpenZeppelin/openzeppelin-contracts/issues/3706),
   mitigation PR [#3979](https://github.com/OpenZeppelin/openzeppelin-contracts/pull/3979) (merged
   2023-02-17), writeup ["A Novel Defense Against ERC4626 Inflation
   Attacks"](https://www.openzeppelin.com/news/a-novel-defense-against-erc4626-inflation-attacks).
   It is the EIP-4626 reference standard's most-discussed footgun.

2. **The origin.** The canonical shape is the OpenZeppelin `ERC4626.sol` extension. The
   thousands of vaults in the wild inherit from or copy this file, so the vulnerable chunk has a
   single canonical ancestor.

3. **Spread evidence (measured live on Sourcify today).** 58,370 verified Solidity contracts have
   "Vault" in their name. Contracts whose name contains "4626": e.g. `ERC4626Vault` (27),
   `ERC4626` (19), `Vault4626` (16), `AaveV3ERC4626` (14), `ConvexERC4626` (13), plus dozens more
   ERC4626Adapter/Strategy/Router/Factory variants. The "Vault" population is the relevant clone
   universe; not all are 4626 and not all are vulnerable, which is exactly what the fingerprint is
   for (separating them).

4. **Source availability.** Excellent. These are mostly recent solc 0.8.x verified contracts with
   full source on Sourcify, ingestible through riffcat's existing sourcify door. We can pull the
   exact `_convertToShares` body per contract.

5. **Patched-vs-unpatched (the killer property, CONFIRMED).** OpenZeppelin Contracts **v4.9.0**
   (released 2023-05-23,
   [release](https://github.com/OpenZeppelin/openzeppelin-contracts/releases/tag/v4.9.0)) shipped
   the decimal-offset mitigation by default. The two shapes differ structurally in the share-math
   chunk (fetched verbatim from the OZ repo):
   - **Vulnerable (v4.8 and earlier, and every hand-rolled pre-mitigation vault):**
     `_convertToShares` = `(assets == 0 || supply == 0) ? _initialConvertToShares(...) :
     assets.mulDiv(supply, totalAssets(), rounding)` — no virtual term, 1:1 first-deposit branch.
   - **Patched (v4.9+):** `_convertToShares` = `assets.mulDiv(totalSupply() + 10 **
     _decimalsOffset(), totalAssets() + 1, rounding)` — adds the `+ 10**_decimalsOffset()` virtual
     shares and `+ 1` virtual assets, and a new `_decimalsOffset()` function.
   These are different ASTs: the patched version adds operands (`+`), a function call
   (`_decimalsOffset()`), and removes the `supply == 0` branch. So a `sol-fn` structure-facet
   fingerprint of `_convertToShares` cleanly separates patched from unpatched, even names-blind.

6. **Reproducibility plan.** Level `sol-fn` (source AST), facet **structure** or **names-blind**
   (so vault-specific identifiers do not matter). (a) Fingerprint the v4.8 `_convertToShares`
   chunk. (b) Ingest the verified "Vault"/"4626" corpus from Sourcify; bucket `sol-fn` at facet
   structure. (c) The class containing the v4.8 fingerprint = "still carries the pre-mitigation
   share-math"; the v4.9 fingerprint forms a distinct class = "patched." Punchline figure: "Of N
   verified ERC-4626 vaults, X still carry the pre-2023 inflation-prone share-math chunk; the
   decimal-offset patch produces a structurally distinct fingerprint that cleanly separates the Y
   that fixed it." Because `sol-fn` is solc-version-stable, this holds across the many solc 0.8.x
   versions these vaults were built with.

7. **Why HIGH.** Localized chunk, single canonical ancestor, a real dated patch with a clean
   structural delta, huge source-available clone population, and the patched/unpatched split is
   exactly riffcat's strongest story. Best fit to the brief's "killer example."

---

### 2. Vyper `@nonreentrant` lock miscompilation (Curve, 2023) — demo-readiness: HIGH (unique angle)

1. **The vulnerability.** A *compiler* bug, not a source bug. Vyper's storage-slot allocator
   stopped deduplicating `@nonreentrant(<key>)` locks by key and assigned every decorator its own
   slot, so distinct functions sharing a reentrancy key no longer shared a mutex: single-function
   reentrancy was still blocked but **cross-function** reentrancy was open. Affected Vyper
   versions: **0.2.15, 0.2.16, 0.3.0**; fixed (incidentally, via optimization PRs #2439/#2514) in
   **0.3.1**. Exploited 2023-07-30 against multiple Curve pools (pETH/ETH, msETH/ETH, alETH/ETH,
   CRV/ETH), ~$69M. Postmortem:
   [Vyper Nonreentrancy Lock Vulnerability Technical Post-Mortem](https://hackmd.io/@vyperlang/HJUgNMhs2)
   (currently 403 to this box, but corroborated by
   [LlamaRisk postmortem](https://hackmd.io/@LlamaRisk/BJzSKHNjn),
   [OtterSec timeline](https://osec.io/blog/2023-08-01-vyper-timeline/),
   [vyper PR #2439](https://github.com/vyperlang/vyper/pull/2439)).

2. **The origin.** Curve's own pool templates (StableSwap, crypto pools, gauges) plus the many
   pools deployed from Curve factories, all compiled with the affected Vyper releases.

3. **Spread evidence (measured live on Sourcify today).** 174 verified Vyper contracts on the
   three affected versions: **0.2.16 -> 72, 0.2.15 -> 52, 0.3.0 -> 50**. Crucially, that
   affected-version set directly contains the real Curve family by name: `StableSwap`,
   `VotingEscrow`, `GaugeController`, `LiquidityGaugev4`, `CurveSidechainStableSwapProxy`,
   `Voting_Escrow_Delegation_Proxy`, plus many `Vyper_contract` (Etherscan default name) on those
   exact versions. The fix landed in 0.3.1 (147 verified) and later.

4. **Source availability.** Good: these are Vyper-verified on Sourcify with source. Note the
   access-doc caveat: Vyper version strings are inconsistent across 0.3.x.

5. **Patched-vs-unpatched.** Yes, but with a twist that makes this the most riffcat-distinctive
   case: **the Vyper source is often IDENTICAL between vulnerable and safe deployments; only the
   compiler version differs.** So the split is invisible at the source level and visible only at
   the compiled level. This is the case the brief flagged: riffcat's `yul`/`evm` levels can in
   principle distinguish vulnerable compiler output (different storage-slot assignment for the
   locks) even when the `sol`/source fingerprint is identical.

6. **Reproducibility plan.** Two-level demo. (a) At source level (Vyper has no `sol-fn` path in
   riffcat today; this is a gap — see caveat), the contracts fingerprint identically: "same
   contract." (b) At `evm/1` bytecode level (Sourcify gives us onchain + recompiled bytecode), the
   `@nonreentrant` slot-allocation difference shows up as a structural divergence between
   0.2.15/0.2.16/0.3.0 output and 0.3.1+ output of the *same* source. Punchline: "identical source,
   but the bytecode fingerprint at facet structure separates the 174 contracts compiled by a
   vulnerable Vyper from the safe ones — version-pinned vulnerability that source-level review
   cannot see." Compare onchain bytecode of an affected-version Curve pool vs the same logic
   recompiled with 0.3.1.

7. **Why HIGH (with one caveat).** It is the only candidate that *requires* the compiled-level
   fingerprint to tell the story, which uniquely showcases riffcat's multi-level facets and lands
   the "compiler-version bug" point hard. Caveat: riffcat's front doors today are solc-centric
   (README lists solc AST/Yul/EVM; Vyper is not a listed ingest door). We would demo this at the
   `evm/1` bytecode level (which is language-agnostic) using Sourcify's bytecode, or invest in a
   Vyper source door. Confirm the bytecode-level slot-difference is visible at facet structure
   before committing this as a live beat.

---

### 3. Uniswap V2 / Sushi / Pancake / Compound fork families — demo-readiness: HIGH (flavor b)

1. **The "vulnerability" angle is weaker (this is mainly flavor (b), large shared-chunk
   families), but several fork-specific exploits exist** (e.g. K-value / fee-on-transfer
   mishandling in V2 forks, MasterChef double-reward and migrator bugs). The headline value is the
   simpler "find all the clones" utility.

2. **The origin.** Uniswap V2 core/periphery (`UniswapV2Pair`, `UniswapV2Factory`,
   `UniswapV2Router02`), SushiSwap's `MasterChef`, Compound's `Comptroller`/`CErc20Delegate`.

3. **Spread evidence (measured live on Sourcify today).** Verbatim forks are enormous:
   `UniswapV2Router02` 2,255, `UniswapV2Factory` 1,873, `UniswapV2Pair` 783; `MasterChef`
   **11,041**; `PancakeRouter` 2,395 / `PancakeFactory` 1,305 / `PancakePair` 619; Compound
   `Comptroller` 920, `CErc20Delegate` 295, `CToken` 184, `CEther` 177, `CErc20` 83. These are
   names alone; the actual verbatim-chunk overlap is what riffcat would quantify.

4. **Source availability.** Excellent (these are heavily verified DeFi contracts).

5. **Patched-vs-unpatched.** Diffuse rather than a single clean dated patch, so this is not the
   killer "split" story; different forks diverge in many directions. Specific fork exploits do have
   patches, but tying one to a clean clone-split would need more digging (unverified which single
   fork-bug gives the cleanest split).

6. **Reproducibility plan.** `sol-fn` / `sol-contract` at facet names-blind across the
   `UniswapV2Pair` or `MasterChef` population: bucket and show the giant dedup class ("N forks,
   one structural body"), then `overlap` two named forks to show Jaccard near 1.0 on the shared
   core with a handful of novel functions highlighted. This is the live, scaled-up version of the
   README's ERC20<->AMM twins beat and the PLAN's "94% standard helpers, review these 3 novel
   functions" triage pitch.

7. **Why HIGH for flavor (b), MEDIUM as a vuln story.** Unbeatable raw clone counts and a clean
   "find the clones" punchline; weaker as a patched-vs-unpatched vuln demo. Best used as the
   warm-up that motivates candidate #1.

---

### 4. batchOverflow / proxyOverflow ERC20 integer-overflow family (2018) — demo-readiness: MEDIUM

1. **The vulnerability.** Classic unchecked integer overflow in copy-pasted ERC20 helper
   functions. batchOverflow: `batchTransfer(address[] _receivers, uint256 _value)` computes
   `uint256 amount = uint256(cnt) * _value` with no SafeMath, so a crafted `_value` overflows
   `amount` to a tiny number that passes the balance check while transferring astronomical amounts
   (BEC token, 2018). proxyOverflow: `transferProxy` where `_fee + _value` overflows to 0 and
   passes the sanity check. CVEs:
   [CVE-2018-10299](https://nvd.nist.gov/vuln/detail/CVE-2018-10299) (batchOverflow),
   [CVE-2018-10376](https://www.cvedetails.com/cve/CVE-2018-10376/) (proxyOverflow). PeckShield
   advisories (batchOverflow 2018-04-22, proxyOverflow 2018-04-25) — note their blog 403s to this
   box; corroborated by NVD/CVE and
   [David Gerard's roundup](https://davidgerard.co.uk/blockchain/2018/04/26/smart-contracts-stupid-humans-new-major-erc-20-token-bugs-batchoverflow-and-proxyoverflow/).

2. **The origin.** A single copy-pasted ERC20 token template circulated among 2018 token
   launches; the same `batchTransfer` / `transferProxy` bodies appear verbatim across tokens.

3. **Spread evidence.** PeckShield reported the same overflow family across "around 12" additional
   token contracts beyond BEC (batch + proxy + burnOverflow + multiOverflow + transferFlow variants;
   exact per-CVE counts vary by report — treat "dozens across the family" as the safe claim, the
   precise per-token list is **unverified** from the primary advisories since they 403'd). The
   classic vulnerable batchTransfer/transferProxy bodies are byte-identical across the affected
   tokens (this is well documented as a copy-paste family).

4. **Source availability (the catch).** These are 2018-era solc 0.4.x contracts. Sourcify has
   147,138 verified solc 0.4.x contracts total, but whether the *specific* affected tokens (BEC,
   SMT, MESH, UGT, etc.) are verified on Sourcify is **unverified** (name probes found `SMT` 93,
   `SmartMesh` 1, `UGToken` 3, but these names are ambiguous and likely include unrelated modern
   tokens). Many 2018 tokens are only verified on Etherscan, not Sourcify. We may need to source the
   original bodies from Etherscan / SWC test cases and fingerprint clones found by bytecode.

5. **Patched-vs-unpatched.** Weak. The "fix" was the ecosystem-wide move to SafeMath and then
   solc 0.8 checked arithmetic; there is not a single dated patch that some clones of *this*
   template adopted and others did not. The split is more "vulnerable 2018 token vs unrelated
   modern token," which is less compelling than #1.

6. **Reproducibility plan.** `sol-fn` at facet names-blind on the `batchTransfer` body: fingerprint
   the BEC version, find verbatim twins. If source coverage on Sourcify is thin, fall back to
   `evm/1` bytecode fingerprinting of the overflow chunk. Punchline would be "the same overflowing
   batchTransfer body appears in N tokens" — strong IF we can assemble the source set.

7. **Why MEDIUM.** Iconic and genuinely a copy-paste family (great narrative), but the source
   availability on Sourcify is uncertain and there is no clean patched/unpatched split. Good as a
   historical "this is why fingerprinting matters" anecdote; risky as the live data beat.

---

### 5. Parity multisig: unprotected initializer + delegatecall-to-selfdestruct (2017) — MEDIUM-LOW

1. **The vulnerability.** Two linked bugs in the Parity `WalletLibrary`. (i) `initWallet`/`initMultiowned`
   had no guard, callable post-deploy by anyone (July 2017, ~153,037 ETH stolen). (ii) The library
   itself was never initialized; an attacker called `initWallet` to become owner then `kill`
   (`selfdestruct`), bricking every wallet that delegatecalled into it (Nov 2017, **587 wallets**,
   513,774.16 ETH frozen). SWC mapping: SWC-105 (unprotected ether withdrawal) and SWC-106
   (unprotected SELFDESTRUCT). Postmortems:
   [Parity](https://medium.com/paritytech/a-postmortem-on-the-parity-multi-sig-library-self-destruct-63daca3a4cf7),
   [OpenZeppelin](https://www.openzeppelin.com/news/on-the-parity-wallet-multisig-hack-405a8c12e8f7).

2. **The origin.** Parity `Wallet.sol` / `WalletLibrary` (canonical, single source).

3. **Spread evidence.** Concrete and clean: 587 deployed wallet proxies all delegatecalled into the
   one library (the proxies are near-identical stub clones). This is a real, numbered clone family.

4. **Source availability (the catch).** 2017-era solc 0.4.x. Whether the 587 wallet proxies are
   verified on Sourcify is **unverified** and likely sparse (mostly tiny delegatecall stubs that
   carry little structure). The WalletLibrary source is public.

5. **Patched-vs-unpatched.** Weak as a structural split: the "fix" was social (stop using the
   library / redeploy), not a structural patch that some clones carry. The interesting structural
   feature (missing initializer guard) is an absence, which fingerprints less crisply than a present
   chunk.

6. **Reproducibility plan.** `sol-fn` on `initWallet` showing the missing guard, or `sol-contract`
   on the proxy stub to find all 587 clones by structure. Bytecode-level if source is unverified.
   Punchline: "fingerprint the unguarded initializer; here are the clones that share it."

7. **Why MEDIUM-LOW.** Famous and cleanly numbered, but the vulnerability is an *absence* of a
   check (harder to fingerprint than a present chunk), the clones are thin stubs, and Sourcify
   source coverage of the 2017 proxies is doubtful. Better as a slide anecdote than a live beat.

---

### 6. ERC-777 reentrancy hooks (imBTC / Uniswap V1 / Lendf.me, 2020) — demo-readiness: LOW (for THIS tool)

1. **The vulnerability.** ERC-777's `tokensToSend`/`tokensReceived` hooks hand control to the
   token sender/recipient mid-transfer; integrators that did `transfer`-then-update (violating
   checks-effects-interactions) were reenterable. Exploited 2020-04-18/19 against Uniswap V1's
   imBTC pool (~1,278 ETH) and Lendf.me (~$25M). SWC-107 (reentrancy). Sources:
   [PeckShield root-cause](https://peckshield.medium.com/uniswap-lendf-me-hacks-root-cause-and-loss-analysis-50f3263dcc09),
   [imToken/Tokenlon](https://medium.com/imtoken/about-recent-uniswap-and-lendf-me-reentrancy-attacks-7cebe834cb3).

2. **The origin.** The vulnerability is an *interaction* between the ERC-777 standard and the
   integrator's ordering, not a single copied chunk. Uniswap V1 (Vyper) and Lendf.me had different
   codebases.

3. **Spread evidence.** The class of "reentrant withdraw/supply that calls an external token before
   updating state" is widespread, but it is a *pattern*, not a verbatim clone of one ancestor.

4. **Source availability.** Uniswap V1 is Vyper (see candidate #2 caveat about Vyper doors);
   Lendf.me is Solidity. Mixed.

5. **Patched-vs-unpatched.** No single canonical patch to one shared chunk.

6. **Reproducibility plan.** Would require a *pattern* query (CEI-violation shape), which riffcat's
   exact-structural-fingerprint model does not natively express — riffcat finds *twins of a
   specific chunk*, not "any function that calls external before SSTORE." A claims-layer assertion
   could tag known-reentrant functions, but that is curation, not fingerprint discovery.

7. **Why LOW.** Real and important, but it is a semantic anti-pattern across heterogeneous code, not
   a copy-pasted chunk with one ancestor and one patch. Poor fit for riffcat's exact-twin model.
   Mention as "the kind of thing the claims layer, not the hash, is for."

---

## Summary table

| # | Candidate | Vuln chunk localized? | Single ancestor? | Clean dated patch / split? | Source on Sourcify? | Best riffcat level | Rating |
|---|---|---|---|---|---|---|---|
| 1 | ERC-4626 inflation | yes (`_convertToShares`) | yes (OZ ERC4626) | YES (v4.9, 2023-05-23, structural) | yes, large + recent | `sol-fn` structure | **HIGH** |
| 2 | Vyper nonreentrant (Curve) | yes (lock alloc, compiled) | yes (Vyper compiler) | yes, version-pinned (0.3.1) | yes (174 on affected vers) | `evm/1` bytecode | HIGH (unique) |
| 3 | UniV2/Sushi/Pancake/Compound forks | n/a (shared core) | yes per family | diffuse | yes, huge | `sol-fn`/`sol-contract` names-blind | HIGH (flavor b) |
| 4 | batchOverflow/proxyOverflow ERC20 | yes (`batchTransfer`) | yes (token template) | weak (SafeMath/0.8 era) | uncertain (2018-era) | `sol-fn`/`evm` names-blind | MEDIUM |
| 5 | Parity multisig | partial (absent guard) | yes (WalletLibrary) | weak (social fix) | doubtful (2017 stubs) | `sol-contract`/`evm` | MEDIUM-LOW |
| 6 | ERC-777 reentrancy | no (anti-pattern) | no | no | mixed (Vyper+Sol) | n/a (claims layer) | LOW |

## Top recommendation

**Build the demo around candidate #1 (ERC-4626 inflation), with candidate #3 (fork families) as
the warm-up and candidate #2 (Vyper) as the "and here's the level only we can see" finale.**

Rationale: #1 is the only candidate that hits every property the brief asked for at once: a
localized vulnerable chunk (`_convertToShares`), a single canonical ancestor (OZ ERC4626), a real
dated patch (v4.9, 2023-05-23) whose mitigation is a *clean structural delta* (the `+
10**_decimalsOffset()` / `+ 1` virtual terms), a large and recent source-available clone population
on Sourcify (58,370 "Vault" contracts, all on modern solc 0.8.x so `sol-fn` is stable), and a
punchline that is precisely riffcat's killer move: fingerprint the pre-mitigation share-math, and
the fingerprint itself sorts the vaults into "patched" vs "still inflation-prone." #2 is the most
*distinctive* (it forces the compiled-level facet and showcases multi-level fingerprinting), so it
makes the strongest closer, but it carries a real tooling caveat (riffcat's ingest doors are
solc-centric today; the Vyper story would run at the language-agnostic `evm/1` bytecode level or
need a new Vyper door, and the bytecode-level slot-difference should be confirmed visible at facet
structure before it goes live).

## Open caveats to resolve before building

- For #1: confirm how many of the 58,370 "Vault" contracts are genuinely ERC-4626 and carry the OZ
  share-math (the name filter overcounts). The precise patched/unpatched split count is the demo
  figure and needs the real corpus pull (Parquet export / full BigQuery, not the capped endpoint)
  to compute at scale.
- For #2: verify riffcat can ingest Vyper-origin bytecode at `evm/1` and that the `@nonreentrant`
  slot-allocation difference is visible at facet structure between affected vs 0.3.1 output. Vyper
  is not a listed source ingest door.
- For #4/#5: confirm Sourcify source coverage of the specific 2018/2017 contracts before relying on
  them; both may need Etherscan or SWC test-case source instead.
- General: the live Cloud Function endpoint has a 6.5 GiB cap that blocks full `content` scans; do
  not promise source-grep-at-scale through it. Use it for name/version counts and per-contract
  source pulls; use Parquet/BigQuery for corpus-scale work.
