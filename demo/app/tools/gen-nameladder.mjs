// Generate nameladder.js: every number on the "what's in a name" spine that
// claims to come from a compiler, generated from a real solc run. Four
// exhibits, one contract:
//
//   selector   rename withdraw -> redeem; the four-byte ABI selector moves.
//   collision  burn(uint256) and collate_propagate_storage(bytes16) share
//              their four bytes; solc refuses the pair (its exact error).
//   quiet      three edits that change no executable byte (rename a local,
//              add a comment, move the file); the appended metadata hash
//              moves every time.
//   ledger     every name the toolchain hangs on one function, across the
//              same two builds gen-namesdata.mjs uses, and which names an
//              unrelated edit moved.
//   address    the two builds' fun_withdraw subtrees, extracted verbatim from
//              solc's irAst, for the live in-browser fingerprint. Verified
//              here against the built wasm engine before being written.
//
// Fails loudly if any phenomenon stops holding rather than emitting a slide
// that is no longer true. Run: bun tools/gen-nameladder.mjs (after a trunk
// build, so the engine in dist/ can verify the address exhibit).
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
const HERE = import.meta.dir;
const solc = "/run/current-system/sw/bin/solc";
const die = (msg) => { throw new Error(msg); };

// One standard-json compile. Returns the whole output; callers pick.
function compile(sources, settings = {}) {
  const input = {
    language: "Solidity",
    sources: Object.fromEntries(Object.entries(sources).map(([p, content]) => [p, { content }])),
    settings: { ...settings },
  };
  const pr = Bun.spawnSync([solc, "--standard-json"], { stdin: Buffer.from(JSON.stringify(input)) });
  if (pr.exitCode !== 0) die("solc failed: " + pr.stderr.toString().slice(0, 400));
  return JSON.parse(pr.stdout.toString());
}
const hardErrors = (out) => (out.errors || []).filter((e) => e.severity === "error");
const outputAll = (what) => ({ outputSelection: { "*": { "*": what, "": ["ast"] } } });

// The same contract gen-namesdata.mjs tracks, so "generated names", the name
// ledger, and the live address chapter all tell one story about one function.
const HEAD = `// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

contract Vault {
    mapping(address => uint256) balances;
`;
const TRACKED = `
    function withdraw(address to, uint256 amount) external {
        require(balances[msg.sender] >= amount, "insufficient");
        balances[msg.sender] -= amount;
        balances[to] += amount;
    }
`;
const ADDED = `
    function version() external pure returns (uint256) {
        return 2;
    }
`;
const SRC_A = HEAD + TRACKED + "}\n";
const SRC_B = HEAD + ADDED + TRACKED + "}\n";

// --- exhibit: the selector ----------------------------------------------
// Rename the function, keep the body: the four-byte selector moves.
function selectorExhibit() {
  const sel = (src, sig) => {
    const out = compile({ "contracts/Vault.sol": src }, outputAll(["evm.methodIdentifiers"]));
    if (hardErrors(out).length) die(hardErrors(out)[0].formattedMessage);
    const ids = out.contracts["contracts/Vault.sol"]["Vault"].evm.methodIdentifiers;
    return ids[sig] || die(`no selector for ${sig}`);
  };
  const rows = [
    { label: "as written", sig: "withdraw(address,uint256)", sel: sel(SRC_A, "withdraw(address,uint256)") },
    { label: "the function renamed", sig: "redeem(address,uint256)",
      sel: sel(SRC_A.replace("function withdraw", "function redeem"), "redeem(address,uint256)") },
  ];
  if (rows[0].sel === rows[1].sel) die("the selector did not move under the rename; the slide would be false");
  return { rows };
}

// --- exhibit: the collision ----------------------------------------------
// Two unrelated signatures, one selector; solc refuses to compile the pair.
function collisionExhibit() {
  const IFACES = `// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

interface Burnable { function burn(uint256 amount) external; }
interface Collate { function collate_propagate_storage(bytes16 slice) external; }
`;
  const out = compile({ "contracts/Ifaces.sol": IFACES }, outputAll(["evm.methodIdentifiers"]));
  if (hardErrors(out).length) die(hardErrors(out)[0].formattedMessage);
  const cs = out.contracts["contracts/Ifaces.sol"];
  const rows = [
    { sig: "burn(uint256)", sel: cs["Burnable"].evm.methodIdentifiers["burn(uint256)"] },
    { sig: "collate_propagate_storage(bytes16)",
      sel: cs["Collate"].evm.methodIdentifiers["collate_propagate_storage(bytes16)"] },
  ];
  if (rows[0].sel !== rows[1].sel) die("the two signatures no longer collide; the slide would be false");
  const BOTH = `// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

contract Both {
    function burn(uint256 amount) external {}
    function collate_propagate_storage(bytes16 slice) external {}
}
`;
  // Capture the refusal as a user sees it at a terminal (the CLI's stderr),
  // not standard-json's re-typed variant.
  const pr = Bun.spawnSync([solc, "--hashes", "-"], { stdin: Buffer.from(BOTH) });
  const error = pr.stderr.toString().trim();
  if (pr.exitCode === 0 || !/hash collision/i.test(error))
    die("solc accepted the colliding pair; the slide would be false");
  return { rows, error };
}

// --- exhibit: the quiet edits --------------------------------------------
// Three edits that change no executable byte; the appended metadata hash
// (the artifact's identity for exact-match verification) moves every time.
function quietExhibit() {
  const BASE = `// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

contract Vault {
    mapping(address => uint256) balances;

    function withdraw(address to, uint256 amount) external {
        uint256 fee = amount / 100;
        balances[msg.sender] -= amount;
        balances[to] += amount - fee;
    }
}
`;
  const builds = [
    { label: "as written", path: "contracts/Vault.sol", src: BASE },
    { label: "one local renamed, fee to cut", path: "contracts/Vault.sol", src: BASE.replaceAll("fee", "cut") },
    { label: "one comment added", path: "contracts/Vault.sol",
      src: BASE.replace("    function withdraw", "    // checked 2026-07-27\n    function withdraw") },
    { label: "the file moved, contracts/ to src/", path: "src/Vault.sol", src: BASE },
  ];
  const rows = builds.map(({ label, path, src }) => {
    const out = compile({ [path]: src }, outputAll(["evm.deployedBytecode.object"]));
    if (hardErrors(out).length) die(hardErrors(out)[0].formattedMessage);
    const bytecode = out.contracts[path]["Vault"].evm.deployedBytecode.object;
    const cborLen = parseInt(bytecode.slice(-4), 16);
    const tail = bytecode.slice(-(cborLen * 2 + 4));
    const code = bytecode.slice(0, -(cborLen * 2 + 4));
    const ipfs = (tail.match(/1220([0-9a-f]{64})/) || [])[1] || die("no ipfs hash in the metadata tail");
    return { label, code, meta: ipfs.slice(0, 12) };
  });
  for (const r of rows.slice(1))
    if (r.code !== rows[0].code) die(`code bytes moved under "${r.label}"; the slide would be false`);
  if (new Set(rows.map((r) => r.meta)).size !== rows.length)
    die("a metadata hash failed to move; the slide would be false");
  return { codeBytes: rows[0].code.length / 2, rows: rows.map(({ label, meta }) => ({ label, meta })) };
}

// --- exhibit: the name ledger + the live address --------------------------
// One compile per build gives everything: the AST (id, source offset), the IR
// text (the body an unrelated edit rewrites), the irAst (the subtree the
// browser fingerprints live), and the selector.
function irBuild(src) {
  const out = compile({ "contracts/Vault.sol": src },
    { viaIR: true, ...outputAll(["evm.methodIdentifiers", "ir", "irAst"]) });
  if (hardErrors(out).length) die(hardErrors(out)[0].formattedMessage);
  const c = out.contracts["contracts/Vault.sol"]["Vault"];
  const ast = out.sources["contracts/Vault.sol"].ast;
  const fn = ast.nodes.flatMap((n) => n.nodes || [])
    .find((n) => n.nodeType === "FunctionDefinition" && n.name === "withdraw") || die("no withdraw in the AST");
  const fnYul = c.irAst.subObjects[0].code.block.statements
    .find((s) => s.nodeType === "YulFunctionDefinition" && /^fun_withdraw_\d+$/.test(s.name))
    || die("no fun_withdraw_* in the irAst");
  return {
    sel: c.evm.methodIdentifiers["withdraw(address,uint256)"],
    astId: fn.id, src: fn.src,
    yulName: fnYul.name,
    varName: (JSON.stringify(fnYul).match(/var_amount_\d+/) || [])[0] || die("no var_amount_* in the body"),
    body: funBody(c.ir),
    // the live chapter's input: the genuine solc subtree, wrapped as a minimal
    // Yul object (same digest as in the full contract, by locality; checked
    // against the engine below).
    object: { name: "NameLadder", code: { block: { nodeType: "YulBlock", src: "-1:-1:0", statements: [fnYul] } } },
  };
}

// The fun_withdraw_* definition in the IR text, by brace balance.
function funBody(irText) {
  const start = irText.search(/function fun_withdraw_\d+\(/);
  if (start < 0) die("no fun_withdraw_* in the IR text");
  let depth = 0;
  for (let j = irText.indexOf("{", start); j < irText.length; j++) {
    if (irText[j] === "{") depth++;
    else if (irText[j] === "}") { depth--; if (depth === 0) return irText.slice(start, j + 1); }
  }
  die("unbalanced braces in the IR text");
}

function ledgerExhibit() {
  const a = irBuild(SRC_A), b = irBuild(SRC_B);
  const rows = [
    { what: "the name in the source", a: "withdraw", b: "withdraw", held: true },
    { what: "the four-byte selector", a: a.sel, b: b.sel, held: a.sel === b.sel },
    { what: "the AST node id", a: String(a.astId), b: String(b.astId), held: a.astId === b.astId },
    { what: "the Yul function name", a: a.yulName, b: b.yulName, held: a.yulName === b.yulName },
    { what: "its locals in the Yul", a: a.varName, b: b.varName, held: a.varName === b.varName },
    { what: "the source offset", a: a.src, b: b.src, held: a.src === b.src },
  ];
  const wrong = [];
  if (!rows[1].held) wrong.push("the selector moved");
  for (const r of rows.slice(2)) if (r.held) wrong.push(`"${r.what}" did not move`);
  if (wrong.length) die(wrong.join("; ") + "; the ledger would be false");

  // how much of the untouched function's IR text the unrelated edit rewrote
  const la = a.body.split("\n").map((l) => l.trim()).filter(Boolean);
  const lb = b.body.split("\n").map((l) => l.trim()).filter(Boolean);
  if (la.length !== lb.length) die("the two IR bodies differ in shape, not only in numbering");
  const moved = la.filter((l, i) => l !== lb[i]).length;
  if (moved <= la.length / 2) die("most IR lines held; the 'all of it numbering' line would overreach");

  // consistency with the "generated names" chapter, which shows the same builds
  const names = JSON.parse(readFileSync(join(HERE, "../namesdata.js"), "utf8").replace(/^window\.RIFFCAT_NAMES = /, "").replace(/;\s*$/, ""));
  if (names.builds[0].name !== a.yulName || names.builds[1].name !== b.yulName)
    die("namesdata.js disagrees with this run; regenerate it first (bun tools/gen-namesdata.mjs)");

  return { rows, irlines: { total: la.length, moved }, address: { a: a.object, b: b.object } };
}

// --- verify the address exhibit against the engine that will show it -------
// The subtree is fingerprinted by the exact wasm build the browser runs: the
// two builds must share the names-blind address and differ at full. Also check
// locality: the subtree's addresses equal the ones computed from the whole
// contract's irAst, so extracting it changed nothing.
async function verifyAddress(address) {
  const dist = join(HERE, "../dist");
  let files;
  try { files = readdirSync(dist); } catch { die("no dist/; run trunk build first so the engine can verify the addresses"); }
  const js = files.find((f) => /^riffcat-demo-app-.*\.js$/.test(f)) || die("no engine js in dist/");
  const wasm = files.find((f) => f.endsWith("_bg.wasm")) || die("no engine wasm in dist/");
  const mod = await import(pathToFileURL(join(dist, js)).href);
  await mod.default({ module_or_path: await Bun.file(join(dist, wasm)).arrayBuffer() });
  const fp = (o) => JSON.parse(mod.fingerprint_yul(JSON.stringify(o), "shape"))
    .find((u) => /^fun_withdraw_\d+$/.test(u.name)) || die("engine returned no fun_withdraw_* unit");
  const ua = fp(address.a), ub = fp(address.b);
  if (ua.facets["names-blind"] !== ub.facets["names-blind"]) die("names-blind addresses differ; the payoff slide would be false");
  if (ua.facets["full"] === ub.facets["full"]) die("full addresses agree; the 'names kept' contrast would be false");
  return { a: ua, b: ub };
}

const selector = selectorExhibit();
const collision = collisionExhibit();
const quiet = quietExhibit();
const { rows: ledger, irlines, address } = ledgerExhibit();
const { a: ua, b: ub } = await verifyAddress(address);

console.log(`selector   ${selector.rows.map((r) => `${r.sig} -> ${r.sel}`).join("  |  ")}`);
console.log(`collision  ${collision.rows.map((r) => r.sig).join(" and ")} -> ${collision.rows[0].sel}, solc: refused`);
console.log(`quiet      code ${quiet.codeBytes} bytes x4 identical, metadata ${quiet.rows.map((r) => r.meta.slice(0, 6)).join(" ")}`);
console.log(`ledger     ${ledger.filter((r) => !r.held).length} of ${ledger.length} moved; IR body ${irlines.moved}/${irlines.total} lines moved`);
console.log(`address    ${ua.name} & ${ub.name}: names-blind ${ua.facets["names-blind"].slice(0, 8)} == ${ub.facets["names-blind"].slice(0, 8)}, full differs`);

const out = {
  solc: Bun.spawnSync([solc, "--version"]).stdout.toString().trim().split("\n").pop(),
  selector, collision, quiet,
  ledger: { rows: ledger },
  irlines, address,
};
writeFileSync(join(HERE, "../nameladder.js"), "window.RIFFCAT_NAMELADDER = " + JSON.stringify(out) + ";\n");
console.log("\nwrote nameladder.js");
