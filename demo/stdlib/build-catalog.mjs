// Build the std-lib recognition catalog from the vendored sources.
//
// For each vendored library, compile its whole source tree to AST, fingerprint
// every function at sol-fn (source level, names-blind + structure), and record
// the mapping from fingerprint -> {library, version, contract.function, file}.
// A real contract's function is "recognized" when its names-blind fingerprint
// is a key in this catalog: same shape, regardless of identifiers or layout.
//
// Reproducible: reads the pinned sources under ./<lib>/, uses the locally built
// wasm engine in ../app/dist. Run: bun build-catalog.mjs  (after refresh.sh).
import { readFileSync, readdirSync, writeFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const HERE = import.meta.dir;
const DIST = join(HERE, "../app/dist");
const glue = readdirSync(DIST).find((f) => /^riffcat-demo-app-.*\.js$/.test(f) && !f.includes("_bg"));
const wasm = readdirSync(DIST).find((f) => f.endsWith("_bg.wasm"));
const b = await import(join(DIST, glue));
await b.default({ module_or_path: "file://" + join(DIST, wasm) });

const solc = "/run/current-system/sw/bin/solc";
const manifest = JSON.parse(readFileSync(join(HERE, "manifest.json"), "utf8"));
const ver = Object.fromEntries(manifest.libraries.map((l) => [l.name, l.ref]));
const norm = (s) => s.replace(/pragma solidity[^;]*;/g, "pragma solidity >=0.8.0;");

function walkSol(dir) {
  const out = [];
  for (const e of readdirSync(dir)) {
    const p = join(dir, e);
    if (statSync(p).isDirectory()) out.push(...walkSol(p));
    else if (e.endsWith(".sol")) out.push(p);
  }
  return out;
}

const catalog = {};   // nb -> {lib, version, name, file, loc, st}
let dups = 0, crossLib = 0, total = 0;

for (const lib of ["openzeppelin", "solady", "solmate"]) {
  const root = join(HERE, lib);
  const files = walkSol(root);
  const sources = {};
  for (const f of files) sources[relative(root, f)] = { content: norm(readFileSync(f, "utf8")) };
  const pr = Bun.spawnSync([solc, "--standard-json"],
    { stdin: Buffer.from(JSON.stringify({ language: "Solidity", sources, settings: { outputSelection: { "*": { "": ["ast"] } } } })) });
  const comp = JSON.parse(pr.stdout.toString());
  if (!comp.sources) { console.log(lib, "compile produced no sources"); continue; }
  let libFns = 0;
  for (const [path, o] of Object.entries(comp.sources)) {
    if (!o.ast) continue;
    let units;
    try { units = JSON.parse(b.fingerprint_source(JSON.stringify(o.ast), "shape")); }
    catch { continue; }
    const byName = Object.fromEntries(units.filter((u) => u.unit === "sol-fn").map((u) => [u.name, u]));
    const walk = (n, ctr) => {
      if (!n || typeof n !== "object") return;
      if (n.nodeType === "ContractDefinition") { for (const ch of (n.nodes || [])) walk(ch, n.name); return; }
      if (n.nodeType === "FunctionDefinition" && ctr) {
        const nm = n.name || n.kind, u = byName[ctr + "." + nm];
        if (u) {
          const [, l] = n.src.split(":").map(Number);
          const loc = norm(readFileSync(join(root, path), "utf8")).slice(n.src.split(":")[0]|0, (n.src.split(":")[0]|0) + l).split("\n").length;
          if (loc >= 3) {
            total++; libFns++;
            const nb = u.facets["names-blind"];
            if (catalog[nb]) { dups++; if (catalog[nb].lib !== lib) crossLib++; }
            else catalog[nb] = { lib, version: ver[lib], name: ctr + "." + nm, file: path, loc, st: u.facets.structure.slice(0, 16) };
          }
        }
      }
      for (const k in n) { const v = n[k]; if (Array.isArray(v)) v.forEach((x) => walk(x, ctr)); else if (v && typeof v === "object") walk(v, ctr); }
    };
    walk(o.ast, null);
  }
  console.log(`${lib} ${ver[lib]}: ${libFns} functions (loc>=3) over ${files.length} files`);
}

const meta = { generated_from: "demo/stdlib vendored sources", versions: ver,
  distinct_shapes: Object.keys(catalog).length, total_functions: total, duplicate_shapes: dups, cross_library_collisions: crossLib };
writeFileSync(join(HERE, "catalog.json"), JSON.stringify({ meta, catalog }, null, 0));
console.log("\ncatalog:", meta);
console.log("wrote catalog.json");
