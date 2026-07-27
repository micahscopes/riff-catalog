// Generate namesdata.js: the same function, compiled in two surroundings, so the
// deck can show that a generated name belongs to the file rather than to the
// function. Two sources that share a byte-identical `withdraw`; the second adds
// one unrelated function above it. solc derives the Yul function name from a
// parse-order node id, so the id moves. Reproducible: needs solc.
// Run: bun tools/gen-namesdata.mjs
import { writeFileSync } from "node:fs";
import { join } from "node:path";
const HERE = import.meta.dir;
const solc = "/run/current-system/sw/bin/solc";

const HEAD = `// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

contract Vault {
    mapping(address => uint256) balances;
`;
// The one function we track. Byte-identical in both builds.
const TRACKED = `
    function withdraw(address to, uint256 amount) external {
        require(balances[msg.sender] >= amount, "insufficient");
        balances[msg.sender] -= amount;
        balances[to] += amount;
    }
`;
// The only difference between the two builds: this sits above withdraw in B.
const ADDED = `
    function version() external pure returns (uint256) {
        return 2;
    }
`;
const BUILDS = [
  { label: "as written", src: HEAD + TRACKED + "}\n" },
  { label: "one unrelated function added above", src: HEAD + ADDED + TRACKED + "}\n" },
];

function yulName(src) {
  const pr = Bun.spawnSync([solc, "--ir", "-"], { stdin: Buffer.from(src, "utf8") });
  const out = pr.stdout.toString();
  if (pr.exitCode !== 0) throw new Error("solc failed: " + pr.stderr.toString().slice(0, 400));
  const m = out.match(/function (fun_withdraw_\d+)/);
  if (!m) throw new Error("no fun_withdraw_* in the generated Yul");
  return m[1];
}

const builds = BUILDS.map((b) => ({ label: b.label, name: yulName(b.src) }));
if (new Set(builds.map((b) => b.name)).size !== builds.length)
  throw new Error("the names did not move; this compiler version numbers differently and the slide would be false");

for (const b of builds) console.log(`  ${b.label.padEnd(38)} ${b.name}`);

const out = {
  solc: Bun.spawnSync([solc, "--version"]).stdout.toString().trim().split("\n").pop(),
  tracked: TRACKED.trim(),
  added: ADDED.trim(),
  builds,
};
writeFileSync(join(HERE, "../namesdata.js"), "window.RIFFCAT_NAMES = " + JSON.stringify(out) + ";\n");
console.log("\nwrote namesdata.js");
