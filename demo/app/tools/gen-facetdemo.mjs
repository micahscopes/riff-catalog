// Generate facetdemo.js: a handful of tiny, fully-readable functions plus the
// solc AST, so the intro chapter can fingerprint them live in the browser and
// show what each facet does. Reproducible: needs solc + the built wasm engine.
// Run: bun tools/gen-facetdemo.mjs
import { readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
const HERE = import.meta.dir, DIST = join(HERE, "../dist");
const glue = readdirSync(DIST).find((f) => /^riffcat-demo-app-.*\.js$/.test(f) && !f.includes("_bg"));
const wasm = readdirSync(DIST).find((f) => f.endsWith("_bg.wasm"));
const b = await import(join(DIST, glue));
await b.default({ module_or_path: "file://" + join(DIST, wasm) });
const solc = "/run/current-system/sw/bin/solc";

const SRC = `// SPDX-License-Identifier: MIT
pragma solidity >=0.8.0;
contract Facets {
    function add(uint a, uint b) public pure returns (uint) { return a + b; }
    function plus(uint x, uint y) public pure returns (uint) { return x + y; }
    function tenth(uint n) public pure returns (uint) { return n * 10 / 100; }
    function fifth(uint n) public pure returns (uint) { return n * 20 / 100; }
    function mulAdd(uint a, uint b) public pure returns (uint) { return a * b + a; }
}
`;
const buf = Buffer.from(SRC, "utf8");
const pr = Bun.spawnSync([solc, "--standard-json"], { stdin: Buffer.from(JSON.stringify(
  { language: "Solidity", sources: { "Facets.sol": { content: SRC } }, settings: { outputSelection: { "*": { "": ["ast"] } } } })) });
const ast = JSON.parse(pr.stdout.toString()).sources["Facets.sol"].ast;
const units = JSON.parse(b.fingerprint_source(JSON.stringify(ast), "shape")).filter((u) => u.unit === "sol-fn");

// slice each function's exact source from the AST src offsets
const ORDER = ["add", "plus", "tenth", "fifth", "mulAdd"];
const srcOf = {};
(function walk(n){ if(!n||typeof n!=="object")return;
  if(n.nodeType==="FunctionDefinition"&&n.name){ const[o,l]=n.src.split(":").map(Number); srcOf[n.name]=buf.subarray(o,o+l).toString(); }
  for(const k in n){const v=n[k]; if(Array.isArray(v))v.forEach(walk); else if(v&&typeof v==="object")walk(v);} })(ast);

const byName = Object.fromEntries(units.map((u) => [u.name.split(".").pop(), u]));
const FACETS = ["full", "names-blind", "structure"];
console.log("=== facet digests (first 8 hex) ===");
for (const nm of ORDER) { const u = byName[nm]; console.log(`  ${nm.padEnd(8)} ` + FACETS.map((f)=>`${f}:${u.facets[f].slice(0,8)}`).join("  ")); }
console.log("\n=== same-shape groups per facet ===");
for (const f of FACETS) {
  const g = {}; for (const nm of ORDER) { const d = byName[nm].facets[f]; (g[d]=g[d]||[]).push(nm); }
  console.log(`  ${f.padEnd(12)} ` + Object.values(g).map((s)=>"{"+s.join(",")+"}").join("  "));
}

const out = { source: SRC, order: ORDER, fns: ORDER.map((nm) => ({ name: nm, src: srcOf[nm] })),
  ast: JSON.stringify(ast) };
writeFileSync(join(HERE, "../facetdemo.js"), "window.RIFFCAT_FACET = " + JSON.stringify(out) + ";\n");
console.log("\nwrote facetdemo.js");
