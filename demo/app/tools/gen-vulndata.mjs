// Generate vulndata.js for the "sniff out the bug" chapter. The ERC-4626 first
// deposit / inflation share-math bug lives in _convertToShares. We fingerprint
// four real conversion shapes at sol-fn and show the structure facet separates
// vulnerable from patched: the OZ pre-4.9 ternary and solmate's supply==0?assets
// are each a vulnerable shape; the OZ v4.9 decimals-offset one-liner is a
// distinct (patched) shape; and solady stays mitigated while KEEPING the
// _initialConvertToShares name a keyword search trips on, which the shape sees
// through. Bodies are verbatim (OZ-vuln from a deployed witness per the
// vuln-sniff report; the rest from the vendored stdlib). Deployed witnesses are
// from demo/vuln-sniff-candidates-2026-06-23.md.
//
// Reproducible: needs solc + the built wasm engine in ../dist. Run:
//   bun tools/gen-vulndata.mjs
import { readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
const HERE = import.meta.dir, DIST = join(HERE, "../dist");
const glue = readdirSync(DIST).find((f) => /^riffcat-demo-app-.*\.js$/.test(f) && !f.includes("_bg"));
const wasm = readdirSync(DIST).find((f) => f.endsWith("_bg.wasm"));
const b = await import(join(DIST, glue));
await b.default({ module_or_path: "file://" + join(DIST, wasm) });
const solc = "/run/current-system/sw/bin/solc";

// Harness: the four conversion bodies, structure preserved (only identifiers
// renamed and dependencies stubbed). The displayed source below is the real
// verbatim body; the fingerprint comes from the structurally-identical harness.
const HARNESS = `// SPDX-License-Identifier: MIT
pragma solidity >=0.8.0;
enum Rounding { Down, Up }
library Math { function mulDiv(uint256 x, uint256 y, uint256 d, Rounding) internal pure returns (uint256) { return x * y / d; } }
library FPM { function mulDivDown(uint256 x, uint256 y, uint256 d) internal pure returns (uint256) { return x * y / d; } }
library FixedPointMathLib { function fullMulDiv(uint256 x, uint256 y, uint256 d) internal pure returns (uint256) { return x * y / d; } }
contract VaultShapes {
    using Math for uint256;
    using FPM for uint256;
    uint256 public totalSupplyVar;
    function totalSupply() public view returns (uint256) { return totalSupplyVar; }
    function totalAssets() public view returns (uint256) { return 0; }
    function _decimalsOffset() internal view virtual returns (uint8) { return 0; }
    function _useVirtualShares() internal view virtual returns (bool) { return true; }
    function _eitherIsZero(uint256 a, uint256 c) internal pure returns (bool) { return a == 0 || c == 0; }
    function _inc(uint256 x) internal pure returns (uint256) { return x + 1; }
    function _initialConvertToShares(uint256 assets, Rounding) internal view virtual returns (uint256) { return assets; }
    function _initialConvertToShares(uint256 assets) internal view virtual returns (uint256 shares) { shares = assets; }

    function ozVuln(uint256 assets, Rounding rounding) internal view virtual returns (uint256 shares) {
        uint256 supply = totalSupply();
        return
            (assets == 0 || supply == 0)
                ? _initialConvertToShares(assets, rounding)
                : assets.mulDiv(supply, totalAssets(), rounding);
    }
    function ozPatched(uint256 assets, Rounding rounding) internal view virtual returns (uint256) {
        return assets.mulDiv(totalSupply() + 10 ** _decimalsOffset(), totalAssets() + 1, rounding);
    }
    function solmateVuln(uint256 assets) public view virtual returns (uint256) {
        uint256 supply = totalSupplyVar;
        return supply == 0 ? assets : assets.mulDivDown(supply, totalAssets());
    }
    function soladyMitig(uint256 assets) public view virtual returns (uint256 shares) {
        if (!_useVirtualShares()) {
            uint256 supply = totalSupply();
            return _eitherIsZero(assets, supply)
                ? _initialConvertToShares(assets)
                : FixedPointMathLib.fullMulDiv(assets, supply, totalAssets());
        }
        uint256 o = _decimalsOffset();
        if (o == uint256(0)) {
            return FixedPointMathLib.fullMulDiv(assets, totalSupply() + 1, _inc(totalAssets()));
        }
        return FixedPointMathLib.fullMulDiv(assets, totalSupply() + 10 ** o, _inc(totalAssets()));
    }
}
`;

const pr = Bun.spawnSync([solc, "--standard-json"], { stdin: Buffer.from(JSON.stringify(
  { language: "Solidity", sources: { "V.sol": { content: HARNESS } }, settings: { outputSelection: { "*": { "": ["ast"] } } } })) });
const comp = JSON.parse(pr.stdout.toString());
if (!comp.sources || !comp.sources["V.sol"]) { console.error(JSON.stringify(comp.errors, null, 2)); process.exit(1); }
const units = JSON.parse(b.fingerprint_source(JSON.stringify(comp.sources["V.sol"].ast), "shape")).filter((u) => u.unit === "sol-fn");
const byName = Object.fromEntries(units.map((u) => [u.name.split(".").pop(), u]));
const fp = (nm) => ({ structure: byName[nm].facets.structure, namesBlind: byName[nm].facets["names-blind"] });

// real verbatim bodies for display
const SRC = {
  ozVuln: `function _convertToShares(uint256 assets, Math.Rounding rounding) internal view virtual returns (uint256 shares) {
    uint256 supply = totalSupply();
    return
        (assets == 0 || supply == 0)
            ? _initialConvertToShares(assets, rounding)
            : assets.mulDiv(supply, totalAssets(), rounding);
}`,
  ozPatched: `function _convertToShares(uint256 assets, Math.Rounding rounding) internal view virtual returns (uint256) {
    return assets.mulDiv(totalSupply() + 10 ** _decimalsOffset(), totalAssets() + 1, rounding);
}`,
  solmateVuln: `function convertToShares(uint256 assets) public view virtual returns (uint256) {
    uint256 supply = totalSupply; // Saves an extra SLOAD if totalSupply is non-zero.
    return supply == 0 ? assets : assets.mulDivDown(supply, totalAssets());
}`,
  soladyMitig: `function convertToShares(uint256 assets) public view virtual returns (uint256 shares) {
    if (!_useVirtualShares()) {
        uint256 supply = totalSupply();
        return _eitherIsZero(assets, supply)
            ? _initialConvertToShares(assets)
            : FixedPointMathLib.fullMulDiv(assets, supply, totalAssets());
    }
    uint256 o = _decimalsOffset();
    if (o == uint256(0)) {
        return FixedPointMathLib.fullMulDiv(assets, totalSupply() + 1, _inc(totalAssets()));
    }
    return FixedPointMathLib.fullMulDiv(assets, totalSupply() + 10 ** o, _inc(totalAssets()));
}`,
};

// Counts from demo/vuln-reach-2026-06-23.md, which classified every DISTINCT
// source_hash variant of ERC4626.sol in Sourcify's verified population (one file
// per variant; a source_hash is one exact file, so every deployment of it shares
// the shape, and the per-contract totals are arithmetic, not extrapolation).
//   exact = the single largest source_hash variant (what a Sourcify exact-source
//           match keyed on one file finds).
//   reach = sum across every variant whose conversion function has this shape
//           (what matching the SHAPE finds). exact <= reach.
// Exact within scope (verified, file named ERC4626.sol) and a floor.
const shapes = [
  { key: "oz-vuln", lib: "openzeppelin", label: "OpenZeppelin _convertToShares", sub: "pre v4.9 (<= v4.8.x)", status: "vulnerable", keyword: true, exact: 153, reach: 228, src: SRC.ozVuln, ...fp("ozVuln") },
  { key: "solmate-vuln", lib: "solmate", label: "Solmate convertToShares", sub: "no virtual shares", status: "vulnerable", keyword: true, exact: 588, reach: 1116, src: SRC.solmateVuln, ...fp("solmateVuln") },
  { key: "oz-patched", lib: "openzeppelin", label: "OpenZeppelin _convertToShares", sub: "v4.9.0+ (decimals offset)", status: "patched", keyword: false, exact: 986, reach: 4227, src: SRC.ozPatched, ...fp("ozPatched") },
  { key: "solady-mitig", lib: "solady", label: "Solady convertToShares", sub: "virtual shares on by default", status: "mitigated", keyword: true, exact: null, reach: null, src: SRC.soladyMitig, ...fp("soladyMitig") },
];

const CHAIN = { 1: "Ethereum", 137: "Polygon", 8453: "Base", 56: "BSC", 10: "Optimism", 42161: "Arbitrum" };
const W = (chain, address, name, shape) => ({ chain, chainName: CHAIN[chain], address, name, shape, url: "https://sourcify.dev/#/lookup/" + address });
const witnesses = [
  W(137, "0xREDACTED_ADDRESS_REMOVED_FROM_HISTORY", "RiveraConcNoStaking", "oz-vuln"),
  W(8453, "0xREDACTED_ADDRESS_REMOVED_FROM_HISTORY", "RiveraAutoCompoundingVaultV2Public", "oz-vuln"),
  W(137, "0xREDACTED_ADDRESS_REMOVED_FROM_HISTORY", "MBVault", "oz-vuln"),
  W(137, "0xREDACTED_ADDRESS_REMOVED_FROM_HISTORY", "FCNProduct", "oz-vuln"),
  W(56, "0xREDACTED_ADDRESS_REMOVED_FROM_HISTORY", "GatewayToken", "oz-vuln"),
  W(8453, "0xREDACTED_ADDRESS_REMOVED_FROM_HISTORY", "ERC4626EthRouter", "solmate-vuln"),
  W(56, "0xREDACTED_ADDRESS_REMOVED_FROM_HISTORY", "StratX4Venus", "solmate-vuln"),
  W(10, "0xREDACTED_ADDRESS_REMOVED_FROM_HISTORY", "AaveV3ERC4626Factory", "solmate-vuln"),
  W(1, "0xREDACTED_ADDRESS_REMOVED_FROM_HISTORY", "Market", "solmate-vuln"),
  W(137, "0xREDACTED_ADDRESS_REMOVED_FROM_HISTORY", "StrategVault", "oz-patched"),
  W(8453, "0xREDACTED_ADDRESS_REMOVED_FROM_HISTORY", "FluidAprOracleBase", "oz-patched"),
  W(8453, "0xREDACTED_ADDRESS_REMOVED_FROM_HISTORY", "CreditStrategy", "oz-patched"),
  W(8453, "0xREDACTED_ADDRESS_REMOVED_FROM_HISTORY", "ERC7540Engine", "solady-mitig"),
  W(137, "0xREDACTED_ADDRESS_REMOVED_FROM_HISTORY", "MaxApyVaultFactory", "solady-mitig"),
];

// sanity: vulnerable and patched must be different shapes at structure
console.log("=== structure fingerprints (first 12 hex) ===");
for (const s of shapes) console.log(`  ${s.key.padEnd(13)} ${s.status.padEnd(10)} ${s.structure.slice(0, 12)}  (keyword-hit: ${s.keyword})`);
const ozV = shapes.find((s) => s.key === "oz-vuln").structure, ozP = shapes.find((s) => s.key === "oz-patched").structure;
console.log("\noz vulnerable vs patched separate at structure:", ozV !== ozP ? "YES" : "NO (PROBLEM)");
const sol = shapes.find((s) => s.key === "solady-mitig").structure;
console.log("solady mitigated shape != oz-vulnerable shape:", sol !== ozV ? "YES" : "NO (PROBLEM)");

const out = {
  generated: "2026-06-23",
  bug: "ERC-4626's first-deposit inflation bug: when a vault is empty, the old _convertToShares mints shares 1:1, so an attacker front-runs with a tiny deposit, donates assets to skew the price, and the next depositor's shares round to zero.",
  corpus: 5711, // verified ERC4626.sol vaults classified (OZ + solmate)
  // the two vulnerable shapes, exact-match reach vs shape reach (see report)
  reach: { exactVuln: 741, shapeVuln: 1344, extra: 603, variants: 35 },
  shapes, witnesses,
};
writeFileSync(join(HERE, "../vulndata.js"), "window.RIFFCAT_VULN = " + JSON.stringify(out) + ";\n");
console.log("\nwrote vulndata.js:", shapes.length, "shapes,", witnesses.length, "witnesses");
