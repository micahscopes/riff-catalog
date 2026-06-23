// Generate realdata.js for the storybook's "recognized" chapter: pull real
// verified contracts from Sourcify that we KNOW used OpenZeppelin (their source
// carries @openzeppelin import paths), fingerprint every function at sol-fn, and
// look each up in the committed std-lib catalog. Output is precomputed, so the
// view needs no engine at runtime.
//
// Reproducible. Deps: bun, the locally built wasm engine in ../dist, solc on
// PATH, demo/stdlib/catalog.json, and network access to Sourcify (SQL endpoint
// for the contract list, v2 API for sources). Run: bun tools/gen-realdata.mjs
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const HERE = import.meta.dir;
const DIST = join(HERE, "../dist");
const glue = readdirSync(DIST).find((f) => /^riffcat-demo-app-.*\.js$/.test(f) && !f.includes("_bg"));
const wasm = readdirSync(DIST).find((f) => f.endsWith("_bg.wasm"));
const b = await import(join(DIST, glue));
await b.default({ module_or_path: "file://" + join(DIST, wasm) });

const solc = "/run/current-system/sw/bin/solc";
const CAT = JSON.parse(readFileSync(join(HERE, "../../stdlib/catalog.json"), "utf8")).catalog;
const SQL = (sql) => fetch("https://europe-west1-sourcify-project.cloudfunctions.net/bigquery-api-prod/bigquery",
  { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ sql }) }).then((r) => r.json());
const norm = (s) => s.replace(/pragma solidity[^;]*;/g, "pragma solidity >=0.8.0;");

const NAMES = ["TimelockController", "Governor", "StakingRewards", "TokenVesting", "VestingWallet",
  "Crowdsale", "Treasury", "Escrow", "Marketplace", "Airdrop", "MerkleDistributor", "PaymentSplitter",
  "Faucet", "Staking", "RewardsDistributor"];
const q = await SQL(`SELECT cc.name, cc.version, cd.chain_id AS chain, CONCAT('0x', TO_HEX(cd.address)) AS address
  FROM sourcify.public_verified_contracts vc
  JOIN sourcify.public_compiled_contracts cc ON vc.compilation_id = cc.id
  JOIN sourcify.public_contract_deployments cd ON vc.deployment_id = cd.id
  WHERE cc.version LIKE '0.8.2%' AND cd.chain_id = 1 AND cc.name IN (${NAMES.map((n) => `'${n}'`).join(",")})
  ORDER BY vc.created_at DESC LIMIT 200`);

function fingerprintFns(srcs) {
  const sources = {}, bufs = {};
  for (const [p, o] of Object.entries(srcs)) { const n = norm(o.content); sources[p] = { content: n }; bufs[p] = Buffer.from(n, "utf8"); }
  const pr = Bun.spawnSync([solc, "--standard-json"], { stdin: Buffer.from(JSON.stringify({ language: "Solidity", sources, settings: { outputSelection: { "*": { "": ["ast"] } } } })) });
  const comp = JSON.parse(pr.stdout.toString());
  if (!comp.sources) return null;
  const fns = [];
  for (const [p, o] of Object.entries(comp.sources)) {
    if (!o.ast) continue;
    let units;
    try { units = JSON.parse(b.fingerprint_source(JSON.stringify(o.ast), "shape")); } catch { continue; }
    const byName = Object.fromEntries(units.filter((u) => u.unit === "sol-fn").map((u) => [u.name, u]));
    const walk = (n, ctr) => {
      if (!n || typeof n !== "object") return;
      if (n.nodeType === "ContractDefinition") { for (const ch of (n.nodes || [])) walk(ch, n.name); return; }
      if (n.nodeType === "FunctionDefinition" && ctr) {
        const nm = n.name || n.kind, u = byName[ctr + "." + nm];
        if (u) {
          const [o2, l] = n.src.split(":").map(Number);
          const src = bufs[p].subarray(o2, o2 + l).toString();
          const cat = CAT[u.facets["names-blind"]];
          fns.push({ ctr, fn: nm, loc: src.split("\n").length, nb: u.facets["names-blind"].slice(0, 12),
            lib: cat ? cat.lib : null, canon: cat ? cat.name : null, src: src.split("\n").slice(0, 40).join("\n") });
        }
      }
      for (const k in n) { const v = n[k]; if (Array.isArray(v)) v.forEach((x) => walk(x, ctr)); else if (v && typeof v === "object") walk(v, ctr); }
    };
    walk(o.ast, null);
  }
  return fns;
}

const picked = [], seen = new Set();
for (const c of (q.rows || [])) {
  if (picked.length >= 10 || seen.has(c.name)) continue;
  try {
    const j = await fetch(`https://sourcify.dev/server/v2/contract/${c.chain}/${c.address}?fields=sources`).then((r) => r.ok ? r.json() : null);
    if (!j || !j.sources) continue;
    if (!Object.keys(j.sources).join("|").toLowerCase().includes("openzeppelin")) continue; // confirmed OZ user
    let fns = fingerprintFns(j.sources);
    if (!fns) continue;
    fns = fns.filter((f) => f.loc >= 3);
    const rec = fns.filter((f) => f.lib).length;
    if (rec < 5 || fns.length < 8 || fns.length > 80) continue;
    seen.add(c.name);
    picked.push({ name: c.name, chain: c.chain, address: c.address, version: c.version.split("+")[0],
      url: "https://sourcify.dev/#/lookup/" + c.address, fns });
    console.log(`${c.name.padEnd(20)} ${c.address.slice(0, 10)} ${rec}/${fns.length} recognized`);
  } catch { /* skip */ }
}
const data = { generated: new Date().toISOString().slice(0, 10), contracts: picked };
writeFileSync(join(HERE, "../realdata.js"), "window.RIFFCAT_REAL = " + JSON.stringify(data) + ";\n");
console.log(`\nwrote realdata.js: ${picked.length} contracts`);
