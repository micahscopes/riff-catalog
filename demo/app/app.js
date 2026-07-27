// riffcat storybook. The chapters are a guided read; the "drive the dial"
// chapter computes fingerprints live in the browser by calling the wasm engine
// (riff-catalog compiled to wasm32) that Trunk exposes on window.wasmBindings.
//
// Vanilla custom elements, no framework, no build step beyond Trunk's wasm.

// Engine readiness: Trunk's loader sets window.wasmBindings then fires the
// event. Resolve immediately if we loaded after it already fired.
const engineReady = window.wasmBindings
  ? Promise.resolve(window.wasmBindings)
  : new Promise((resolve) =>
      addEventListener(
        "TrunkApplicationStarted",
        () => resolve(window.wasmBindings),
        { once: true },
      ),
    );

const badge = document.getElementById("engine");
engineReady
  .then(() => {
    badge.textContent = "wasm live ✓";
  })
  .catch(() => {
    badge.textContent = "wasm failed";
    badge.classList.add("bad");
  });

const short = (hex) => (hex || "").slice(0, 8);

// Session-scoped memo for yul fingerprints: each (fixture, mode) runs through
// the engine at most once. The live chapters read through this, so navigating
// back to one is an instant re-render instead of re-running the engine. The
// library chapter alone is ~0.5s of synchronous work across three fixtures, and
// tour-app rebuilds the chapter element on every visit, so without this the
// cost was paid again on every switch into the tab.
const yulCache = new Map(); // `${id}:${mode}` -> { units, ms }
function yulUnits(b, id, mode) {
  const key = id + ":" + mode;
  let v = yulCache.get(key);
  if (!v) {
    const t0 = performance.now();
    const units = JSON.parse(b.fingerprint_yul(b.fixture(id), mode));
    v = { units, ms: performance.now() - t0 };
    yulCache.set(key, v);
  }
  return v;
}

// Warm the live chapters right after boot, one fixture per idle slice, so the
// heavy synchronous engine calls fall in idle time at startup rather than on a
// tab switch. Best-effort: a failure just leaves that fixture to compute on
// first visit, exactly as before.
(function warmEngine() {
  const jobs = [
    ["demo", "shape"],
    ["demo", "identity"],
    ["oz-muldiv", "shape"],
    ["solady-muldiv", "shape"],
    ["solmate-muldiv", "shape"],
  ];
  const ric = window.requestIdleCallback || ((f) => setTimeout(f, 16));
  engineReady
    .then((b) => {
      const step = () => {
        const j = jobs.shift();
        if (!j) return;
        try {
          yulUnits(b, j[0], j[1]);
        } catch {
          /* leave it for first visit */
        }
        ric(step);
      };
      ric(step);
    })
    .catch(() => {});
})();

// --- equivalence visuals -------------------------------------------------
// Each function becomes a chip. Its class at a facet IS its fingerprint there,
// so we derive both a CSS class and a color straight from the digest: same
// shape -> same class -> same color, automatically, and hovering one chip can
// light its whole twin-network by selecting that one class (the fe trick).

// CSS-safe class key from a digest (hex, so already safe). Same digest -> same
// key -> shared class across every chip and every grid on the page.
const eqKey = (digest) => "eq-" + digest.slice(0, 12);

// Deterministic hue from a digest. Hash the prefix so neighbors in hex space
// still land on visibly different hues.
function digestHue(digest) {
  let h = 0;
  for (let i = 0; i < 10 && i < digest.length; i++)
    h = (h * 131 + digest.charCodeAt(i)) >>> 0;
  return h % 360;
}

// Chip color: a perceptually-uniform OKLCH color for a shape digest. Prefer the
// Rust harmonizer (palette, gamut-safe, golden-angle hue) the engine exposes;
// fall back to a JS OKLCH with the same golden-angle hue if a chip happens to
// render before the engine is live. The whole chip family (dark fill, bright
// accent, light text) is mixed from this one accent in CSS, in oklch.
function chipColor(digest) {
  const b = window.wasmBindings;
  if (b && b.chip_color) {
    try {
      return b.chip_color(digest);
    } catch (_) {
      /* fall through */
    }
  }
  let acc = 0;
  for (let i = 0; i < 12 && i < digest.length; i++)
    acc = (acc * 131 + digest.charCodeAt(i)) >>> 0;
  return `oklch(72% 0.13 ${((acc * 137.508) % 360).toFixed(1)})`;
}

// A distinct SHAPE per fingerprint class, so the coding is never color-only:
// same digest gets the same glyph as well as the same hue. Keyed off a later
// slice of the digest than the hue, so two near hues usually differ in glyph.
const SHAPE_SYMS = ["●", "■", "◆", "★", "▲", "▼", "◀", "▶", "✚"];
function chipSymbol(digest) {
  let h = 0;
  for (let i = 6; i < 16 && i < (digest || "").length; i++)
    h = (h * 131 + digest.charCodeAt(i)) >>> 0;
  return SHAPE_SYMS[h % SHAPE_SYMS.length];
}

// Distinct glyphs per view: the i-th distinct digest present gets SHAPE_SYMS[i].
function symMapFor(digests) {
  const m = new Map();
  for (const d of digests)
    if (d && !m.has(d)) m.set(d, SHAPE_SYMS[m.size % SHAPE_SYMS.length]);
  return m;
}

// Compact, human label for a yul function name (the name is shown for people,
// never folded into the fingerprint).
function chipLabel(name) {
  let s = name
    .replace(/^fun_/, "")
    .replace(/^constructor_\w*?_(\d+)$/, "constructor")
    .replace(/^constructor_/, "constructor·")
    .replace(/_\d+$/, "");
  return s.length > 18 ? s.slice(0, 17) + "…" : s;
}

// The five dimensions a fingerprint decomposes into, shown shortest-name first.
const DIMS = [
  ["structure", "struct"],
  ["names", "names"],
  ["constants", "const"],
  ["types", "types"],
  ["trace_events", "trace"],
];

// Render one function as a chip carrying its class + color + the per-dimension
// digests (so a hover can show which dimensions a twin-network shares vs differs
// on: the "faceted preimage signature" that explains why two differently-named
// functions are the same shape).
function chip(u, facet, lib, syms) {
  const d = u.facets[facet];
  const dims = DIMS.map(([k]) => short(u.digests[k]).slice(0, 6)).join(",");
  return (
    `<span class="chip ${eqKey(d)}" style="--chip:${chipColor(d)}" data-eq="${eqKey(d)}"` +
    ` data-name="${u.name}" data-fp="${short(d)}" data-dims="${dims}"${lib ? ` data-lib="${lib}"` : ""}` +
    ` title="${u.name}">${(syms && syms.get(d)) || chipSymbol(d)} ${chipLabel(u.name)}</span>`
  );
}

// Build the dimension breakdown for a hovered chip vs its lit network: a strip
// of cells (green = identical across the whole network, rose = varies) plus a
// plain-language note. This is what makes "they differ only in their names"
// legible: the names cell goes rose, every other cell stays green.
function dimStrip(c, lit) {
  const mine = c.dataset.dims.split(",");
  const arrs = [...lit].map((x) => x.dataset.dims.split(","));
  const same = (i) => arrs.every((a) => a[i] === mine[i]);
  const cells = DIMS.map(
    ([, lab], i) =>
      `<span class="dimcell ${same(i) ? "same" : "diff"}">${lab} ${mine[i]}</span>`,
  ).join("");
  const diff = DIMS.filter((_, i) => !same(i)).map(([, lab]) => lab);
  const note =
    lit.length === 1
      ? "unique here at this facet"
      : diff.length === 0
        ? `all <b>${lit.length}</b> identical in every dimension`
        : `<b>${lit.length}</b> identical except <span class="vary">${diff.join(", ")}</span>`;
  return `<div class="dims">${cells}</div><div class="dimnote">${note}</div>`;
}

// Wire hover highlighting for every .eqgrid inside `host`. Hovering a chip lights
// every chip sharing its class (across all grids in the host) and dims the rest,
// then writes a one-line readout. Delegated on `host` and wired once: re-renders
// replace the inner DOM, so handlers re-query rather than capture stale nodes.
// The idle readout is restored from the current `.eqread`'s data-idle attribute.
function wireHighlight(host, describe) {
  if (host._wired) return;
  host._wired = true;
  // mouseover lights the hovered chip's network (clearing the previous one
  // first, so moving chip-to-chip repaints cleanly). mouseleave clears, and
  // unlike mouseout it does not bubble and fires exactly once when the pointer
  // leaves the whole component, so the highlight can never get stuck.
  host.addEventListener("mouseover", (e) => {
    const c = e.target.closest(".chip");
    if (!c || !host.contains(c)) return;
    host
      .querySelectorAll(".chip.lit")
      .forEach((x) => x.classList.remove("lit"));
    const lit = host.querySelectorAll("." + c.dataset.eq);
    host.querySelectorAll(".eqgrid").forEach((g) => g.classList.add("focused"));
    lit.forEach((x) => x.classList.add("lit"));
    const read = host.querySelector(".eqread");
    if (read) read.innerHTML = describe(c, lit);
  });
  host.addEventListener("mouseleave", () => {
    host
      .querySelectorAll(".chip.lit")
      .forEach((x) => x.classList.remove("lit"));
    host
      .querySelectorAll(".eqgrid")
      .forEach((g) => g.classList.remove("focused"));
    const read = host.querySelector(".eqread");
    if (read) read.innerHTML = read.dataset.idle || "";
  });
}

// The chapter pool. Every payoff beat runs on real material: verified Sourcify
// sources, the Yul a contract compiles to, an ingested fe origin bundle, live
// engine calls. Nothing on a spine slide is a hand-drawn stand-in for something
// the engine can compute (the talk's cold open is the one staged enactment,
// and it says so on the slide). Chapters an arc does not name become its
// appendix: built, honest, and there for follow-up questions.
const CH = [
  {
    nav: "many forms",
    kicker: "what is actually there",
    title: "A myth: the compiler",
    lede: "There is no single thing called the compiler. There are representations of the program, and transformations between them.",
    body: `<div class="compiler-paths">
      <div class="compiler-path"><b>solc</b><span>source</span><i>→</i><span>AST</span><i>→</i><span>Yul</span><i>→</i><span>bytecode</span></div>
      <div class="compiler-path"><b>fe</b><span>source</span><i>→</i><span>HIR</span><i>→</i><span>sonatina IR</span><i>→</i><span>bytecode</span></div>
    </div>`,
  },
  {
    nav: "stranded",
    kicker: "the problem",
    title: "A different finding on each form",
    lede: "Each tool leaves what it learns on the form it works in, and none of it moves to the others on its own.",
    body: `<div class="result-attachments">
      <div><b>source</b><span>an audit finding</span></div>
      <div><b>IR</b><span>a compiler warning</span></div>
      <div><b>bytecode</b><span>an execution trace</span></div>
      <div><b>formal model</b><span>a proof</span></div>
    </div>`,
  },
  {
    nav: "broken reference",
    kicker: "the cost",
    title: "Change the compiler and the reference breaks",
    lede: "The check is almost certainly still in there. Nothing tells you where.",
    body: `<div class="broken-reference">
      <div><span>before</span><b>proof about IR node 184</b><small>the reference resolves</small></div>
      <div class="broken-arrow">→</div>
      <div><span>after one pass changes</span><b>node 184 is gone</b><small>the proof still names it</small></div>
    </div>`,
  },
  {
    nav: "generated names",
    kicker: "how the compiler names things",
    title: "The names the compiler generates",
    lede: "The same Solidity function, compiled twice; the second build adds one unrelated function above it. solc numbers the generated name from a parse counter, so the counter moves.",
    body: `<generated-names></generated-names>`,
  },
  {
    nav: "a rename",
    kicker: "the question",
    title: "A rename at six scales",
    lede: "Something is renamed and nothing else changes. Each row is who notices.",
    body: `<div class="figure"><div class="cap">the same edit, six scales</div><table>
      <tr><th>renamed</th><th>who notices</th></tr>
      <tr><td class="n">a local variable</td><td>no one else can even refer to it</td></tr>
      <tr><td class="n">a function argument</td><td>call sites that pass arguments by name</td></tr>
      <tr><td class="n">a function</td><td>every caller</td></tr>
      <tr><td class="n">a module</td><td>every import</td></tr>
      <tr><td class="n">the module tree</td><td>every path, in code and in tooling</td></tr>
      <tr><td class="n">a package version</td><td>every lockfile that pinned it</td></tr>
    </table></div>`,
  },
  {
    nav: "the selector",
    kicker: "a name on the wire",
    title: "The four bytes a name becomes",
    lede: "A caller reaches a Solidity function through four bytes: a hash of its name and argument types. Rename the function and the four bytes move.",
    body: `<sel-rename></sel-rename>`,
  },
  {
    nav: "one selector",
    kicker: "the same four bytes",
    title: "Two names, one selector",
    lede: "Two unrelated signatures whose hashes share their first four bytes. solc refuses to compile the pair into one contract; its exact error below.",
    body: `<sel-collide></sel-collide>`,
  },
  {
    nav: "quiet edits",
    kicker: "edits that change nothing",
    title: "Three edits that change no code",
    lede: "solc stamps every deployed artifact with a hash of the whole build input, file paths included, and exact-match verification keys on that stamp. Three edits that leave every executable byte identical.",
    body: `<quiet-edits></quiet-edits>`,
  },
  {
    nav: "every name",
    kicker: "ids nobody promised",
    title: "Every name one function has",
    lede: "The same two builds: every name the toolchain hangs on withdraw, and what one unrelated edit above it did to each. Ids like these are not stable within one compiler, let alone between versions, let alone between two compilers.",
    body: `<name-ledger></name-ledger>`,
  },
  {
    nav: "one address",
    kicker: "the payoff, live in this browser",
    title: "Two builds, one address",
    lede: "The same two builds' withdraw, fingerprinted in your browser this run. Choose whether the names go into the address.",
    body: `<held-address></held-address>`,
  },
  {
    nav: "an address",
    kicker: "what an address is",
    title: "What a content address is",
    lede: "Ask for the content with this address, hash what comes back, and you know whether it is right. Nobody has to be trusted.",
    body: `<address-check></address-check>`,
  },
  {
    nav: "two builds",
    kicker: "comparing two builds",
    title: "Two builds of the same program",
    lede: "Two compilers, or two branches of one. Four reasons the output differs.",
    body: `<div class="figure"><div class="cap">why the bytes disagree</div><table>
      <tr><td class="n">the generated names</td><td><code>fun_appId_185</code> and <code>fun_appId_1843</code></td><td>leave names out of the address</td></tr>
      <tr><td class="n">the order of independent operations</td><td>the same work, scheduled differently</td><td>would need an order-blind address, we do not have one</td></tr>
      <tr><td class="n">optimization choices</td><td>one inlines the call, the other does not</td><td>nothing. This one is a real difference</td></tr>
      <tr><td class="n">one small edit</td><td>moves the hash of the whole file</td><td>address the parts, so the change stays local</td></tr>
    </table></div>`,
  },
  {
    nav: "resolution",
    kicker: "inside the compiler",
    title: "Resolution by address",
    lede: "A path resolves through scope, imports, and whichever version you selected. An address resolves to one definition, everywhere. We have not built this; Unison has.",
    body: `<div class="identity-boundary">
      <div><span>by name</span><b>SafeERC20.safeTransfer</b><small>depends on the imports in scope and the version selected</small></div>
      <div><span>by address</span><b>7f3a91c4</b><small>one definition, the same one everywhere</small></div>
    </div>
`,
  },
  {
    nav: "facets",
    kicker: "on five functions",
    title: "Facets of similarity",
    lede: "Loosen what counts as the same, and watch which of these functions merge to one shape.",
    body: `<facet-primer></facet-primer>`,
  },
  {
    nav: "the riff",
    kicker: "the same idea, on music",
    title: "Riffs and riffing",
    lede: "A motif and its variants: choose what counts as the same, then press play.",
    body: `<riff-dial></riff-dial><div class="kicker" style="margin-top:24px">now on chords, parsed from real notation</div><chord-fp></chord-fp>`,
  },
  {
    nav: "address it",
    kicker: "the proposal",
    title: "Choose what must not change, then address it",
    lede: "Normalize the differences you allowed, then content-address what is left. A checksum keeps every byte; a facet keeps the boundary you declared.",
    body: `<div class="address-proposition">
      <div class="address-choice"><span>keep</span><b>structure</b><b>types</b><b>origins</b></div>
      <div class="address-arrow">→</div>
      <div class="address-result"><span>derive</span><b>one address for that choice</b></div>
    </div>`,
  },
  {
    nav: "recognized",
    kicker: "the pitch, on real mainnet code",
    title: "Recognizing known library code",
    lede: "Ten verified Sourcify contracts, every function marked by the library shape it matches; grey is new code.",
    body: `<recog-scan></recog-scan>`,
  },
  {
    nav: "three libraries",
    kicker: "the same job, three implementations",
    title: "Three libraries, side by side",
    lede: "Multiply and divide with rounding, as OpenZeppelin, Solady and Solmate compile it. Same color means same shape; hover one to trace it across all three.",
    body: `<live-xref></live-xref>`,
  },
  {
    nav: "twins",
    kicker: "the same shape, across real contracts",
    title: "The same function across contracts",
    lede: "The same library functions, exact same shape, recur across these contracts. Pick one to see everywhere it lands.",
    body: `<recog-twins></recog-twins>`,
  },
  {
    nav: "dedup",
    kicker: "how much is actually new",
    title: "How much code is actually new",
    lede: "Across ten real contracts: how much is a known library shape, and how much is new code someone has to audit.",
    body: `<recog-dedup></recog-dedup>`,
  },
  {
    nav: "sniff it out",
    kicker: "find a known bug by shape",
    title: "Finding a known bug by shape",
    lede: "A known bug is a shape: match it across verified vaults, and the patch reads as a different shape.",
    body: `<vuln-sniff></vuln-sniff>`,
  },
  {
    nav: "modified",
    kicker: "similarity is a spectrum",
    title: "Catching edited forks",
    lede: "Forks edit the function, so exact match and text search miss them; the subtree shapes still match.",
    body: `<fuzzy-scan></fuzzy-scan>`,
  },
  {
    nav: "the compiler too",
    kicker: "one level down, post-compilation",
    title: "Repetition in compiled code",
    lede: "The Yul this contract compiles to, fingerprinted: loosen the facet and the groups merge.",
    body: `<live-dial></live-dial>`,
  },
  {
    nav: "provenance",
    kicker: "fold riffcat into origin tracing",
    title: "Provenance rides along",
    lede: "Every lowered node keeps an edge back to its source, and the address is computed on the shape alone, so provenance rides as payload the catalog still dedups through. Point the same read at two branches of fe or sonatina, or two compilers of one contract: where the shape agrees they share an address, and where a pass moves the code the divergence is local.",
    body: `<provenance-rides></provenance-rides>`,
  },
  {
    nav: "two fingerprints",
    kicker: "two axes, not two rivals",
    title: "Two kinds of fingerprint",
    lede: "Edit the source: the metadata hash flips on any character, the structural fingerprint holds while the shape holds.",
    body: `<metadata-axes></metadata-axes>`,
  },
  {
    nav: "prove it",
    kicker: "the seat we leave open",
    title: "A match is a candidate, not a proof",
    lede: "A shape match is a candidate, not a verdict. riffcat points at the matching subtree and writes the proof obligation, then leaves the verdict to a named verifier. Pick a tool to see what it would have to discharge.",
    body: `<verdict-seat></verdict-seat>`,
  },
  {
    nav: "anchors",
    kicker: "a fact rides an address",
    title: "How a fact rides an address",
    lede: "Pin a fact to an address and it rides to every shape that shares it; loosen the anchor and watch where it rides wrong.",
    body: `<anchor-transport></anchor-transport>`,
  },
  {
    nav: "the cubical prototype",
    kicker: "the same rule, mechanized",
    title: "The cubical Agda prototype",
    lede: "Cubical type theory is built on the idea that equality is something you declare rather than something fixed. A facet is such a declaration, and the typechecker refuses to let a fact travel along one unless the fact respects it.",
    body: `<div class="figure"><div class="cap">what the prototype checks</div><table>
      <tr><td class="n">a facet</td><td>a declared identification, as a real type</td></tr>
      <tr><td class="n">the address</td><td>the normalization that decides it, without ever inspecting the proof</td></tr>
      <tr><td class="n">a fact riding an address</td><td>allowed only by supplying the proof that the fact respects the identification</td></tr>
      <tr><td class="n">the fact arrives unchanged</td><td>and it computes, which the Lean version cannot do</td></tr>
      <tr><td class="n">the hash</td><td>abstract here. The Lean port owns the bytes, this owns the laws</td></tr>
    </table></div>
`,
  },
  {
    nav: "prior art",
    kicker: "the primitive is older than us",
    title: "Where this idea comes from",
    lede: "Several systems reached this move separately; the hash is not the new part, the faceted explainable query is.",
    body: `<prior-art></prior-art>`,
  },
  {
    nav: "locality runs out",
    kicker: "the honest edge of a local address",
    title: "The limits of a local address",
    lede: "A bottom-up node digest is a function of its own subtree and nothing else (invariant I3). Two real cases push past that: a cycle has no leaf to fold from, and an instantiated shape gets its identity from a context the subtree cannot see. One ships a workaround, one stays an open edge.",
    body: `<locality-limit></locality-limit>`,
  },
  {
    nav: "what we sampled",
    kicker: "what we measured, and what we did not",
    title: "What we measured, and what we didn't",
    lede: "Every number is an exact count over Sourcify's verified sources; click a row for how it was made.",
    body: `<sampled-ledger></sampled-ledger>`,
  },
  {
    nav: "where we're headed",
    tonightOnly: true,
    kicker: "the vision, plainly",
    title: "Faceted addresses, many tools",
    lede: "Each of these tools has to decide when two pieces of code count as the same, and a facet is exactly that choice.",
    body: `<div class="vh-conv">
    <div class="vh-row"><span class="vh-w">compilers</span><span class="vh-h">pin facts to the shape, not the build</span></div>
    <div class="vh-row"><span class="vh-w">verification</span><span class="vh-h">carry a proof's scope with it</span></div>
    <div class="vh-row"><span class="vh-w">debugging</span><span class="vh-h">a breakpoint that survives every pass</span></div>
    <div class="vh-row"><span class="vh-w">reuse</span><span class="vh-h">recognize a shape you already audited</span></div>
    <div class="vh-row"><span class="vh-w">naming</span><span class="vh-h">resolve names to content</span></div>
    <svg class="vh-fan" viewBox="0 0 64 220" preserveAspectRatio="none" aria-hidden="true">
      <line x1="0" y1="22" x2="64" y2="110"/>
      <line x1="0" y1="66" x2="64" y2="110"/>
      <line x1="0" y1="110" x2="64" y2="110"/>
      <line x1="0" y1="154" x2="64" y2="110"/>
      <line x1="0" y1="198" x2="64" y2="110"/>
    </svg>
    <div class="vh-hub"><b>faceted content addresses</b></div>
  </div>
  <p class="twnote">A facet is the choice of what to forget; a shape is the structure left at that facet; an address is the content hash of that shape. The engine is content-blind over labelled graphs, so the same faceted addresses serve all of them.</p>`,
  },
  {
    nav: "what we need",
    kicker: "what do you think?",
    title: "Help!",
    lede: "riff-catalog gives parts of programs stable addresses, what can we build on top?",
    body: `<div class="wn-loop">
    <div class="wn-row wn-riffcat"><span class="wn-who">riffcat</span>
      <span class="wn-brings">gives a piece of code an address from its structure, the same across its representations</span>
      <span class="wn-q">Which differences must that address keep for your tool?</span></div>
    <div class="wn-row wn-compiler"><span class="wn-who">a compiler</span>
      <span class="wn-brings">produces those representations, and could carry each piece's origin through its passes</span>
      <span class="wn-q">What would carrying those origins cost, and where would you want them?</span></div>
    <div class="wn-row wn-verifier"><span class="wn-who">a verifier</span>
      <span class="wn-brings">proves what the code does, or gives a counterexample</span>
      <span class="wn-q">What would you need before a syntactic match is worth checking for semantic equivalence?</span></div>
  </div>
  <p class="twnote">A facet is the choice of what to forget; a shape is the structure left at that facet; an address is the content hash of that shape. riffcat only narrows the search: a match is a candidate for a verifier, not a decision.</p>`,
  },
  {
    nav: "a shared block",
    kicker: "the offer",
    title: "A library and a thin CLI",
    lede: "The whole library is three verbs.",
    body: `<div class="sb2-verbs">
    <div><b>fingerprint</b><small>a handle for what you chose to keep</small></div>
    <div><b>recognize</b><small>find a known shape, point at the parts that match</small></div>
    <div><b>similar</b><small>catch edited versions that still carry it</small></div>
  </div>
  <p class="sb2-cli">the CLI: <code>ingest</code> · <code>query by shape</code> · <code>diff</code></p>
  <p class="twnote">fe let us build provenance in from the start, so the producer side is real; it is not something you have to adopt.</p>`,
  },
  {
    nav: "structure vs meaning",
    kicker: "the substrate, not the climb",
    title: "Structure as the substrate for meaning",
    lede: "riffcat stays on the syntactic side, a lattice of facets, each a content address. Forgetting more moves toward meaning but never reaches it, and is not meant to: it is the substrate a semantic claim pins to.",
    body: `<facet-lattice></facet-lattice>`,
  },
  {
    nav: "the fold",
    kicker: "how a facet address is made",
    title: "How the address is computed",
    lede: "One small unit lowers to a graph. Each node gets a context-free local digest, then the addresses snap in bottom-up: a parent folds its own local content with its children's tree digests, and the whole-graph digest lands last. Step through the fold, then drop a dimension and watch every address change.",
    body: `<fold-merkle></fold-merkle>`,
  },
  {
    nav: "the cheap yes",
    kicker: "generalizing a fast path hevm already ships",
    title: "When structure can skip the solver",
    lede: "hevm's byte-identical fast path, generalized to a facet: loosen it and watch where the yes stays sound.",
    body: `<cy-cheap-yes></cy-cheap-yes>`,
  },
  {
    nav: "seat filled",
    kicker: "the seat, filled: a Lean proof takes it",
    title: "A cited proof of equivalence",
    lede: "The occupant is cited, not run: a pinned EquiVM Lean proof (argotorg/EquiVM) that a solc-built ERC20 runtime is observationally equivalent to a hand-written Solm spec, four-way case split, trust boundary named. Pinned to a facet address, it carries to every behavior-complete twin; the Solm spec is itself a forgetting, so it too is a facet. Nothing here runs Lean.",
    body: `<seat-filled></seat-filled>`,
  },
  {
    nav: "the proofs check",
    kicker: "one rung up: the map itself is proved",
    title: "The normal form, proved",
    lede: "A kernel-checked Lean proof that the prime form riffcat lands on really is the canonical one for its transpose-and-invert class. The proof is content-addressed and pinned to the one facet where it holds. Hover the badge to read each part honestly.",
    body: `<verified-attest></verified-attest>`,
  },
  {
    nav: "the engine checks too",
    kicker: "the same discipline, turned inward",
    title: "Checking the engine against itself",
    lede: "The digest is written twice: Rust, the engine that ships, and Lean, where the rules are theorems. Every ordering is canonical, so both hash one corpus and the bytes must match in CI. This is a cited plan over the existing golden tests, not a live proof; the Lean run waits on the deferred toolchain.",
    body: `<lockstep-bench></lockstep-bench>`,
  },
  {
    nav: "two instantiations",
    kicker: "the edge of the method: one generic, two ways",
    title: "Where structure agrees and types differ",
    lede: "One generic shape, instantiated two ways. At the structure facet they share one address, one skeleton; keep the type spelling and they part. The honest gap: that split is the syntactic shadow of the instantiation, not the monomorphized form.",
    body: `<two-instantiations></two-instantiations>`,
  },
  {
    nav: "main vs meta",
    kicker: "the wall riffcat steps around",
    title: "Code and metadata, interleaved",
    lede: "Raw onchain bytecode is one run-on strip: code (Main) interleaved with metadata, immutables, libraries, constructor arguments (Meta), no clean line between them. riffcat works one level up, on source, where structure, names, constants, and types are separate by construction.",
    body: `<byte-wall></byte-wall>`,
  },
  {
    nav: "forte catalog",
    kicker: "structure lands on a published catalog",
    title: "The chord catalog, from structure alone",
    lede: "Eight chords, parsed from notation, land on the published catalog at the set-class setting, A/B letters and all. The only label riffcat is given is Forte's name; the rest is read off the structure. Hit invert: A and B swap, the symmetric chords hold still.",
    body: `<forte-catalog></forte-catalog>`,
  },
  {
    nav: "interval vector",
    kicker: "the harmonic signature underneath a chord",
    title: "A chord's interval signature",
    lede: "Every chord reduces to six numbers, its interval counts from the minor second up to the tritone. Move it to any key or flip it over and the six hold. Pick a chord; major and minor share a signature.",
    body: `<ivf-fingerprint></ivf-fingerprint>`,
  },
  {
    nav: "three rungs",
    kicker: "one chord, three rungs of forgetting",
    title: "Three rungs of similarity",
    lede: "Take one chord up three rungs: the literal notes; the same set packed tight, with the key forgotten; then the prime form, which also folds mirror images together. Each rung forgets one more thing.",
    body: `<three-rungs></three-rungs>`,
  },
  // --- the talk deck ("A name you can check") ------------------------------
  // Staging slides for ?arc=talk only. talkOnly keeps them out of every other
  // arc's appendix, so those arcs keep today's chapter list and arrow-key
  // behavior exactly. Sub-slide builds are .reveal[data-at] blocks that
  // tour-app's arrow keys walk before changing chapters. The live slides
  // (one address, recognized, sniff it out) are the existing pool chapters,
  // named by the arc rather than duplicated here. Every solc-derived number is
  // read from nameladder.js at render time; the maintainer quotes are verbatim
  // from the trackers and attributed at repo level only.
  {
    nav: "riff catalog",
    tonightOnly: true,
    title: "Riff Catalog",
    body: ``,
  },
  {
    nav: "cold open",
    talkOnly: true,
    body: `<talk-cold-open></talk-cold-open>`,
  },
  {
    nav: "where knowledge lives",
    talkOnly: true,
    title: "Where knowledge lives",
    body: `<div class="compiler-paths"><div class="compiler-path talk-pipe">
        <span>source</span><i>→</i><span>AST</span><i>→</i><span>Yul</span><i>→</i><span>bytecode</span><i>→</i><span>trace</span><i>→</i><span>proof</span>
      </div></div>
      <div class="reveal" data-at="1"><div class="result-attachments talk-pins">
        <div><b>finding</b><span>file:line</span></div>
        <div><b>proof</b><span>IR node id</span></div>
        <div><b>trace</b><span>program counter</span></div>
        <div><b>verdict</b><span>bytecode hash</span></div>
      </div></div>`,
  },
  {
    nav: "names people choose",
    talkOnly: true,
    title: "Names people choose",
    body: `<talk-names-chosen></talk-names-chosen>`,
  },
  {
    nav: "edits that change nothing",
    talkOnly: true,
    title: "Edits that change nothing",
    body: `<talk-quiet-edits></talk-quiet-edits>`,
  },
  {
    nav: "names nobody chose",
    talkOnly: true,
    title: "Generated names",
    lede: "Don't touch the function, add one unrelated function above it.",
    body: `<talk-generated-names></talk-generated-names>`,
  },
  {
    nav: "the two jaws",
    talkOnly: true,
    title: "The trap has two jaws",
    body: `<p class="say talk-jaw">“That means the compilation is not reproducible.”<span class="talk-cite">sourcify #1054</span></p>
      <div class="reveal" data-at="1">
        <p class="say talk-jaw">25 compilations of a Pancake contract, all labelled as the Curve contract. None labelled Pancake.<span class="talk-cite">sourcify #2858</span></p>
        <p class="talk-sum">too sensitive (a path moves it), too blunt (two contracts collapse into one)</p>
      </div>`,
  },
  {
    nav: "choose the hash",
    talkOnly: true,
    title: "Choose what goes into the hash",
    lede: "You already content-address.",
    body: `<div class="identity-boundary identity-three">
        <div><b>the metadata hash</b></div>
        <div><b>the bytecode hash</b></div>
        <div><b>this morning's git commit</b></div>
      </div>
      <div class="reveal" data-at="1">
        <p class="talk-sum">both hash the wrong thing</p>
        <div class="identity-boundary">
          <div><span>the metadata hash</span><b>swallows the file path</b></div>
          <div><span>the bytecode hash</span><b>swallows optimizer accidents, drops source</b></div>
        </div>
      </div>`,
  },
  {
    nav: "a facet is the choice",
    talkOnly: true,
    title: "A facet is the choice",
    body: `<div class="identity-boundary">
        <div><span>keep every byte</span><b>a checksum</b><small>the choice failing us</small></div>
        <div><span>drop names, AST ids, offsets; keep structure and types</span><b>the two builds are one address</b></div>
      </div>
      <div class="reveal" data-at="1">
        <p class="talk-sum">merklize: every function, every subtree, its own address</p>
      </div>`,
  },
  {
    nav: "knowledge that rides",
    talkOnly: true,
    title: "Knowledge that rides",
    body: `<anchor-transport></anchor-transport>
      <div class="reveal" data-at="1"><div class="figure talk-mt"><div class="cap">prior art</div><table>
        <tr><td class="n">Unison</td><td>the hash is the definition</td></tr>
        <tr><td class="n">Nix</td><td>identity is the build plan</td></tr>
        <tr><td class="n">git</td><td></td></tr>
      </table></div></div>
      <div class="reveal" data-at="2"><p class="say">The spec holds: the Lean spec and the Rust engine agree byte for byte, 254 checks (<code>lake exe check</code>), and the transport theorem is proved (<code>transport_respects</code>).</p></div>`,
  },
  {
    nav: "the honest edge",
    talkOnly: true,
    title: "The honest edge",
    body: `<p class="talk-sum">A facet match is not semantic equivalence.</p>
      <p class="talk-sum">The address names the residual obligation: same up to this facet, here is what is left to prove.</p>`,
  },
  {
    nav: "the ask",
    talkOnly: true,
    title: "The ask",
    lede: "One per team.",
    body: `<div class="plain-ask">
        <div><span>solc / fe</span>expose provenance: origin edges through lowering</div>
        <div><span>Sourcify</span>a structural-fingerprint column beside the bytecode hash</div>
        <div><span>verifiers</span>pin the proof to the address, the facet as its scope</div>
      </div>`,
  },
  {
    nav: "closer",
    talkOnly: true,
    body: `<div class="talk-closer">A name tells you where somebody put a thing. An address tells you what the thing is.</div>`,
  },
  // --- the demos arc ("demos") ---------------------------------------------
  // Reframed slides for ?arc=demos only: the same live components the pool
  // already ships, recomposed spare. Each slide is the demo, an idea-stating
  // title, and at most one line: a verbatim tracker quote where one grounds
  // the demo (repo-level attribution only), a plain do-this line otherwise.
  // demosOnly keeps them out of every other arc's appendix, exactly as
  // talkOnly does for the talk deck. The recognition slide is the pool's
  // "recognized" chapter itself: recog-scan deep-links pin under #recognized,
  // so that nav must stay the chapter's.
  {
    nav: "audit once",
    demosOnly: true,
    title: "One audit, many contracts",
    lede: "Pick a shape to see every contract that carries it.",
    body: `<recog-twins></recog-twins>`,
  },
  {
    nav: "shared machinery",
    demosOnly: true,
    title: "Three authors, the same shapes",
    lede: "Hover a chip to light the same shape in all three libraries.",
    body: `<live-xref></live-xref>`,
  },
  {
    nav: "sameness",
    demosOnly: true,
    title: "Facets of similarity",
    lede: "Loosen the facet and watch which functions merge to one shape.",
    body: `<facet-primer></facet-primer>`,
  },
  {
    nav: "on music",
    demosOnly: true,
    title: "Facets of musical riffs",
    lede: "Press play on each variant, then choose what counts as the same.",
    body: `<riff-dial></riff-dial>`,
  },
  {
    nav: "names out",
    demosOnly: true,
    title: "Two builds, one address",
    lede: "“This results in the Yul identifiers to differ if AST IDs differ!” (solidity #14535)",
    body: `<held-address></held-address>`,
  },
  {
    nav: "flips vs holds",
    demosOnly: true,
    title: "The hash that flips, the shape that holds",
    lede: "“Since the metadata contains the name of the input file, the hash of it will differ.” (solidity #1644)",
    body: `<metadata-axes></metadata-axes>`,
  },
  {
    nav: "leaves up",
    demosOnly: true,
    title: "An address built from the leaves up",
    lede: "Step the fold, then drop a dimension and watch every address change.",
    body: `<fold-merkle></fold-merkle>`,
  },
  {
    nav: "the bug's shape",
    demosOnly: true,
    title: "The shape of a bug",
    lede: "“That means the compilation is not reproducible.” (sourcify #1054)",
    body: `<vuln-sniff></vuln-sniff>`,
  },
  {
    nav: "edited forks",
    demosOnly: true,
    title: "An edited fork still carries the shape",
    lede: "Click a fork that exact match and text search both miss.",
    body: `<fuzzy-scan></fuzzy-scan>`,
  },
  {
    nav: "a fact rides",
    demosOnly: true,
    title: "A fact that rides the address",
    lede: "Pick a fact, then broaden the anchor and watch where it rides wrong.",
    body: `<anchor-transport></anchor-transport>`,
  },
  {
    nav: "rides along",
    demosOnly: true,
    title: "Provenance rides along, the address holds still",
    lede: "Toggle the origin edges, then compare two lowerings.",
    body: `<provenance-rides></provenance-rides>`,
  },
];

// The URL hash deep-links the storybook: "#<chapter>" selects a chapter, and a
// chapter may own a sub-anchor after a slash ("#recognized/Contract.fn"). The
// chapter part is handled here; sub-anchors are left to the chapter's component.
const slugify = (s) => s.replace(/\s+/g, "-");

// CH above is a POOL, not an order. An arc is the order: a list of
// [section label, [chapter navs, in order]], and it only has to name the beats
// it wants. Whatever the arc leaves out is collected, in pool order, into a
// final appendix section, so a hand-written arc can be short without losing a
// chapter. Writing a new arc means adding one entry to ARCS; ?arc=<key> selects
// it. See demo/ARCS.md for the pool inventory and how to draft one.
const ARCS = {
  // The 20-minute talk, "A name you can check": a cold open, the receipts read
  // back in order, the turn, three live proofs plus the anchor payoff, then
  // the honest edge, the ask, and one closing line. The slides marked talkOnly
  // in the pool stage this arc's builds; everything else the pool holds lands
  // in the appendix for follow-up questions, exactly as before.
  talk: {
    label: "talk",
    sections: [
      ["the hook", ["cold open", "where knowledge lives"]],
      [
        "the receipts",
        [
          "names people choose",
          "edits that change nothing",
          "names nobody chose",
          "the two jaws",
        ],
      ],
      ["the turn", ["choose the hash", "a facet is the choice"]],
      [
        "proof",
        ["one address", "recognized", "sniff it out", "knowledge that rides"],
      ],
      ["the ask", ["the honest edge", "the ask", "closer"]],
    ],
  },
  // The naming arc, built around one escalating question: what's in a name?
  // A rename at six scales, then receipts at each scale from real solc runs
  // (the wire selector, the collision, the no-op edits, the id ledger), the
  // climax that ids inside a compiler are promised by nothing, and the payoff
  // computed live: the address the unrelated edit cannot reach. The earlier
  // ordering of this arc is in git before 2026-07-27.
  names: {
    label: "names",
    sections: [
      ["what's in a name", ["many forms", "a rename"]],
      ["names people choose", ["the selector", "one selector"]],
      [
        "names nobody chose",
        ["quiet edits", "broken reference", "generated names", "every name"],
      ],
      ["an address instead", ["an address", "the fold", "one address"]],
      ["what an address buys", ["facets", "recognized", "resolution"]],
      ["across compilers", ["two builds", "provenance", "two fingerprints"]],
      [
        "limits, and the ask",
        ["locality runs out", "what we sampled", "what we need"],
      ],
    ],
  },
  // Same spine, ten beats, for a short slot: one problem slide, one mechanism,
  // two payoffs, the compiler beat, the boundary, the ask.
  short: {
    label: "short",
    sections: [
      ["the problem", ["broken reference"]],
      ["the choice", ["facets"]],
      ["on real code", ["recognized", "sniff it out"]],
      ["through the compiler", ["provenance"]],
      ["from match to knowledge", ["prove it", "anchors"]],
      ["in honesty, and the ask", ["what we sampled", "what we need"]],
    ],
  },
  // Interactive demos only: every slide is a live component that fingerprints,
  // filters, or steps in this browser and answers hover/click/toggle with a
  // readout. No connective slides, no appendix: the arc ends where the demos
  // end, and the idea on each slide is in the interaction.
  demos: {
    label: "demos",
    appendix: false,
    sections: [
      ["on real code", ["recognized", "audit once", "shared machinery"]],
      [
        "what counts as the same",
        ["sameness", "on music", "names out", "flips vs holds", "leaves up"],
      ],
      [
        "put to work",
        ["the bug's shape", "edited forks", "a fact rides", "rides along"],
      ],
    ],
  },
  // The storybook order the deck grew up in: domain-coherent rather than causal,
  // useful for browsing everything that exists.
  storybook: {
    label: "storybook",
    sections: [
      ["the idea", ["facets", "address it"]],
      [
        "on music",
        ["the riff", "forte catalog", "interval vector", "three rungs"],
      ],
      ["on real code", ["recognized", "twins", "dedup"]],
      [
        "in the compiler",
        [
          "the compiler too",
          "sniff it out",
          "modified",
          "two fingerprints",
          "main vs meta",
        ],
      ],
      [
        "structure and meaning",
        [
          "structure vs meaning",
          "prove it",
          "seat filled",
          "anchors",
          "the cheap yes",
          "prior art",
          "the proofs check",
        ],
      ],
      [
        "the address up close",
        ["the fold", "provenance", "two instantiations", "locality runs out"],
      ],
      ["a building block", ["a shared block", "what we need"]],
      ["in honesty", ["the engine checks too", "what we sampled"]],
    ],
  },
  // The presentation arc for tonight: one idea per beat, facets as the spine,
  // the demos-trimmed variant of each shared component. No appendix: the arc
  // ends on the ask.
  tonight: {
    label: "tonight",
    appendix: false,
    sections: [
      ["", ["riff catalog"]],
      ["what counts as the same", ["sameness", "on music", "names out"]],
      ["on real code", ["recognized", "audit once", "shared machinery"]],
      [
        "through the compiler",
        ["the compiler too", "flips vs holds", "leaves up"],
      ],
      ["put to work", ["the bug's shape", "edited forks", "a fact rides"]],
      ["standing on", ["the cheap yes", "prior art"]],
      ["in honesty", ["locality runs out", "what we sampled"]],
      [
        "the invitation",
        ["where we're headed", "what we need", "a shared block"],
      ],
    ],
  },
};
const APPENDIX_LABEL = "for follow-up questions";
const requestedArc = new URLSearchParams(location.search).get("arc");
// A query string that lands after the hash, or a tab still running an older
// build, both read as "no arc requested" and would silently serve the default.
// Remember the miss so the header can say so out loud instead.
const ARC_MISSING =
  requestedArc && !Object.hasOwn(ARCS, requestedArc) ? requestedArc : null;
const ARC_KEY = Object.hasOwn(ARCS, requestedArc) ? requestedArc : "tonight";
const ARC = ARCS[ARC_KEY];
const SECTIONS = [];
{
  const pool = new Map(CH.map((c) => [c.nav, c]));
  const ordered = [];
  const place = (label, navs) => {
    if (!navs.length) return;
    SECTIONS.push([label, navs]);
    navs.forEach((nv, i) => {
      const c = pool.get(nv);
      if (!c)
        throw new Error(`arc "${ARC_KEY}" references unknown chapter: ${nv}`);
      c.sec = label;
      c.secHead = i === 0; // opens its section, so the nav draws a label before it
      ordered.push(c);
      pool.delete(nv);
    });
  };
  for (const [label, navs] of ARC.sections) place(label, navs);
  // Everything the arc did not name lands in the appendix, except chapters
  // that serve exactly one arc (talkOnly, demosOnly): letting them pad the
  // other arcs would also change those arcs' arrow-key behavior through their
  // reveal steps. An arc may opt out of the appendix entirely (appendix:
  // false); the demos arc does, so it ends where its demos end.
  if (ARC.appendix !== false)
    place(
      APPENDIX_LABEL,
      [...pool.values()]
        .filter((c) => !c.talkOnly && !c.demosOnly && !c.tonightOnly)
        .map((c) => c.nav),
    );
  CH.length = 0;
  CH.push(...ordered);
}

customElements.define(
  "tour-app",
  class extends HTMLElement {
    connectedCallback() {
      // The active arc, readable from CSS: the demos arc uses this to trim the
      // slide chrome (kickers, long in-component captions) it composes without.
      document.body.classList.add("arc-" + ARC_KEY);
      // Relative hrefs, so this works at a subpath and from a local server alike.
      // No hash, so switching arcs lands on that arc's first chapter.
      document.getElementById("arcs").innerHTML =
        Object.entries(ARCS)
          .map(
            ([k, a]) =>
              `<a href="?arc=${encodeURIComponent(k)}" aria-current="${k === ARC_KEY ? "page" : "false"}">${a.label}</a>`,
          )
          .join("") +
        (ARC_MISSING
          ? `<em>no arc named "${ARC_MISSING}", showing ${ARC.label}</em>`
          : "");
      document.querySelector(".crumb").textContent = `riffcat · ${ARC.label}`;
      const details = document.getElementById("details");
      const setDetails = (on) => {
        document.body.classList.toggle("details", on);
        details.setAttribute("aria-pressed", on ? "true" : "false");
        details.textContent = `details: ${on ? "on" : "off"}`;
      };
      details.addEventListener("click", () =>
        setDetails(!document.body.classList.contains("details")),
      );
      const nav = document.getElementById("nav");
      nav.innerHTML = CH.map(
        (c, k) =>
          (c.secHead ? `<span class="navsec">${c.sec}</span>` : "") +
          `<button data-k="${k}">${k === 0 ? "·" : k}. ${c.nav}</button>`,
      ).join("");
      nav
        .querySelectorAll("button")
        .forEach((b) =>
          b.addEventListener("click", () => this.go(+b.dataset.k)),
        );
      document.addEventListener("keydown", (e) => {
        if (e.target.closest("live-dial") || e.target.closest("recog-scan"))
          return; // let those keep focus
        if (e.key === "ArrowRight") this.next();
        if (e.key === "ArrowLeft") this.prev();
        if (e.key.toLowerCase() === "d")
          setDetails(!document.body.classList.contains("details"));
      });
      this.i = this.chapterFromHash();
      addEventListener("hashchange", () => {
        const k = this.chapterFromHash();
        if (k !== this.i) this.go(k, true); // chapter changed via hash; do not rewrite it
      });
      this.render();
    }
    chapterFromHash() {
      const seg = decodeURIComponent(
        (location.hash || "").replace(/^#/, ""),
      ).split("/")[0];
      const k = CH.findIndex((c) => slugify(c.nav) === seg);
      return k >= 0 ? k : 0;
    }
    go(k, fromHash) {
      if (k < 0 || k >= CH.length) return;
      this.i = k;
      this.render();
      if (!fromHash) {
        const h = "#" + slugify(CH[k].nav); // navigating chapters clears any sub-anchor
        if (location.hash !== h) location.hash = h;
      }
    }
    // --- sub-slide reveals ---------------------------------------------------
    // Opt-in build steps within one chapter: the body may mark elements with
    // class "reveal" and data-at="1" (2, 3, ...). They start hidden (CSS in
    // index.html). ArrowRight walks this.step up to the largest data-at in the
    // stage before leaving the chapter; ArrowLeft walks it back down before
    // leaving. Visibility is applied by toggling .shown only, never by
    // re-rendering, so live components in the stage survive a reveal untouched.
    // A chapter with no .reveal elements has maxStep 0 and the arrows behave
    // exactly as before. The reveal set is queried live, so a component that
    // renders its reveals inside connectedCallback is picked up too.
    reveals() {
      return this.querySelectorAll(".stage .reveal[data-at]");
    }
    maxStep() {
      let m = 0;
      this.reveals().forEach((el) => {
        m = Math.max(m, +el.dataset.at || 0);
      });
      return m;
    }
    applyStep() {
      this.reveals().forEach((el) =>
        el.classList.toggle("shown", (+el.dataset.at || 0) <= this.step),
      );
    }
    next() {
      if (this.step < this.maxStep()) {
        this.step += 1;
        this.applyStep();
      } else this.go(this.i + 1);
    }
    prev() {
      if (this.step > 0) {
        this.step -= 1;
        this.applyStep();
      } else this.go(this.i - 1);
    }
    render() {
      const c = CH[this.i];
      this.step = 0; // every chapter entry starts before its first reveal
      document
        .querySelectorAll("#nav button")
        .forEach((b, k) =>
          b.setAttribute("aria-current", k === this.i ? "true" : "false"),
        );
      // Pager rides above the stage so the only thing below the content is the
      // live panel: the page then grows and shrinks at the bottom, anchored from
      // the top, and scroll-anchoring holds the chips while a panel resizes.
      this.innerHTML = `
      <div class="pager">
        <button data-d="-1" ${this.i === 0 ? "disabled" : ""}>← prev</button>
        <span class="count">${this.i + 1} / ${CH.length} · ${c.nav}</span>
        <button data-d="1" ${this.i === CH.length - 1 ? "disabled" : ""}>next →</button>
      </div>
      <div class="stage">
        ${c.kicker ? `<div class="kicker">${c.kicker}</div>` : ""}
        ${c.title ? `<h1>${c.title}</h1>` : ""}
        ${c.lede ? `<p class="lede">${c.lede}</p>` : ""}
        ${c.body}
      </div>`;
      this.querySelectorAll(".pager button").forEach((b) =>
        b.addEventListener("click", () => this.go(this.i + +b.dataset.d)),
      );
      window.scrollTo({ top: 0, behavior: "smooth" });
    }
  },
);

// The name a compiler generates belongs to the file, not to the function. Both
// builds contain the same Solidity `withdraw`, byte for byte; the second adds
// one unrelated function above it, and solc numbers the generated Yul function
// from a parse-order node id, so the id moves (as do the ids inside the body,
// which "every name" itemizes). Generated by tools/gen-namesdata.mjs, which
// fails loudly rather than emitting a slide that is no longer true.
customElements.define(
  "generated-names",
  class extends HTMLElement {
    connectedCallback() {
      const d = window.RIFFCAT_NAMES;
      if (!d) {
        this.innerHTML = `<p class="live-note bad">name data not loaded</p>`;
        return;
      }
      const rows = d.builds
        .map(
          (b, i) =>
            `<div class="gnrow"><span class="gnwhere">${i === 0 ? "as written" : "the same file, plus <code>version()</code> above"}</span>` +
            `<code class="gnname">${b.name.replace(/_(\d+)$/, "_<b>$1</b>")}</code></div>`,
        )
        .join("");
      this.innerHTML = `<pre class="gnsrc">${solHi(d.tracked)}</pre>
      <div class="gnames">${rows}</div>
      <div class="eqread">One function, unchanged. Two builds, two names.</div>`;
    }
  },
);

// --- the name ladder ------------------------------------------------------
// The "what's in a name" evidence chapters. Everything they show is baked in
// nameladder.js by tools/gen-nameladder.mjs from real solc runs; the generator
// throws rather than write a slide that stopped being true. The one live
// chapter (held-address) additionally recomputes its addresses in this browser
// and refuses to render if the engine disagrees with the slide.
const NL = () => window.RIFFCAT_NAMELADDER;
const NL_MISSING = `<p class="live-note bad">name-ladder data not loaded</p>`;
const escText = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;");

// Rename the function, keep the body: the ABI selector is a hash of the name,
// so the four bytes on the wire move with it.
customElements.define(
  "sel-rename",
  class extends HTMLElement {
    connectedCallback() {
      const d = NL();
      if (!d) {
        this.innerHTML = NL_MISSING;
        return;
      }
      const rows = d.selector.rows
        .map(
          (r, i) =>
            `<div class="gnrow"><span class="gnwhere">${r.label}</span><code>${r.sig}</code>` +
            `<code class="gnname">${i === 0 ? `<b>${r.sel}</b>` : `<i class="moved">${r.sel}</i>`}</code></div>`,
        )
        .join("");
      this.innerHTML = `<div class="gnames">${rows}</div>
      <div class="eqread">Same arguments, same body. Every caller holding <b>${d.selector.rows[0].sel}</b> now misses.</div>`;
    }
  },
);

// Two unrelated signatures, one selector, and solc's own refusal, verbatim.
customElements.define(
  "sel-collide",
  class extends HTMLElement {
    connectedCallback() {
      const d = NL();
      if (!d) {
        this.innerHTML = NL_MISSING;
        return;
      }
      const rows = d.collision.rows
        .map(
          (r) =>
            `<div class="gnrow"><span class="gnwhere"><code>${r.sig}</code></span>` +
            `<code class="gnname"><b>${r.sel}</b></code></div>`,
        )
        .join("");
      this.innerHTML = `<div class="gnames">${rows}</div>
      <pre class="cmd">${escText(d.collision.error)}</pre>
      <div class="eqread">The dispatcher reads only the four bytes, so it cannot tell these two apart.</div>`;
    }
  },
);

// Three edits that change no executable byte; the appended metadata hash, the
// artifact's identity for exact-match verification, moves every time.
customElements.define(
  "quiet-edits",
  class extends HTMLElement {
    connectedCallback() {
      const d = NL();
      if (!d) {
        this.innerHTML = NL_MISSING;
        return;
      }
      const rows = d.quiet.rows
        .map(
          (r, i) =>
            `<tr><td class="n">${r.label}</td>` +
            `<td>${i === 0 ? `${d.quiet.codeBytes} bytes` : `the same ${d.quiet.codeBytes} bytes`}</td>` +
            `<td class="fp${i === 0 ? "" : " moved"}">${r.meta}…</td></tr>`,
        )
        .join("");
      this.innerHTML = `<div class="figure"><div class="cap">one contract, four builds</div><table>
      <tr><th>the edit</th><th>the code it deploys</th><th>the hash solc appends</th></tr>${rows}</table></div>
      <div class="eqread">Zero code bytes moved. The identity hash moved all three times.</div>`;
    }
  },
);

// Every name the toolchain hangs on one function, across the same two builds
// as "generated names", and which of them one unrelated edit moved.
customElements.define(
  "name-ledger",
  class extends HTMLElement {
    connectedCallback() {
      const d = NL();
      if (!d) {
        this.innerHTML = NL_MISSING;
        return;
      }
      const rows = d.ledger.rows
        .map(
          (r) =>
            `<tr><td class="n">${r.what}</td><td><code>${r.a}</code></td><td><code>${r.b}</code></td>` +
            `<td class="lx-verd ${r.held ? "ok" : "bad"}">${r.held ? "held" : "moved"}</td></tr>`,
        )
        .join("");
      const moved = d.ledger.rows.filter((r) => !r.held).length;
      this.innerHTML = `<div class="figure"><div class="cap">one function, two builds</div><table>
      <tr><th></th><th>as written</th><th>one function added above</th><th></th></tr>${rows}</table></div>
      <div class="eqread"><b>${moved} of ${d.ledger.rows.length}</b> moved. The two that held are the two a person chose.</div>`;
    }
  },
);

// The payoff, computed as you watch: the two builds' fun_withdraw subtrees
// (extracted verbatim from solc's irAst) are fingerprinted by the engine in
// this browser. Names kept in the address: the unrelated edit moved it. Names
// left out: one address. If the engine ever disagrees, the slide says so
// instead of rendering.
customElements.define(
  "held-address",
  class extends HTMLElement {
    async connectedCallback() {
      this.keep = true;
      this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
      try {
        const b = await engineReady;
        const d = NL();
        if (!d) throw new Error("name-ladder data not loaded");
        const t0 = performance.now();
        const fp = (o) =>
          JSON.parse(b.fingerprint_yul(JSON.stringify(o), "shape")).find((u) =>
            /^fun_withdraw_\d+$/.test(u.name),
          );
        this.units = [fp(d.address.a), fp(d.address.b)];
        this.ms = performance.now() - t0;
        if (
          this.units.some((u) => !u) ||
          this.units[0].facets["names-blind"] !==
            this.units[1].facets["names-blind"] ||
          this.units[0].facets["full"] === this.units[1].facets["full"]
        )
          throw new Error(
            "the engine disagrees with this slide; regenerate nameladder.js",
          );
        this.render();
      } catch (e) {
        this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`;
      }
    }
    render() {
      const facet = this.keep ? "full" : "names-blind";
      const rows = this.units
        .map(
          (u, i) =>
            `<div class="gnrow"><span class="gnwhere">${i === 0 ? "as written" : "one function added above"}` +
            ` · <code>${u.name}</code></span><code class="gnname">${
              this.keep
                ? `<i class="moved">${short(u.facets[facet])}</i>`
                : `<b>${short(u.facets[facet])}</b>`
            }</code></div>`,
        )
        .join("");
      const d = NL();
      const read = this.keep
        ? `Two addresses. ${d.irlines.moved} of ${d.irlines.total} lines of the untouched function's IR differ` +
          ` between the builds, every difference a generated name or offset, and this address keeps names in.`
        : `<b>One address.</b> The numbering is out; what is left is everything else about the function,` +
          ` and the unrelated edit cannot reach any of it. ${this.ms.toFixed(0)} ms in your browser.`;
      this.innerHTML = `
      <div class="dialbar"><div class="grp"><span>facet</span>
        <button data-k="1" aria-pressed="${this.keep}">names in</button>
        <button data-k="0" aria-pressed="${!this.keep}">names out</button>
      </div></div>
      <div class="gnames">${rows}</div>
      <div class="eqread">${read}</div>`;
      this.querySelectorAll("[data-k]").forEach((btn) =>
        btn.addEventListener("click", () => {
          const keep = btn.dataset.k === "1";
          if (keep !== this.keep) {
            this.keep = keep;
            this.render();
          }
        }),
      );
    }
  },
);

// --- the talk deck's staging components -----------------------------------
// Slides for ?arc=talk. The evidence components read every solc-derived value
// out of nameladder.js at render time, the same source the name-ladder
// chapters above use; nothing here types a number by hand. Sub-slide builds
// are .reveal[data-at] blocks driven by tour-app's arrow keys.

// The cold open: a staged enactment on clearly-illustrative synthetic code,
// never presented as a real audit. A review note pins a real bug to line 118.
// Reveal 1 drops two imports in at the top of the file: the code shifts down
// two lines, gutter line 118 is now blank, and the note points at nothing.
// Reveal 2 is the title card.
const CO_BEFORE = [
  [1, `pragma solidity ^0.8.24;`],
  [2, `import "./IVault.sol";`],
  null, // fold
  [110, `        balances[to] += amount;`],
  [111, `    }`],
  [112, ``],
  [113, `    function sweep(address to, uint256 amount) external {`],
  [114, `        // residual funds may only be swept by the owner`],
  [115, `        uint256 fee = (amount * FEE_BPS) / 10_000;`],
  [116, ``],
  [117, `        // the owner check`],
  [118, `        require(msg.sender != owner, "unauthorized");`, "hl"],
  [119, `        _payout(to, amount - fee);`],
  [120, `    }`],
];
const CO_IMPORTS = [
  `import "./SafeCast.sol";`,
  `import "./ReentrancyGuard.sol";`,
];
function coPanel(lines, cls) {
  const rows = lines
    .map((l) =>
      l === null
        ? `<div class="co-line co-fold"><span class="co-n">⋮</span><span class="co-c"></span></div>`
        : `<div class="co-line${l[2] ? ` co-${l[2]}` : ""}"><span class="co-n">${l[0]}</span><span class="co-c">${solHi(l[1])}</span></div>`,
    )
    .join("");
  return `<div class="co-code ${cls}">${rows}</div>`;
}
customElements.define(
  "talk-cold-open",
  class extends HTMLElement {
    connectedCallback() {
      // The after state is derived, not hand-copied: the two imports go in at
      // lines 3 and 4, every later line moves down two, and whatever now sits
      // at gutter line 118 (the blank old line 116) inherits the highlight.
      const after = [
        CO_BEFORE[0],
        CO_BEFORE[1],
        [3, CO_IMPORTS[0], "add"],
        [4, CO_IMPORTS[1], "add"],
        null,
        ...CO_BEFORE.slice(3).map(([n, src]) => [
          n + 2,
          src,
          n + 2 === 118 ? "hl" : "",
        ]),
      ];
      this.innerHTML = `
      <div class="co-wrap">
        ${coPanel(CO_BEFORE, "co-before")}
        <div class="reveal" data-at="1">${coPanel(after, "co-after")}</div>
        <aside class="co-note"><span class="co-note-k">review note</span>owner check on line 118 is inverted</aside>
      </div>
      <div class="reveal" data-at="2"><div class="talk-title">A name you can check</div></div>
      <p class="twnote">A staged enactment on illustrative code, not a real audit.</p>`;
    }
  },
);

// Names people choose: rename withdraw and the four wire bytes move. Reveal 1:
// two unrelated signatures share one selector. Reveal 2: solc's own refusal,
// verbatim. All of it from nameladder.js.
customElements.define(
  "talk-names-chosen",
  class extends HTMLElement {
    connectedCallback() {
      const d = NL();
      if (!d) {
        this.innerHTML = NL_MISSING;
        return;
      }
      const ren = d.selector.rows
        .map(
          (r, i) =>
            `<div class="gnrow"><span class="gnwhere">${r.label}</span><code>${r.sig}</code>` +
            `<code class="gnname">${i === 0 ? `<b>${r.sel}</b>` : `<i class="moved">${r.sel}</i>`}</code></div>`,
        )
        .join("");
      const col = d.collision.rows
        .map(
          (r) =>
            `<div class="gnrow"><span class="gnwhere"><code>${r.sig}</code></span>` +
            `<code class="gnname"><b>${r.sel}</b></code></div>`,
        )
        .join("");
      this.innerHTML = `<div class="gnames">${ren}</div>
      <div class="reveal" data-at="1"><div class="gnames talk-mt">${col}</div></div>
      <div class="reveal" data-at="2"><pre class="cmd">${escText(d.collision.error)}</pre></div>`;
    }
  },
);

// Edits that change nothing: one contract, three edits. Reveal 1: the code
// bytes identical all three times, four different metadata hashes, and the
// tracker's own explanation.
customElements.define(
  "talk-quiet-edits",
  class extends HTMLElement {
    connectedCallback() {
      const d = NL();
      if (!d) {
        this.innerHTML = NL_MISSING;
        return;
      }
      const edits = d.quiet.rows
        .slice(1)
        .map((r) => `<tr><td class="n">${r.label}</td></tr>`)
        .join("");
      const builds = d.quiet.rows
        .map(
          (r, i) =>
            `<tr><td class="n">${r.label}</td>` +
            `<td>${i === 0 ? `${d.quiet.codeBytes} bytes` : `the same ${d.quiet.codeBytes} bytes`}</td>` +
            `<td class="fp${i === 0 ? "" : " moved"}">${r.meta}…</td></tr>`,
        )
        .join("");
      this.innerHTML = `
      <div class="figure"><div class="cap">one contract, ${d.quiet.codeBytes} bytes of code, three edits</div><table>${edits}</table></div>
      <div class="reveal" data-at="1">
        <div class="figure talk-mt"><div class="cap">four builds</div><table>
          <tr><th>the edit</th><th>the code it deploys</th><th>the hash solc appends</th></tr>${builds}</table></div>
        <p class="say">“Since the metadata contains the name of the input file, the hash of it will differ.”<span class="talk-cite">solidity #1644</span></p>
      </div>`;
    }
  },
);

// Generated names: the unrelated edit above the function moves every id the
// toolchain generated for it, with the root cause in the tracker's words.
// Reveal 1: the bytecode consequence, same tracker.
customElements.define(
  "talk-generated-names",
  class extends HTMLElement {
    connectedCallback() {
      const d = NL();
      if (!d) {
        this.innerHTML = NL_MISSING;
        return;
      }
      const rows = d.ledger.rows
        .filter((r) => /AST|Yul/.test(r.what))
        .map(
          (r) =>
            `<tr><td class="n">${r.what}</td><td class="fp">${r.a}</td><td class="fp moved">${r.b}</td></tr>`,
        )
        .join("");
      this.innerHTML = `
      <div class="figure"><div class="cap">one function, two builds</div><table>
        <tr><th></th><th>as written</th><th>one function added above</th></tr>${rows}</table></div>
      <p class="talk-count"><b>${d.irlines.moved} of ${d.irlines.total}</b> IR lines of the untouched function moved.</p>
      <p class="say">“the compiler may assign different AST-IDs in the presence of additional source files ... This results in the Yul identifiers to differ if AST IDs differ”<span class="talk-cite">solidity #14535</span></p>
      <div class="reveal" data-at="1">
        <p class="say">“Bytecodes are expected to be equal, however they have a pretty big diff”<span class="talk-cite">solidity #14829, adding an empty <code>contract DummyContract {}</code></span></p>
      </div>`;
    }
  },
);

// The definition slide. One real address, taken from the fixtures so the figure
// is not decorated with an invented digest, and the check spelled out: you can
// run the hash yourself, so the reference does not require trusting the sender.
customElements.define(
  "address-check",
  class extends HTMLElement {
    async connectedCallback() {
      this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
      try {
        const b = await engineReady;
        const u = yulUnits(b, "oz-muldiv", "shape").units.find((x) =>
          /^fun_appId_/.test(x.name),
        );
        this.render(short(u.facets["full"]));
      } catch (e) {
        this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`;
      }
    }
    render(addr) {
      this.innerHTML = `<div class="addrcheck">
        <div><span>you ask for</span><code>${addr}</code></div>
        <div class="addrarrow">→</div>
        <div><span>you receive</span><b>the content</b></div>
        <div class="addrarrow">→</div>
        <div class="addrok"><span>you hash it yourself</span><code>${addr}</code></div>
      </div>
`;
    }
  },
);

// Drive the dial: fingerprint the baked Demo contract in-browser, render every
// function as a chip colored by its fingerprint, and let the reader turn the
// dial. The facet ladder doubles as the selector and shows the class count at
// each stop, so the visual collapse and the number move together.
const FACETS = ["full", "names-blind", "structure"];

customElements.define(
  "live-dial",
  class extends HTMLElement {
    async connectedCallback() {
      this.mode = "shape";
      this.facet = "names-blind";
      this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
      try {
        this.bindings = await engineReady;
        if (!this.bindings.fixture("demo"))
          throw new Error("demo fixture missing");
        this.render();
      } catch (e) {
        this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`;
      }
    }
    unitsFor(mode) {
      const r = yulUnits(this.bindings, "demo", mode); // session-cached; this._ms is the real first-compute time
      this._ms = r.ms;
      return r.units;
    }
    render() {
      const funcs = this.unitsFor(this.mode).slice(1);
      const count = (f) => new Set(funcs.map((u) => u.facets[f])).size;
      const ladder = FACETS.map(
        (f) =>
          `<span class="stop ${f === this.facet ? "on" : ""}" data-facet="${f}"><b>${count(f)}</b> ${f}</span>`,
      ).join("");
      const syms = symMapFor(funcs.map((u) => u.facets[this.facet]));
      const symByEq = new Map(
        [...syms].map(([d, g]) => [eqKey(d).slice(3), g]),
      );
      const grid = funcs.map((u) => chip(u, this.facet, null, syms)).join("");
      const k = count(this.facet);
      const idle =
        `${funcs.length} functions · <b>${k}</b> classes at ${this.facet} · ${this.mode} ·` +
        ` ${this.mode === "shape" ? "loosen the facet and the colors merge" : "identity pins every artifact, so nothing merges"}` +
        ` · ${this._ms.toFixed(0)} ms in your browser`;
      this.innerHTML = `
      <div class="dialbar">
        <div class="grp"><span>mode</span>
          <button data-mode="shape" aria-pressed="${this.mode === "shape"}">shape</button>
          <button data-mode="identity" aria-pressed="${this.mode === "identity"}">identity</button>
        </div>
        <div class="grp"><span>facet</span><div class="ladder">${ladder}</div></div>
      </div>
      <div class="eqgrid">${grid}</div>
      <div class="eqread" data-idle="${idle}">${idle}</div>`;
      this.querySelectorAll("[data-mode]").forEach((b) =>
        b.addEventListener("click", () => {
          if (this.mode !== b.dataset.mode) {
            this.mode = b.dataset.mode;
            this.render();
          }
        }),
      );
      this.querySelectorAll("[data-facet]").forEach((b) =>
        b.addEventListener("click", () => {
          if (this.facet !== b.dataset.facet) {
            this.facet = b.dataset.facet;
            this.render();
          }
        }),
      );
      wireHighlight(
        this,
        (c, lit) =>
          `<span class="sw" style="color:${chipColor(c.dataset.eq.slice(3))};background:none">${symByEq.get(c.dataset.eq.slice(3))}</span>` +
          `${c.dataset.name} · <span style="color:var(--warm)">${c.dataset.fp}</span>` +
          dimStrip(c, lit),
      );
    }
  },
);

// Library cross-reference: one wrapper per library, every emitted function as a
// chip at names-blind. Because color and class come from the fingerprint, the
// machinery the three libraries share lands on the same color in every row, and
// hovering it lights all three; each library's mul·div is its own island.
const XREF = [
  { id: "oz-muldiv", lib: "OpenZeppelin", short: "OZ" },
  { id: "solady-muldiv", lib: "Solady", short: "Solady" },
  { id: "solmate-muldiv", lib: "Solmate", short: "Solmate" },
];

customElements.define(
  "live-xref",
  class extends HTMLElement {
    async connectedCallback() {
      this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
      try {
        const b = await engineReady;
        const F = "names-blind";
        // If the three libraries are not warm yet, say so before the (one-time,
        // synchronous) engine work and let that note paint first, so a cold visit
        // reads as "computing" rather than a frozen tab. A warm visit skips this
        // and renders straight from cache.
        if (!XREF.every((x) => yulCache.has(x.id + ":shape"))) {
          this.innerHTML = `<p class="live-note">fingerprinting three libraries in your browser…</p>`;
          await new Promise((r) =>
            requestAnimationFrame(() => requestAnimationFrame(r)),
          );
        }
        let ms = 0;
        const rows = XREF.map((x) => {
          const r = yulUnits(b, x.id, "shape");
          ms += r.ms;
          return { x, funcs: r.units.slice(1) };
        });
        ms = ms.toFixed(0);
        // how many distinct chunks are shared across all three libraries
        const spread = new Map();
        for (const { x, funcs } of rows)
          for (const u of funcs) {
            const d = u.facets[F];
            if (!spread.has(d)) spread.set(d, new Set());
            spread.get(d).add(x.short);
          }
        const all3 = [...spread.values()].filter((s) => s.size === 3).length;
        const syms = symMapFor(
          rows.flatMap(({ funcs }) => funcs.map((u) => u.facets[F])),
        );
        const symByEq = new Map(
          [...syms].map(([d, g]) => [eqKey(d).slice(3), g]),
        );
        const gridRows = rows
          .map(
            ({ x, funcs }) =>
              `<div class="eqrow"><div class="libname"><b>${x.short}</b>${funcs.length} fns</div>` +
              `<div class="eqgrid" data-lib="${x.short}">${funcs.map((u) => chip(u, F, x.short, syms)).join("")}</div></div>`,
          )
          .join("");
        const idle =
          `${spread.size} distinct chunks · <b>${all3}</b> shared across all three libraries` +
          ` · hover one to trace it · ${ms} ms in your browser`;
        this.innerHTML = `${gridRows}<div class="eqread" data-idle="${idle}">${idle}</div>`;
        wireHighlight(this, (c, lit) => {
          const libs = new Set([...lit].map((x) => x.dataset.lib));
          const where =
            libs.size === 3
              ? "shared across <b>all three</b> libraries"
              : libs.size === 2
                ? `shared across <b>two</b> (${[...libs].join(", ")})`
                : `<b>only</b> in ${[...libs][0]}`;
          return (
            `<span class="sw" style="color:${chipColor(c.dataset.eq.slice(3))};background:none">${symByEq.get(c.dataset.eq.slice(3))}</span>` +
            `${c.dataset.name} · <span style="color:var(--warm)">${c.dataset.fp}</span> · ${where}` +
            dimStrip(c, lit)
          );
        });
      } catch (e) {
        this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`;
      }
    }
  },
);

// Recognition: real verified contracts (baked in realdata.js) scanned against
// the committed std-lib catalog. Each function is colored by the library it was
// recognized as (or grey if novel app code); hovering lights the same shape
// across every contract and shows the actual Solidity in a code panel. The
// recognition is precomputed (fingerprint + catalog lookup), so this view needs
// no engine at runtime.
// keyed by the catalog's lowercase library id; n = display name, h = hue.
// Each library carries a distinct shape (sym) as well as a hue, so the coding is
// legible to colorblind viewers: the symbol, not just the color, tells them apart.
const LIB = {
  openzeppelin: { n: "OpenZeppelin", h: 210, sym: "●" },
  solady: { n: "Solady", h: 145, sym: "▲" },
  solmate: { n: "Solmate", h: 32, sym: "■" },
};

// Minimal, safe Solidity highlighter: tokenize comments/strings out first, then
// color keywords and value types only inside code segments (no nested mangling).
function solHi(src) {
  const esc = (t) =>
    t.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const KW =
    /\b(function|returns?|memory|storage|calldata|public|private|internal|external|view|pure|payable|require|revert|assert|if|else|for|while|do|mapping|struct|enum|event|emit|new|delete|unchecked|assembly|modifier|constructor|using|override|virtual|import|pragma|contract|library|interface|is|abstract|immutable|constant|try|catch)\b/g;
  const TY = /\b(address|uint\d*|int\d*|bool|bytes\d*|string)\b/g;
  const code = (t) =>
    esc(t)
      .replace(KW, '<span class="c-kw">$1</span>')
      .replace(TY, '<span class="c-ty">$1</span>');
  const re =
    /(\/\/[^\n]*|\/\*[\s\S]*?\*\/)|("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')/g;
  let out = "",
    last = 0,
    m;
  while ((m = re.exec(src))) {
    out += code(src.slice(last, m.index));
    out += m[1]
      ? `<span class="c-com">${esc(m[1])}</span>`
      : `<span class="c-str">${esc(m[2])}</span>`;
    last = re.lastIndex;
  }
  return out + code(src.slice(last));
}

// Facet primer: five tiny, fully-readable functions, fingerprinted live at the
// source level (sol-fn). Slide the facet and watch which functions collapse to
// the same shape and which never do. This is the vocabulary every later chapter
// leans on, taught on code you can read in one glance. Data+AST in facetdemo.js.
const FACET_GLOSS = {
  full: "every dimension counts",
  "names-blind": "names dropped; constants and types still count",
  structure: "shape only; names, constants, and types all dropped",
};
const FACET_LABEL = {
  full: "keep everything",
  "names-blind": "ignore names",
  structure: "shape only",
};
const feq = (d) => "fe-" + d.slice(0, 12);
customElements.define(
  "facet-primer",
  class extends HTMLElement {
    async connectedCallback() {
      this.facet = "full";
      const data = window.RIFFCAT_FACET;
      if (!data) {
        this.innerHTML = `<p class="live-note bad">primer data not loaded</p>`;
        return;
      }
      this.fns = data.fns;
      this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
      try {
        const b = await engineReady;
        const t0 = performance.now();
        const units = JSON.parse(
          b.fingerprint_source(data.ast, "shape"),
        ).filter((u) => u.unit === "sol-fn");
        this._ms = performance.now() - t0;
        this.byName = {};
        for (const u of units) this.byName[u.name.split(".").pop()] = u;
        this.render();
      } catch (e) {
        this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`;
      }
    }
    facetCount(f) {
      return new Set(this.fns.map((x) => this.byName[x.name].facets[f])).size;
    }
    render() {
      const FACETS = ["full", "names-blind", "structure"];
      const seen = new Map(),
        order = []; // digest -> [names], in display order
      for (const f of this.fns) {
        const d = this.byName[f.name].facets[this.facet];
        if (!seen.has(d)) {
          seen.set(d, []);
          order.push(d);
        }
        seen.get(d).push(f.name);
      }
      const syms = symMapFor(order);
      // Per-view palette: evenly spaced hues by group order, so a handful of shapes
      // never land on near-identical hashed hues. Keyed by digest, so twins match.
      const cols = new Map(
        order.map((d, i) => [
          d,
          `oklch(72% 0.15 ${Math.round((i * 360) / Math.max(1, order.length))})`,
        ]),
      );
      const cards = this.fns
        .map((f) => {
          const d = this.byName[f.name].facets[this.facet],
            col = cols.get(d);
          return (
            `<div class="fcard ${feq(d)}" data-eq="${feq(d)}" data-nm="${f.name}" style="--chip:${col}">` +
            `<div class="fcard-h"><span class="sw" style="color:${col};background:none">${syms.get(d)}</span>${f.name}</div>` +
            `<pre class="fcode">${solHi(f.src)}</pre></div>`
          );
        })
        .join("");
      const ladder = FACETS.map(
        (f) =>
          `<span class="stop ${f === this.facet ? "on" : ""}" data-facet="${f}"><b>${this.facetCount(f)}</b> ${FACET_LABEL[f]}</span>`,
      ).join("");
      const groupTxt = order
        .map((d) => {
          const n = seen.get(d);
          return n.length > 1 ? `<b>${n.join(" = ")}</b>` : n[0];
        })
        .join(" · ");
      const idle =
        `<b>${seen.size}</b> shape${seen.size === 1 ? "" : "s"} at ${this.facet} · ${FACET_GLOSS[this.facet]}` +
        ` · ${groupTxt} · ${this._ms.toFixed(0)} ms in your browser`;
      this.innerHTML = `
      <div class="dialbar"><div class="grp"><span>facet</span><div class="ladder">${ladder}</div></div></div>
      <div class="fgrid">${cards}</div>
      <div class="eqread" data-idle="${idle}">${idle}</div>`;
      this.querySelectorAll("[data-facet]").forEach((s) =>
        s.addEventListener("click", () => {
          if (this.facet !== s.dataset.facet) {
            this.facet = s.dataset.facet;
            this.render();
          }
        }),
      );
      this.querySelectorAll(".fcard").forEach((card) => {
        card.addEventListener("mouseenter", () => {
          const twins = this.querySelectorAll("." + card.dataset.eq);
          this.querySelectorAll(".fcard").forEach((x) =>
            x.classList.add("dim"),
          );
          twins.forEach((x) => {
            x.classList.remove("dim");
            x.classList.add("twin");
          });
          const read = this.querySelector(".eqread");
          if (read)
            read.innerHTML =
              twins.length > 1
                ? `<b>${twins.length}</b> match when we ${FACET_LABEL[this.facet]}: <b>${[...twins].map((x) => x.dataset.nm).join(" = ")}</b>`
                : `<b>${card.dataset.nm}</b> has no match here`;
        });
        card.addEventListener("mouseleave", () => {
          this.querySelectorAll(".fcard").forEach((x) =>
            x.classList.remove("dim", "twin"),
          );
          const read = this.querySelector(".eqread");
          if (read) read.innerHTML = read.dataset.idle || "";
        });
      });
    }
  },
);

const RECOG_HINT =
  "Hover to inspect the source. Click to keep one match selected.";
customElements.define(
  "recog-scan",
  class extends HTMLElement {
    connectedCallback() {
      const data = window.RIFFCAT_REAL;
      if (!data || !data.contracts) {
        this.innerHTML = `<p class="live-note bad">recognition data not loaded</p>`;
        return;
      }
      this.contracts = data.contracts;
      this.spread = new Map(); // nb12 -> Set(contract index)
      this.contracts.forEach((c, ci) =>
        c.fns.forEach((f) => {
          if (!this.spread.has(f.nb)) this.spread.set(f.nb, new Set());
          this.spread.get(f.nb).add(ci);
        }),
      );
      this.onHash = () => this.selectByHash(true);
      addEventListener("hashchange", this.onHash);
      this.render();
      this.selectByHash(false); // honor a deep-link anchor on load
    }
    disconnectedCallback() {
      removeEventListener("hashchange", this.onHash);
    }
    render() {
      const rows = this.contracts
        .map((c, ci) => {
          const rec = c.fns.filter((f) => f.lib).length;
          const chips = c.fns
            .map((f, fi) => {
              const cls = f.lib ? "chip rec" : "chip novel";
              const hue =
                f.lib && LIB[f.lib]
                  ? `style="--chip:oklch(72% 0.15 ${LIB[f.lib].h})"`
                  : "";
              const sym = f.lib && LIB[f.lib] ? LIB[f.lib].sym + " " : "";
              const label = f.fn === "constructor" ? "constructor" : f.fn;
              return (
                `<span class="${cls} eq-${f.nb}" data-eq="eq-${f.nb}" data-ci="${ci}" data-fi="${fi}"` +
                ` data-anchor="${encodeURIComponent(c.name + "." + f.fn)}" ${hue} title="${c.name}.${f.fn}">${sym}${label}</span>`
              );
            })
            .join("");
          return `<div class="eqrow recogrow">
        <div class="libname"><b>${c.name}</b><span class="ver">${c.version}</span>
          <a class="srcfy" href="${c.url}" target="_blank" rel="noopener">sourcify ↗</a>
          <span class="rate"><b>${rec}</b>/${c.fns.length} library matches</span></div>
        <div class="eqgrid" data-ci="${ci}">${chips}</div></div>`;
        })
        .join("");
      // The key is derived from the data, with counts, never from the LIB table.
      // Listing every library we can recognize implied a three-way mix that this
      // sample does not contain: these ten contracts are OpenZeppelin-derived, so
      // the honest key is one library with 232 matches and one with 1.
      const seen = new Map();
      let novel = 0;
      for (const c of this.contracts)
        for (const f of c.fns) {
          if (!f.lib) {
            novel++;
            continue;
          }
          seen.set(f.lib, (seen.get(f.lib) || 0) + 1);
        }
      const sortedLibs = [...seen.entries()].sort((a, b) => b[1] - a[1]);
      const mainLibs = sortedLibs.filter(([, n]) => n >= 2);
      const asideLibs = sortedLibs.filter(([, n]) => n < 2);
      const legend =
        `<div class="legend">` +
        mainLibs
          .map(
            ([k, n]) =>
              `<span><span class="lg-sym" style="color:hsl(${(LIB[k] || {}).h || 0} 70% 55%)">${(LIB[k] || {}).sym || ""}</span> ${(LIB[k] || {}).n || k} <b>${n}</b></span>`,
          )
          .join("") +
        `<span><span class="lg-sym" style="color:var(--ink-dim)">○</span> no library match <b>${novel}</b></span>` +
        (asideLibs.length
          ? `<span class="lg-aside">plus ${asideLibs.map(([k, n]) => `${n} ${(LIB[k] || {}).n || k} match`).join(", ")}, shown on hover</span>`
          : "") +
        `</div>`;
      // codedock reserves a constant height; the panel inside sizes to content.
      // Keeping the dock height fixed means hovering never changes the page
      // height, so scrollTop never clamps and the chips never bump (the flicker),
      // while the visible panel still hugs each function's source.
      this.innerHTML = `${legend}${rows}<div class="codedock"><div class="codepanel"><div class="cphint">${RECOG_HINT}</div></div></div>`;
      this.panel = this.querySelector(".codepanel");
      this.addEventListener("mouseover", (e) => {
        const c = e.target.closest(".chip");
        if (c && this.contains(c)) this.show(c);
      });
      this.addEventListener("mouseleave", () => this.rest()); // fires once on leaving; settles to the pinned anchor or clears, never sticks
      this.addEventListener("click", (e) => {
        const c = e.target.closest(".chip");
        if (c && this.contains(c)) this.toggleAnchor(c);
      });
    }
    show(chip) {
      // transient view (hover): light the shape's network + fill the panel
      this.querySelectorAll(".chip.lit").forEach((x) =>
        x.classList.remove("lit"),
      );
      this.querySelectorAll(".eqgrid").forEach((g) =>
        g.classList.add("focused"),
      );
      this.querySelectorAll("." + chip.dataset.eq).forEach((x) =>
        x.classList.add("lit"),
      );
      this.panel.innerHTML = this.panelHTML(chip);
    }
    rest() {
      // settle back to the pinned anchor, or clear if nothing is pinned
      if (this.anchorChip) this.show(this.anchorChip);
      else {
        this.querySelectorAll(".chip.lit").forEach((x) =>
          x.classList.remove("lit"),
        );
        this.querySelectorAll(".eqgrid").forEach((g) =>
          g.classList.remove("focused"),
        );
        this.panel.innerHTML = `<div class="cphint">${RECOG_HINT}</div>`;
      }
    }
    toggleAnchor(chip) {
      if (chip === this.anchorChip) {
        // click the pinned one again to unpin
        chip.classList.remove("anchored");
        this.anchorChip = null;
        this.rest();
        if (location.hash !== "#recognized") location.hash = "#recognized";
        return;
      }
      this.pin(chip, false);
    }
    pin(chip, fromHash) {
      this.querySelectorAll(".chip.anchored").forEach((x) =>
        x.classList.remove("anchored"),
      );
      chip.classList.add("anchored");
      this.anchorChip = chip;
      this.show(chip);
      if (!fromHash) {
        const h = "#recognized/" + chip.dataset.anchor;
        if (location.hash !== h) location.hash = h;
      }
    }
    selectByHash(fromHash) {
      const parts = (location.hash || "").replace(/^#/, "").split("/");
      if (parts[0] !== "recognized" || !parts[1]) return;
      const chip = this.querySelector(
        `.chip[data-anchor="${parts.slice(1).join("/")}"]`,
      );
      if (chip && chip !== this.anchorChip) {
        this.pin(chip, true);
        chip.scrollIntoView({ block: "center", behavior: "smooth" });
      }
    }
    panelHTML(chip) {
      const c = this.contracts[+chip.dataset.ci],
        f = c.fns[+chip.dataset.fi];
      const inN = this.spread.get(f.nb)?.size || 1;
      const verdict =
        f.lib && LIB[f.lib]
          ? `recognized as <span class="reclib" style="color:hsl(${LIB[f.lib].h} 70% 62%)">${LIB[f.lib].sym} ${LIB[f.lib].n} ${f.canon}</span>`
          : `<span class="recnovel">no library match</span>`;
      const also =
        inN > 1 ? ` · same shape in <b>${inN}</b> of these contracts` : "";
      return (
        `<div class="cphead"><b>${c.name}.${f.fn}</b> · ${verdict} · <span class="fp">${f.nb.slice(0, 8)}</span>${also}` +
        `<a href="${c.url}" target="_blank" rel="noopener">on sourcify ↗</a></div>` +
        `<pre class="code">${solHi(f.src)}</pre>`
      );
    }
  },
);

// Twins across real contracts: the same recognized library function, the exact
// same shape, in more than one of the verified contracts. Pick one from the list
// to see its source and every contract that carries it. Precomputed (realdata).
customElements.define(
  "recog-twins",
  class extends HTMLElement {
    connectedCallback() {
      const data = window.RIFFCAT_REAL;
      if (!data || !data.contracts) {
        this.innerHTML = `<p class="live-note bad">recognition data not loaded</p>`;
        return;
      }
      this.contracts = data.contracts;
      const g = new Map(); // lib+canon -> { lib, canon, inst:[{ci,fi}], cset:Set(ci) }
      this.contracts.forEach((c, ci) =>
        c.fns.forEach((f, fi) => {
          if (!f.lib || !LIB[f.lib]) return;
          const k = f.lib + "" + f.canon;
          if (!g.has(k))
            g.set(k, { lib: f.lib, canon: f.canon, inst: [], cset: new Set() });
          const e = g.get(k);
          e.inst.push({ ci, fi });
          e.cset.add(ci);
        }),
      );
      this.shared = [...g.values()]
        .filter((e) => e.cset.size >= 2)
        .sort(
          (a, b) => b.cset.size - a.cset.size || a.canon.localeCompare(b.canon),
        );
      this.sel = 0;
      this.render();
    }
    render() {
      const n = this.contracts.length;
      const list = this.shared
        .map(
          (e, i) =>
            `<button class="twrow ${i === this.sel ? "on" : ""}" data-i="${i}">` +
            `<span class="tw-c"><b>${e.cset.size}</b>/${n}</span>` +
            `<span class="tw-n">${e.canon}</span>` +
            `<span class="tw-lib" style="color:hsl(${LIB[e.lib].h} 70% 62%)">${LIB[e.lib].n}</span></button>`,
        )
        .join("");
      this.innerHTML = `<div class="twwrap"><div class="twlist">${list}</div><div class="twview codepanel"></div></div>`;
      this.querySelectorAll(".twrow").forEach((b) =>
        b.addEventListener("click", () => {
          this.sel = +b.dataset.i;
          this.querySelectorAll(".twrow").forEach((x, i) =>
            x.classList.toggle("on", i === this.sel),
          );
          this.renderView();
        }),
      );
      this.renderView();
    }
    renderView() {
      const e = this.shared[this.sel],
        rep = e.inst[0],
        c = this.contracts[rep.ci],
        f = c.fns[rep.fi];
      const chips = [...e.cset]
        .sort((a, b) => a - b)
        .map((ci) => `<span class="cchip">${this.contracts[ci].name}</span>`)
        .join("");
      this.querySelector(".twview").innerHTML =
        `<div class="cphead">the same shape in <b>${e.cset.size}</b> of ${this.contracts.length}: ` +
        `<b style="color:hsl(${LIB[e.lib].h} 70% 62%)">${LIB[e.lib].n} ${e.canon}</b></div>` +
        `<div class="twcontracts">${chips}</div>` +
        `<pre class="code">${solHi(f.src)}</pre>` +
        `<p class="twnote">Audit it once. Every contract above carries this shape, identifiers aside.</p>`;
    }
  },
);

// Dedup at the source level: of every function across the real contracts, how
// much is a shape already in the std-lib catalog (the part you do not read) vs
// novel app code (the part you do). One bar per contract. Precomputed (realdata).
customElements.define(
  "recog-dedup",
  class extends HTMLElement {
    connectedCallback() {
      const data = window.RIFFCAT_REAL;
      if (!data || !data.contracts) {
        this.innerHTML = `<p class="live-note bad">recognition data not loaded</p>`;
        return;
      }
      this.contracts = data.contracts;
      let known = 0,
        total = 0;
      const byLib = {};
      for (const c of this.contracts)
        for (const f of c.fns) {
          total++;
          if (f.lib && LIB[f.lib]) {
            known++;
            byLib[f.lib] = (byLib[f.lib] || 0) + 1;
          }
        }
      this.stats = { known, total, novel: total - known, byLib };
      this.render();
    }
    render() {
      const s = this.stats,
        pct = Math.round((100 * s.known) / s.total),
        n = this.contracts.length;
      const libOrder = Object.entries(s.byLib).sort((a, b) => b[1] - a[1]);
      const segs =
        libOrder
          .map(
            ([lib, k]) =>
              `<span style="flex:${k};background:hsl(${LIB[lib].h} 58% 46%)" title="${LIB[lib].n} ${k}"></span>`,
          )
          .join("") +
        `<span class="seg-novel" style="flex:${s.novel}" title="novel ${s.novel}"></span>`;
      const mainLibs = libOrder.filter(([, k]) => k >= 2);
      const asideLibs = libOrder.filter(([, k]) => k < 2);
      const legend =
        mainLibs
          .map(
            ([lib, k]) =>
              `<span><span class="lg-sym" style="color:hsl(${LIB[lib].h} 58% 46%)">${LIB[lib].sym}</span> ${LIB[lib].n} ${k}</span>`,
          )
          .join("") +
        `<span><span class="lg-sym" style="color:var(--ink-dim)">○</span> novel ${s.novel}</span>` +
        (asideLibs.length
          ? `<span class="lg-aside">plus ${asideLibs.map(([lib, k]) => `${k} ${LIB[lib].n} match`).join(", ")}, shown on hover</span>`
          : "");
      const rows = this.contracts
        .map((c) => {
          const seg = {};
          for (const f of c.fns) {
            const key = f.lib && LIB[f.lib] ? f.lib : "novel";
            seg[key] = (seg[key] || 0) + 1;
          }
          const bar = Object.entries(seg)
            .sort(
              (a, b) => (a[0] === "novel" ? 1 : 0) - (b[0] === "novel" ? 1 : 0),
            )
            .map(([key, k]) =>
              key === "novel"
                ? `<span class="seg-novel" style="flex:${k}"></span>`
                : `<span style="flex:${k};background:hsl(${LIB[key].h} 58% 46%)"></span>`,
            )
            .join("");
          const known = c.fns.filter((f) => f.lib && LIB[f.lib]).length;
          return `<div class="dduprow"><span class="ddup-n">${c.name}</span><div class="ddupbar">${bar}</div><span class="ddup-r">${known}/${c.fns.length}</span></div>`;
        })
        .join("");
      this.innerHTML = `
      <div class="dduphead"><b>${s.known}</b> of <b>${s.total}</b> functions across these ${n} contracts are shapes already in a known library <span class="ddup-pct">${pct}%</span></div>
      <div class="ddupsel">These ten were selected as OpenZeppelin users with at least five library matches each, so this rate describes that sample, not the chain.</div>
      <div class="ddupbar big">${segs}</div>
      <div class="legend">${legend}</div>
      <div class="dduprows">${rows}</div>
      <p class="twnote">Each bar is one contract; the grey is its novel surface, the only part an auditor reads closely. TimelockController is entirely standard, Airdrop almost entirely its own.</p>`;
    }
  },
);

// Sniff out a known bug by shape. The ERC-4626 first-deposit inflation bug lives
// in _convertToShares; we fingerprint the vulnerable shapes, the patched shape,
// and the mitigated-but-keyword-matching solady shape, then list the deployed
// contracts that carry each. Shapes + witnesses precomputed in vulndata.js.
const VULN_STATUS = {
  vulnerable: { label: "vulnerable", hue: 2 },
  patched: { label: "patched", hue: 145 },
  mitigated: { label: "look-alike", hue: 38 },
};
const VULN_ROLE = {
  vulnerable:
    "Vulnerable: when the vault is empty this mints shares 1:1, so an attacker can steal the first real deposit.",
  patched:
    "The fix: same function name, structurally different. The fingerprint separates it from the vulnerable shape, so you can see who patched.",
  mitigated:
    "Safe look-alike: keeps the _initialConvertToShares name a text search would flag, but routes through virtual shares, so its shape is not the vulnerable one.",
};
customElements.define(
  "vuln-sniff",
  class extends HTMLElement {
    connectedCallback() {
      const data = window.RIFFCAT_VULN;
      if (!data || !data.shapes) {
        this.innerHTML = `<p class="live-note bad">vuln data not loaded</p>`;
        return;
      }
      this.data = data;
      this.shapes = data.shapes;
      this.witnesses = data.witnesses;
      this.sel = 0;
      this.render();
    }
    render() {
      const r = this.data.reach;
      const rows = this.shapes
        .map((s, i) => {
          const st = VULN_STATUS[s.status];
          const verd =
            s.status === "vulnerable"
              ? "vulnerable"
              : s.status === "patched"
                ? "safe: patched"
                : "safe: look-alike";
          const exact = s.exact == null ? "-" : s.exact.toLocaleString();
          const reach = s.reach == null ? "-" : s.reach.toLocaleString();
          return (
            `<tr class="lxrow ${i === this.sel ? "on" : ""}" data-i="${i}" style="--hue:${st.hue}">` +
            `<td class="lx-shape">${s.label.replace("OpenZeppelin", "OZ")} <span class="vsub">${s.sub}</span></td>` +
            `<td class="lx-fp">${s.structure.slice(0, 8)}</td>` +
            `<td class="lx-verd ${s.status === "vulnerable" ? "bad" : "ok"}">${verd}</td>` +
            `<td class="lx-n">${exact}</td>` +
            `<td class="lx-n lx-reach">${reach}</td></tr>`
          );
        })
        .join("");
      this.innerHTML = `
      <p class="vbug">${this.data.bug}</p>
      <p class="vlead">All four rows are functions named <code>convertToShares</code>. Across Sourcify's verified ERC-4626 vaults, an exact-source match flags <b>${r.exactVuln.toLocaleString()}</b> as the vulnerable file; matching the <b>shape</b> flags <b>${r.shapeVuln.toLocaleString()}</b>, the same bug in <b>${r.extra}</b> more vaults (${r.variants} source variants) an exact match treats as unrelated.</p>
      <table class="vledger"><thead><tr><th>shape</th><th>structure</th><th>riffcat</th><th>Sourcify exact</th><th>riffcat shape</th></tr></thead><tbody>${rows}</tbody></table>
      <p class="vledger-cap">Counts are exact over distinct Sourcify source files and an undercount; Solady's look-alike keeps the flagged name but not the shape.</p>
      <div class="vbody codepanel"></div>`;
      this.querySelectorAll(".lxrow").forEach((row) =>
        row.addEventListener("click", () => {
          this.sel = +row.dataset.i;
          this.querySelectorAll(".lxrow").forEach((x, i) =>
            x.classList.toggle("on", i === this.sel),
          );
          this.renderBody();
        }),
      );
      this.renderBody();
    }
    renderBody() {
      const s = this.shapes[this.sel],
        st = VULN_STATUS[s.status];
      // The witnesses are deliberately not named. A shape match is a candidate,
      // not a verdict, so a public list of live deployments under this heading
      // would assert exactly what the deck says a match cannot assert. The counts
      // are the claim; the identities are not ours to publish.
      const match = s.reach
        ? `riffcat matched this shape in <b>${s.reach.toLocaleString()}</b> verified vaults, where an exact-source match finds ${s.exact.toLocaleString()}.`
        : `kept the name, but its shape is not the vulnerable one.`;
      this.querySelector(".vbody").innerHTML =
        `<div class="vhead" style="--hue:${st.hue}"><span class="vbadge">${st.label}</span> <b>${s.label}</b> <span class="vsub">${s.sub}</span>` +
        `<span class="vfp">structure ${s.structure.slice(0, 10)}</span></div>` +
        `<p class="vrole">${VULN_ROLE[s.status]}</p>` +
        `<pre class="code">${solHi(s.src)}</pre>` +
        `<div class="vwits"><span class="vwlabel">${match}</span></div>`;
    }
  },
);

// Modified-variant detection (fuzzy). Real deployed forks that EDITED the
// vulnerable Multicall body, so exact whole-function matching, Sourcify's
// byte-identical match, and a text search all miss them, but weighted
// containment over the per-node Merkle digests still recognizes the shape.
// Precomputed in fuzzydata.js (the proposed `riffcat similar`); Sourcify floor.
customElements.define(
  "fuzzy-scan",
  class extends HTMLElement {
    connectedCallback() {
      const d = window.RIFFCAT_FUZZY;
      if (!d || !d.variants) {
        this.innerHTML = `<p class="live-note bad">fuzzy data not loaded</p>`;
        return;
      }
      this.data = d;
      this.variants = d.variants;
      this.sel = 0;
      this.render();
    }
    render() {
      const d = this.data;
      const rows = this.variants
        .map(
          (v, i) =>
            `<tr class="lxrow ${i === this.sel ? "on" : ""}" data-i="${i}">` +
            `<td class="lx-shape">fork ${i + 1} <span class="vsub">${v.chainName}</span></td>` +
            `<td class="lx-verd bad">none</td>` +
            `<td class="lx-n lx-reach">${Math.round(v.cwVuln * 100)}%</td></tr>`,
        )
        .join("");
      this.innerHTML = `
      <p class="vbug">${d.bug}</p>
      <p class="vlead">Each fork below <b>edited</b> the <code>multicall</code> body, so its shape is neither the vulnerable class nor the patch: exact match, Sourcify's byte-identical match, and a text search all return <b>nothing</b>. Weighted containment of the vulnerable shape still finds them. Click a row.</p>
      <div class="codepanel fref"><div class="cphead">known vulnerable function <span class="vfp">address ${d.vuln.fp}</span> <span class="vsub">${d.vuln.nodes} nodes</span></div><pre class="code">${solHi(d.vuln.src)}</pre></div>
      <table class="vledger"><thead><tr><th>edited fork</th><th>whole-function match</th><th>vulnerable structure retained</th></tr></thead><tbody>${rows}</tbody></table>
      <div class="vbody codepanel"></div>
      <p class="vledger-cap">The chance baseline for this shape is ~<b>${d.nullCeiling}</b> and the OZ patch scores ~<b>${d.patchedScore}</b>; both catches clear it, and two customized bodies land near the baseline where the score alone cannot certify them.</p>`;
      this.querySelectorAll(".lxrow").forEach((row) =>
        row.addEventListener("click", () => {
          this.sel = +row.dataset.i;
          this.querySelectorAll(".lxrow").forEach((x, i) =>
            x.classList.toggle("on", i === this.sel),
          );
          this.renderBody();
        }),
      );
      this.renderBody();
    }
    renderBody() {
      const v = this.variants[this.sel];
      this.querySelector(".vbody").innerHTML =
        `<div class="vhead" style="--hue:2"><span class="vbadge">caught, modified</span> <b>fork ${this.sel + 1}</b> <span class="vsub">${v.chainName}, ${v.nodes} nodes</span>` +
        `<span class="vfp">${Math.round(v.cwVuln * 100)}% of vulnerable structure retained</span></div>` +
        `<p class="vrole">${v.edit}</p>` +
        `<pre class="code">${solHi(v.src)}</pre>`;
    }
  },
);

// The riff chapter: the same dial, on music. A short motif and a few variants,
// fingerprinted by the very same engine (fingerprint_riff) at musical facets.
// Pick a facet and the variants that count as "the same" share a color, exactly
// like the code chapters. Playback is raw Web Audio (one triangle osc per note);
// the click is the user gesture the browser needs to start audio.
const NOTE_NAMES = [
  "C",
  "C#",
  "D",
  "D#",
  "E",
  "F",
  "F#",
  "G",
  "G#",
  "A",
  "A#",
  "B",
];
const noteName = (p) =>
  NOTE_NAMES[((p % 12) + 12) % 12] + (Math.floor(p / 12) - 1);
const RIFF_FACETS = [
  ["full", "every note"],
  ["harmonic_relationships", "intervals"],
  ["rhythm", "rhythm"],
  ["pitch_class_set", "note set"],
];
// The riff is the opening of Schubert's "An die Musik" (D.547), "Du holde
// Kunst...", in D major. (A by-ear reading; the exact pitches are easy to tune.)
// The variants are built from it: a true transposition (up a fifth), a
// re-voicing that keeps the same set of notes, and a line that keeps only the
// rhythm. Each one collapses onto the riff at a different facet.
const RIFFS = [
  {
    name: "An die Musik",
    notes: [
      [69, 2],
      [69, 1],
      [71, 1],
      [69, 2],
      [66, 1],
      [64, 1],
      [66, 2],
      [62, 2],
    ],
  },
  {
    name: "up a fifth",
    notes: [
      [76, 2],
      [76, 1],
      [78, 1],
      [76, 2],
      [73, 1],
      [71, 1],
      [73, 2],
      [69, 2],
    ],
  },
  {
    name: "same notes, re-voiced",
    notes: [
      [62, 1],
      [78, 1],
      [66, 1],
      [81, 1],
      [64, 1],
      [71, 2],
    ],
  },
  {
    name: "same rhythm, new notes",
    notes: [
      [72, 2],
      [67, 1],
      [71, 1],
      [67, 2],
      [65, 1],
      [69, 1],
      [67, 2],
      [72, 2],
    ],
  },
  {
    name: "a different riff",
    notes: [
      [60, 1],
      [60, 1],
      [67, 1],
      [67, 1],
      [69, 2],
    ],
  },
];
// A sweet, soft flute-ish voice in raw Web Audio: a near-sine tone (fundamental
// plus a faint octave) with a gentle ~5.5 Hz vibrato that eases in, a whisper of
// band-passed breath noise, a soft lowpass, and a short convolver reverb for
// air. No samples, no deps; the button click is the gesture audio needs.
let _ac = null;
function fluteCtx() {
  if (_ac) return _ac;
  const Ctx = window.AudioContext || window.webkitAudioContext;
  if (!Ctx) return null;
  _ac = new Ctx();
  const len = Math.floor(_ac.sampleRate * 1.5),
    ir = _ac.createBuffer(2, len, _ac.sampleRate);
  for (let ch = 0; ch < 2; ch++) {
    const data = ir.getChannelData(ch);
    for (let i = 0; i < len; i++)
      data[i] = (Math.random() * 2 - 1) * Math.pow(1 - i / len, 2.8);
  }
  const verb = _ac.createConvolver();
  verb.buffer = ir;
  const wet = _ac.createGain();
  wet.gain.value = 0.25;
  verb.connect(wet).connect(_ac.destination);
  _ac._verb = verb;
  return _ac;
}
function playRiff(notes) {
  const ac = fluteCtx();
  if (!ac) return;
  if (ac.state === "suspended") ac.resume();
  const sec = 0.34;
  let t = ac.currentTime + 0.05;
  for (const [pitch, dur] of notes) {
    const d = dur * sec;
    const freq = 440 * Math.pow(2, (pitch - 69) / 12);
    const o1 = ac.createOscillator();
    o1.type = "sine";
    o1.frequency.value = freq;
    const o2 = ac.createOscillator();
    o2.type = "sine";
    o2.frequency.value = freq * 2;
    const o2g = ac.createGain();
    o2g.gain.value = 0.1;
    // vibrato: gentle, eased in over the note's first moments
    const lfo = ac.createOscillator();
    lfo.type = "sine";
    lfo.frequency.value = 5.5;
    const lfoG = ac.createGain();
    lfoG.gain.setValueAtTime(0, t);
    lfoG.gain.linearRampToValueAtTime(
      freq * 0.007,
      t + Math.min(0.22, d * 0.6),
    );
    lfo.connect(lfoG);
    lfoG.connect(o1.frequency);
    lfoG.connect(o2.frequency);
    // breath: a whisper of band-passed noise, gated with the note
    const nlen = Math.ceil(d * ac.sampleRate) + 1,
      nb = ac.createBuffer(1, nlen, ac.sampleRate),
      nd = nb.getChannelData(0);
    for (let i = 0; i < nlen; i++) nd[i] = Math.random() * 2 - 1;
    const noise = ac.createBufferSource();
    noise.buffer = nb;
    const nbp = ac.createBiquadFilter();
    nbp.type = "bandpass";
    nbp.frequency.value = freq * 2.5;
    nbp.Q.value = 0.6;
    const ng = ac.createGain();
    const lp = ac.createBiquadFilter();
    lp.type = "lowpass";
    lp.frequency.value = 2400;
    const env = ac.createGain();
    const A = 0.06,
      R = 0.16,
      peak = 0.16;
    env.gain.setValueAtTime(0.0001, t);
    env.gain.linearRampToValueAtTime(peak, t + A);
    env.gain.setValueAtTime(peak, t + Math.max(A + 0.01, d - R));
    env.gain.exponentialRampToValueAtTime(0.0006, t + d);
    ng.gain.setValueAtTime(0, t);
    ng.gain.linearRampToValueAtTime(0.02, t + A);
    ng.gain.linearRampToValueAtTime(0.0001, t + d);
    o1.connect(lp);
    o2.connect(o2g).connect(lp);
    noise.connect(nbp).connect(ng).connect(lp);
    lp.connect(env);
    env.connect(ac.destination);
    env.connect(ac._verb);
    o1.start(t);
    o2.start(t);
    lfo.start(t);
    noise.start(t);
    const end = t + d + 0.06;
    o1.stop(end);
    o2.stop(end);
    lfo.stop(end);
    noise.stop(end);
    t += d;
  }
}
customElements.define(
  "riff-dial",
  class extends HTMLElement {
    async connectedCallback() {
      this.facet = "harmonic_relationships";
      this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
      try {
        this.b = await engineReady;
        this.fp = RIFFS.map((r) => ({
          ...r,
          addr: JSON.parse(
            this.b.fingerprint_riff(
              JSON.stringify({
                notes: r.notes.map(([pitch, dur]) => ({ pitch, dur })),
              }),
            ),
          ),
        }));
        this.render();
      } catch (e) {
        this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`;
      }
    }
    render() {
      const ladder = RIFF_FACETS.map(
        ([k, label]) =>
          `<span class="stop ${k === this.facet ? "on" : ""}" data-facet="${k}">${label}</span>`,
      ).join("");
      const syms = symMapFor(this.fp.map((r) => r.addr[this.facet]));
      const groups = new Map();
      for (const r of this.fp) {
        const a = r.addr[this.facet];
        if (!groups.has(a)) groups.set(a, []);
        groups.get(a).push(r.name);
      }
      const rows = this.fp
        .map((r, i) => {
          const a = r.addr[this.facet];
          const chips = r.notes
            .map(([p]) => `<span class="nchip">${noteName(p)}</span>`)
            .join("");
          return (
            `<div class="riffrow"><button class="playbtn" data-i="${i}">▶ play</button>` +
            `<span class="riffname">${r.name}</span>` +
            `<span class="nchips">${chips}</span>` +
            `<span class="shapedot" style="--chip:${chipColor(a)}" title="address ${a.slice(0, 10)}">${syms.get(a)}</span></div>`
          );
        })
        .join("");
      const n = groups.size;
      const groupTxt = [...groups.values()]
        .map((names) =>
          names.length > 1 ? `<b>${names.join(" = ")}</b>` : names[0],
        )
        .join(" · ");
      const idle = `<b>${n}</b> group${n === 1 ? "" : "s"} under this comparison · ${groupTxt}`;
      this.innerHTML = `
      <div class="dialbar"><div class="grp"><span>facet</span><div class="ladder">${ladder}</div></div></div>
      <div class="riffs">${rows}</div>
      <div class="eqread">${idle}</div>
      <p class="twnote">The same facet machinery as the code chapters (<code>fingerprint_riff</code>): transpose and the intervals survive, keep durations and the rhythm survives, reorder and the note set survives.</p>`;
      this.querySelectorAll("[data-facet]").forEach((s) =>
        s.addEventListener("click", () => {
          if (this.facet !== s.dataset.facet) {
            this.facet = s.dataset.facet;
            this.render();
          }
        }),
      );
      this.querySelectorAll(".playbtn").forEach((btn) =>
        btn.addEventListener("click", () =>
          playRiff(this.fp[+btn.dataset.i].notes),
        ),
      );
    }
  },
);

// Play a chord: its pitch classes sounded together, soft flute-ish (shares the
// reverb with the riff voice). A simpler voice than playRiff (no breath layer).
function playChord(pcs) {
  const ac = fluteCtx();
  if (!ac) return;
  if (ac.state === "suspended") ac.resume();
  const t = ac.currentTime + 0.05,
    d = 1.7,
    gain = 0.13 / Math.max(2, pcs.length);
  pcs.forEach((pc, i) => {
    const freq = 440 * Math.pow(2, (60 + pc - 69) / 12);
    const o = ac.createOscillator();
    o.type = "sine";
    o.frequency.value = freq;
    const lfo = ac.createOscillator();
    lfo.type = "sine";
    lfo.frequency.value = 5.2 + i * 0.2;
    const lg = ac.createGain();
    lg.gain.setValueAtTime(0, t);
    lg.gain.linearRampToValueAtTime(freq * 0.006, t + 0.3);
    lfo.connect(lg);
    lg.connect(o.frequency);
    const env = ac.createGain();
    env.gain.setValueAtTime(0.0001, t);
    env.gain.linearRampToValueAtTime(gain, t + 0.08);
    env.gain.setValueAtTime(gain, t + d - 0.4);
    env.gain.exponentialRampToValueAtTime(0.0005, t + d);
    o.connect(env);
    env.connect(ac.destination);
    env.connect(ac._verb);
    o.start(t);
    lfo.start(t);
    o.stop(t + d + 0.05);
    lfo.stop(t + d + 0.05);
  });
}

// Chords parsed from real notation, fingerprinted at two facets: the literal
// note set (two spellings of the same notes collapse) and the Forte set class
// (major, minor, and other inversions of one class collapse to a single anchor).
const CHORD_FACETS = [
  ["note_set", "note set"],
  ["set_class", "set class"],
];
const CHORDS = ["C", "Cdo", "Am", "F", "Caug", "Bdim"];
customElements.define(
  "chord-fp",
  class extends HTMLElement {
    async connectedCallback() {
      this.facet = "set_class";
      this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
      try {
        const b = await engineReady;
        this.data = CHORDS.map((c) => {
          try {
            return { c, fp: JSON.parse(b.fingerprint_chord(c)) };
          } catch {
            return null;
          }
        }).filter(Boolean);
        this.render();
      } catch (e) {
        this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`;
      }
    }
    render() {
      const ladder = CHORD_FACETS.map(
        ([k, label]) =>
          `<span class="stop ${k === this.facet ? "on" : ""}" data-facet="${k}">${label}</span>`,
      ).join("");
      const syms = symMapFor(this.data.map((d) => d.fp[this.facet]));
      const groups = new Map();
      for (const d of this.data) {
        const a = d.fp[this.facet];
        if (!groups.has(a)) groups.set(a, []);
        groups.get(a).push(d.c);
      }
      const rows = this.data
        .map((d, i) => {
          const a = d.fp[this.facet];
          const notes = d.fp.pitch_classes
            .map((pc) => `<span class="nchip">${NOTE_NAMES[pc]}</span>`)
            .join("");
          // Show the Tn-type AND the prime form it folds to, both live from the
          // engine. That way the prime [0 3 7] under a major triad reads as the
          // inversion fold (where major and minor meet), not as a minor mislabel.
          const pf =
            this.facet === "set_class"
              ? ` <span class="vsub">Tn [${d.fp.transposition_normal_form.join(" ")}] · prime [${d.fp.prime_form.join(" ")}]</span>`
              : "";
          return (
            `<div class="riffrow"><button class="playbtn" data-i="${i}">▶ play</button>` +
            `<span class="riffname">${d.c}</span><span class="nchips">${notes}</span>${pf}` +
            `<span class="shapedot" style="--chip:${chipColor(a)}" title="${this.facet} ${a.slice(0, 10)}">${syms.get(a)}</span></div>`
          );
        })
        .join("");
      const n = groups.size;
      const gtxt = [...groups.values()]
        .map((cs) => (cs.length > 1 ? `<b>${cs.join(" = ")}</b>` : cs[0]))
        .join(" · ");
      const note =
        this.facet === "set_class"
          ? `At the <b>set-class</b> facet, riffcat lands on Allen Forte's catalog: C, Cdo, Am and F collapse to one class. Each card shows its own <b>Tn-type</b> and the <b>prime form</b> it folds to: the major triads sit at Tn [0 4 7] and the minor at [0 3 7], yet all of them fold to the one prime [0 3 7], the unlettered <b>3-11</b>. The augmented and diminished triads are their own. The Forte-catalog chapter, later in the tour, splits that fold back into 3-11A and 3-11B; here, the engine rediscovers set theory from the structure alone.`
          : `At the <b>note-set</b> facet, two spellings of the same notes share an anchor (C and its solfege spelling Cdo); every other chord is its own set of notes.`;
      this.innerHTML = `
      <div class="dialbar"><div class="grp"><span>facet</span><div class="ladder">${ladder}</div></div></div>
      <div class="riffs">${rows}</div>
      <div class="eqread"><b>${n}</b> shape${n === 1 ? "" : "s"} at this facet · ${gtxt}</div>
      <p class="twnote">${note}</p>`;
      this.querySelectorAll("[data-facet]").forEach((s) =>
        s.addEventListener("click", () => {
          if (this.facet !== s.dataset.facet) {
            this.facet = s.dataset.facet;
            this.render();
          }
        }),
      );
      this.querySelectorAll(".playbtn").forEach((btn) =>
        btn.addEventListener("click", () =>
          playChord(this.data[+btn.dataset.i].fp.pitch_classes),
        ),
      );
    }
  },
);

// Framing the verifier, the gist corrected. riffcat does not climb from
// syntactic to semantic; it lives on the syntactic side, a LATTICE of facets,
// each one a CONTENT ADDRESS (a URI for a syntactic equivalence class). Coarser
// facets forget more and collapse more, moving toward meaning without reaching
// it or aiming to. Meaning is an ORTHOGONAL, incomparable axis (an optimizer
// relates programs that share no address; a changed constant splits programs
// that share one). riffcat is the SUBSTRATE: a semantic statement pins to a
// facet address and rides it, as far as the facet covers the statement's
// footprint. Hand-laid SVG; hover a facet, a witness, or the pinned claim.
const SS_FACETS = {
  full: {
    x: 240,
    y: 66,
    t: "all syntax",
    chip: "var(--a)",
    s: "Everything participates in the address. Only syntactically identical artifacts match.",
  },
  namesblind: {
    x: 130,
    y: 170,
    t: "ignore local names",
    chip: "var(--cool)",
    s: "Renaming a local identifier does not move the address. Behaviorally significant names still require care.",
  },
  constblind: {
    x: 360,
    y: 170,
    t: "ignore constants",
    chip: "var(--warm)",
    s: "Changing a literal does not move the address. Claims that depend on its value cannot be reused here.",
  },
  structure: {
    x: 240,
    y: 274,
    t: "structure only",
    chip: "var(--ink-dim)",
    s: "Names, constants, and types are omitted. This identifies structural agreement, not behavioral equivalence.",
  },
};
const SS_EDGES = [
  ["full", "namesblind"],
  ["full", "constblind"],
  ["namesblind", "structure"],
  ["constblind", "structure"],
];
const SS_HOVERS = {
  optimizer:
    "An optimizer may produce structurally different code with the same behavior. Semantic equivalence can relate artifacts that share no structural address.",
  constant:
    "PUSH1 3 and PUSH1 4 share a structure-only address but return different values. Structural agreement alone is not a proof of equivalence.",
  anchor:
    "A proof can be attached to an address when it depends only on information that address preserves.",
};
customElements.define(
  "facet-lattice",
  class extends HTMLElement {
    connectedCallback() {
      const F = SS_FACETS;
      const edges = SS_EDGES.map(
        ([a, b]) =>
          `<line x1="${F[a].x}" y1="${F[a].y + 16}" x2="${F[b].x}" y2="${F[b].y - 16}" class="latedge"/>`,
      ).join("");
      const fnode = (k) => {
        const n = F[k];
        return (
          `<g class="latnode" data-k="${k}" transform="translate(${n.x},${n.y})">` +
          `<rect x="-70" y="-16" width="140" height="32" rx="5"/>` +
          `<circle class="ss-chip" cx="-54" cy="0" r="5" style="fill:${n.chip}"/>` +
          `<text x="10" y="4" text-anchor="middle">${n.t}</text></g>`
        );
      };
      const idle =
        "Hover an address or an example to see what the comparison establishes.";
      this.innerHTML = `
      <svg viewBox="0 0 720 360" class="lattice" role="img" aria-label="syntactic facets as addresses, and the orthogonal axis of meaning">
        <defs><marker id="ssarr" markerWidth="9" markerHeight="9" refX="6" refY="3" orient="auto">
          <path d="M0,0 L6,3 L0,6 z" class="latarrhead"/></marker></defs>
        <text x="240" y="26" text-anchor="middle" class="latcap">structural addresses</text>
        <text x="588" y="26" text-anchor="middle" class="latcap sem">claims about behavior</text>
        <line x1="448" y1="44" x2="448" y2="336" class="latdivide"/>
        <path d="M 44 80 L 44 262" class="ss-coarse" marker-end="url(#ssarr)"/>
        <text x="36" y="170" text-anchor="middle" class="ss-coarse-lab" transform="rotate(-90 36 170)">fewer syntactic details participate</text>
        ${edges}
        ${Object.keys(F).map(fnode).join("")}
        <line x1="100" y1="314" x2="380" y2="314" class="ss-floor"/>
        <text x="240" y="330" text-anchor="middle" class="ss-floor-lab">behavioral equivalence is a separate relation</text>
        <g class="ss-wg" data-k="optimizer">
          <text x="588" y="90" text-anchor="middle">different shape, same behavior</text>
          <circle class="ss-wdot" cx="500" cy="106" r="7"/>
          <circle class="ss-wdot" cx="676" cy="106" r="7"/>
          <path class="ss-wlink eq" d="M 508 106 L 668 106"/>
        </g>
        <g class="ss-wg" data-k="constant">
          <text x="588" y="158" text-anchor="middle">same shape, one constant changed</text>
          <circle class="ss-wdot" cx="566" cy="174" r="7"/>
          <circle class="ss-wdot" cx="610" cy="174" r="7"/>
          <path class="ss-wlink ne" d="M 574 174 L 602 174"/>
        </g>
        <text x="588" y="222" text-anchor="middle" class="latfv">hevm · EquiVM · SMTChecker · Lean</text>
        <g class="ss-pin" data-k="anchor">
          <rect x="470" y="280" width="118" height="30" rx="6"/>
          <text x="529" y="299" text-anchor="middle">checked claim</text>
        </g>
        <path d="M 470 294 C 452 250, 452 200, 432 174" class="lathandoff" marker-end="url(#ssarr)"/>
        <text x="529" y="332" text-anchor="middle" class="latflow">reusable where its required details are preserved</text>
      </svg>
      <div class="eqread" data-idle="${idle}">${idle}</div>
      <p class="twnote"><b>Meaning is a different axis, not the top of this ladder.</b> An optimizer can change every structural address while preserving behavior; one changed constant can preserve a shape while changing behavior. riffcat does not decide meaning. It gives a semantic claim a precise place to attach.</p>`;
      const read = this.querySelector(".eqread");
      const clear = () =>
        this.querySelectorAll(".latnode, .ss-wg, .ss-pin").forEach((x) =>
          x.classList.remove("on"),
        );
      this.querySelectorAll(".latnode").forEach((g) =>
        g.addEventListener("mouseenter", () => {
          clear();
          g.classList.add("on");
          const n = SS_FACETS[g.dataset.k];
          read.innerHTML = `<b>${n.t}</b> · ${n.s}`;
        }),
      );
      this.querySelectorAll(".ss-wg, .ss-pin").forEach((g) =>
        g.addEventListener("mouseenter", () => {
          clear();
          g.classList.add("on");
          read.innerHTML = SS_HOVERS[g.dataset.k];
        }),
      );
      this.addEventListener("mouseleave", () => {
        clear();
        read.innerHTML = read.dataset.idle;
      });
    }
  },
);

// The empty verdict seat. riffcat localizes a candidate (a shape match at a
// chosen facet) and emits a proof obligation; the claim-ledger row's verdict
// and attested-by columns are an EMPTY SEAT, awaiting a verifier. This view is
// pure narrative + a static ledger (no engine call): it renders synchronously.
// Selecting a candidate row shows the obligation riffcat hands off and the seat
// that stays open; selecting an adjudicator names what that tool would discharge
// and how (a proof, or a counterexample). riffcat never fills the seat itself.
// Each candidate is a localized shape match from an earlier chapter, restated as
// a proof obligation: what riffcat asserts (structural), the facet the anchor was
// computed at (the scope the obligation stays sound in), and the semantic claim
// that is OPEN, i.e. not ours to make. footprint names the dimensions the
// semantic claim depends on, so the reader sees why a coarser anchor would not
// carry it. Grounded in the proof-and-anchors note: localize plus provenance
// plus adjudicate, attach the adjudication to the localized anchor at the right facet.
const SEAT_CANDIDATES = [
  {
    key: "twins",
    subj: "Solady FixedPointMathLib.mulDiv",
    from: "twins across real contracts",
    facet: "names-blind",
    asserts: "the same shape, identifiers aside, in several deployed contracts",
    open: "do these instances compute the same function",
    footprint: "structure, constants, types",
    obligation:
      "check whether each pair behaves the same, or find an input where it differs",
  },
  {
    key: "vuln",
    subj: "ERC-4626 convertToShares (inflation shape)",
    from: "the bug-shape sniff",
    facet: "structure",
    asserts:
      "the control- and data-flow shape of the first-depositor inflation pattern",
    open: "is this instance actually exploitable",
    footprint: "structure, constants (the guard threshold)",
    obligation: "check whether an attacker can reach the unguarded mint",
  },
  {
    key: "fuzzy",
    subj: "Multicall (edited fork)",
    from: "the modified forks",
    facet: "structure (weighted containment)",
    asserts:
      "enough of the known vulnerable subtree is still present to be a candidate",
    open: "is the surviving shape still the vulnerable one",
    footprint: "structure, resolved callees",
    obligation: "check whether the vulnerable path remains after the edit",
  },
];

// The seat's candidate fillers. Each is a real verifier; the note states, in the
// phrasebook voice, the verb it does (prove / refute / model-check) and the layer
// it works at. SMTChecker is the warmest door (it ships inside solc); the others
// are named honestly as the tools that COULD fill the seat, not endorsements.
const SEAT_ADJUDICATORS = [
  {
    id: "hevm",
    name: "hevm",
    does: "symbolic execution and equivalence checking over EVM bytecode; returns a proof or a concrete counterexample.",
  },
  {
    id: "smtchecker",
    name: "SMTChecker",
    does: "ships inside solc; discharges assertions and reachability over the source. The closest door, because the obligation can travel with the compile.",
  },
  {
    id: "kontrol",
    name: "Kontrol",
    does: "K-framework proofs over EVM semantics; the obligation becomes a claim it discharges or leaves open.",
  },
  {
    id: "certora",
    name: "Certora",
    does: "specification-driven verification; the localized candidate becomes a rule to prove or a violation to surface.",
  },
];

const SEAT_EMPTY =
  '<span class="seat-empty" title="not checked yet">not checked</span>';

customElements.define(
  "verdict-seat",
  class extends HTMLElement {
    connectedCallback() {
      this.sel = 0; // selected candidate row
      this.tool = null; // selected adjudicator (null = seat still empty)
      this.render();
    }
    render() {
      const rows = SEAT_CANDIDATES.map(
        (c, i) =>
          `<tr class="lxrow seatrow ${i === this.sel ? "on" : ""}" data-i="${i}">` +
          `<td class="lx-shape">${c.subj} <span class="vsub">${c.from}</span></td>` +
          `<td class="seat-asserts">${c.asserts}</td>` +
          `<td class="lx-fp">${c.facet}</td>` +
          `<td class="lx-verd seat-cell">${this.tool ? this.tool.name : SEAT_EMPTY}</td>` +
          `<td class="lx-verd seat-cell">${this.tool ? '<span class="seat-pending">a proof or a counterexample</span>' : SEAT_EMPTY}</td></tr>`,
      ).join("");
      const chips = SEAT_ADJUDICATORS.map(
        (a) =>
          `<button class="seat-tool ${this.tool && this.tool.id === a.id ? "on" : ""}" data-tool="${a.id}">${a.name}</button>`,
      ).join("");
      this.innerHTML = `
      <table class="vledger seat-ledger">
        <thead><tr><th>code riffcat found</th><th>what matched</th><th>matched as</th><th>checked by</th><th>result</th></tr></thead>
        <tbody>${rows}</tbody>
      </table>
      <p class="vledger-cap">Click a match to see the question another tool still needs to answer.</p>
      <div class="seat-handoff codepanel"></div>
      <div class="seat-fill">
        <span class="seat-fill-label">choose a tool to check it</span>
        ${chips}
        <button class="seat-tool seat-clear ${this.tool ? "" : "on"}" data-tool="">leave it open</button>
      </div>
      <p class="twnote">A checked result can be reused at a shared address only when the result depends exclusively on information preserved by that address. The address records structural agreement. The verification tool establishes the behavioral claim.</p>`;
      this.querySelectorAll(".seatrow").forEach((row) =>
        row.addEventListener("click", () => {
          this.sel = +row.dataset.i;
          this.querySelectorAll(".seatrow").forEach((x, i) =>
            x.classList.toggle("on", i === this.sel),
          );
          this.renderHandoff();
        }),
      );
      this.querySelectorAll("[data-tool]").forEach((btn) =>
        btn.addEventListener("click", () => {
          const id = btn.dataset.tool;
          this.tool = id ? SEAT_ADJUDICATORS.find((a) => a.id === id) : null;
          this.render(); // re-render so the ledger's verdict/attested columns reflect the seat
        }),
      );
      this.renderHandoff();
    }
    renderHandoff() {
      const c = SEAT_CANDIDATES[this.sel];
      const seatLine = this.tool
        ? `<span class="seat-named"><b>${this.tool.name}</b> takes the seat</span>: ${this.tool.does}`
        : `<span class="seat-open">not checked yet</span>: riffcat found the match and stopped there.`;
      this.querySelector(".seat-handoff").innerHTML =
        `<div class="cphead seat-head"><b>${c.subj}</b> <span class="vsub">matched as ${c.facet}</span></div>` +
        `<div class="seat-ob">` +
        `<div class="seat-ob-row"><span class="seat-tag">what matched</span><span class="seat-val">${c.asserts}</span></div>` +
        `<div class="seat-ob-row"><span class="seat-tag">what we don't know</span><span class="seat-val open">${c.open}</span></div>` +
        `<div class="seat-ob-row"><span class="seat-tag">what the answer depends on</span><span class="seat-val">${c.footprint}</span></div>` +
        `<div class="seat-ob-row"><span class="seat-tag">what to check</span><span class="seat-val ob">${c.obligation}</span></div>` +
        `</div>` +
        `<div class="seat-resolve">${seatLine}</div>`;
    }
  },
);

// Addresses as anchors: a fact is attached to a facet address and transports to
// every shape sharing that address. Live chords (fingerprint_chord at the
// note-set and set-class facets) are the runnable instance; the transport is
// sound exactly when the anchor facet covers the fact's footprint (the
// dimensions its truth depends on). The reader picks a fact, picks an anchor,
// and the component shows where the fact rides and, when the anchor is too
// coarse, where it would WRONGLY ride. Nothing here adjudicates meaning; it
// shows the precise scope a transported fact stays sound in.
//
// All names below are prefixed `anc` to avoid collision with app.js. Reuses
// engineReady, chipColor, NOTE_NAMES, and the .dialbar/.ladder/.stop/.eqread/
// .twnote/.shapedot classes; new visuals are the .anc* classes in the css field.

// The two live facets the chord engine exposes, coarse-to-fine is the OTHER way:
// set_class forgets transposition (coarser), note_set keeps the literal pitch
// classes (finer). We order the ladder fine -> coarse so sliding right is
// "forget more", matching the dial metaphor everywhere else.
const ANC_FACETS = [
  { key: "note_set", label: "exact notes", drops: "keeps the exact notes" },
  {
    key: "set_class",
    label: "chord shape",
    drops: "allows the chord to move to another key",
  },
];

// The chords we anchor across. Chosen so a whole family collapses at set_class
// (every major or minor triad is Forte 3-11) while two chords stand alone, so a
// fact pinned at set_class has somewhere to ride and somewhere it must not.
const ANC_CHORDS = ["C", "Am", "F", "Em", "Caug", "Bdim"];

// The facts a reader can pin, each with a DECLARED FOOTPRINT: the finest facet
// its truth depends on. `foot` is the index into ANC_FACETS that the fact needs
// covered. A fact transports soundly along an anchor only if the anchor is at
// least as fine as the footprint, i.e. anchorIndex <= foot (lower index = finer
// here). The "wrongly" case is precisely anchorIndex > foot.
//   - note-set footprint (foot 0): depends on the literal pitch classes, so it
//     needs the note-set anchor; riding it on set_class is unsound.
//   - set-class footprint (foot 1): depends only on the transposition class, so
//     it rides the coarse set-class anchor soundly and reaches the whole family.
const ANC_FACTS = [
  {
    key: "triad",
    foot: 1,
    label: "is a major or minor chord",
    say: "this depends on the chord's shape, not the key it is played in",
    // true exactly of the chords whose prime form is the 3-11 class [0,3,7]
    holds: (d) => JSON.stringify(d.prime_form) === JSON.stringify([0, 3, 7]),
  },
  {
    key: "evenness",
    foot: 1,
    label: "uses all six interval sizes",
    say: "this depends on the distances between notes, not their literal names",
    holds: (d) => d.interval_vector.every((x) => x > 0),
  },
  {
    key: "rootC",
    foot: 0,
    label: "contains C",
    say: "this depends on the literal notes, not just the chord's shape",
    holds: (d) => d.pitch_classes.includes(0),
  },
  {
    key: "hasE",
    foot: 0,
    label: "contains E",
    say: "this also depends on the literal notes",
    holds: (d) => d.pitch_classes.includes(4),
  },
];

// A short static catalog that carries the same shape into the code/FV domain, so
// the formal point lands even though the live engine here speaks chords. Each
// entry names a fact, the dimensions its truth depends on (its footprint), and
// the home facet that footprint defines. This is prose-as-data, not a live
// computation; it mirrors section 3 of the proof note.
const ANC_FV = [
  {
    fact: "matches the ERC-4626 inflation vuln shape",
    foot: "structure",
    home: "structure",
    note: "a control-and-data-flow shape, so it rides the structural anchor and reaches every structural twin: triage, not a verdict.",
  },
  {
    fact: "is OpenZeppelin mulDiv, identifiers aside",
    foot: "structure, names",
    home: "names-blind",
    note: "an identification modulo names; it rides names-blind, not bare structure.",
  },
  {
    fact: "the overflow guard holds at this threshold",
    foot: "structure, constants",
    home: "keeps constants",
    note: "depends on a literal, so transporting it along a constants-blind anchor would be unsound.",
  },
];

const ancShort = (h) => (h || "").slice(0, 8);

customElements.define(
  "anchor-transport",
  class extends HTMLElement {
    async connectedCallback() {
      this.anchor = 1; // index into ANC_FACETS; start coarse (set_class) to show the family
      this.factKey = "triad";
      this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
      try {
        const b = await engineReady;
        // Fingerprint each chord once; we read its facet addresses + the raw
        // pitch data the facts inspect straight off the binding's full result.
        this.data = ANC_CHORDS.map((c) => {
          try {
            return { c, fp: JSON.parse(b.fingerprint_chord(c)) };
          } catch {
            return null;
          }
        }).filter(Boolean);
        this.render();
      } catch (e) {
        this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`;
      }
    }
    render() {
      const facet = ANC_FACETS[this.anchor];
      const fact = ANC_FACTS.find((f) => f.key === this.factKey);

      // The anchor address each chord computes at the chosen facet. Chords that
      // share an address share an anchor; a pinned fact rides exactly those.
      const addrOf = (fp) => fp[facet.key];
      const syms = symMapFor(this.data.map((d) => addrOf(d.fp)));

      // The chord we pin the fact ON: the first chord for which the fact actually
      // holds, so "pin here" is always meaningful. Its address is the anchor the
      // fact is attached to.
      const subject = this.data.find((d) => fact.holds(d.fp)) || this.data[0];
      const anchorAddr = addrOf(subject.fp);

      // Who shares that anchor (the address class the fact transports across).
      const riders = this.data.filter((d) => addrOf(d.fp) === anchorAddr);

      // The soundness check, per TRANSPORT: the anchor must be at least as fine as
      // the footprint. Lower index = finer, so cover holds iff anchor <= foot.
      const covers = this.anchor <= fact.foot;

      // Where the fact would WRONGLY ride: a shape that shares the anchor but on
      // which the fact does NOT actually hold. These can only appear when the
      // anchor dropped a dimension the fact depended on, i.e. when cover fails.
      const wrong = riders.filter((d) => !fact.holds(d.fp));

      const ladder = ANC_FACETS.map(
        (f, i) =>
          `<span class="stop ${i === this.anchor ? "on" : ""}" data-anchor="${i}" title="${f.drops}">${f.label}</span>`,
      ).join("");

      const factPills = ANC_FACTS.map(
        (f) =>
          `<button class="ancfact ${f.key === this.factKey ? "on" : ""}" data-fact="${f.key}">` +
          `<span class="ancfoot foot-${f.foot}">depends on ${ANC_FACETS[f.foot].label}</span>${f.label}</button>`,
      ).join("");

      // One row per chord: its notes, its address at the anchor, and a transport
      // state. A rider whose fact holds is a SOUND landing; a rider whose fact
      // fails is a WRONG landing (only possible when cover fails); a non-rider is
      // simply out of scope.
      const rows = this.data
        .map((d) => {
          const a = addrOf(d.fp);
          const isRider = a === anchorAddr;
          const ok = fact.holds(d.fp);
          const state = !isRider ? "out" : ok ? "sound" : "wrong";
          const notes = d.fp.pitch_classes
            .map((pc) => `<span class="nchip">${NOTE_NAMES[pc]}</span>`)
            .join("");
          const tag =
            state === "sound"
              ? "fact holds here"
              : state === "wrong"
                ? "fact would be false here"
                : "different address";
          const subjMark =
            d === subject
              ? `<span class="ancpin" title="the fact is pinned here">attached here</span>`
              : "";
          return (
            `<div class="ancrow anc-${state}" data-play="${d.fp.pitch_classes.join(",")}" title="click to hear this chord">` +
            `<span class="shapedot" style="--chip:${chipColor(a)}" title="${facet.label} ${ancShort(a)}">${syms.get(a)}</span>` +
            `<span class="riffname">${d.c}${subjMark}</span>` +
            `<span class="nchips">${notes}</span>` +
            `<span class="ancaddr">${ancShort(a)}</span>` +
            `<span class="anctag">${tag}</span></div>`
          );
        })
        .join("");

      const soundLine = covers
        ? `<span class="anc-ok">safe to reuse</span>: <b>${facet.label}</b> preserves everything this fact depends on.`
        : `<span class="anc-bad">not safe to reuse</span>: <b>${facet.label}</b> omits information the fact depends on. The fact would be false for ${wrong.map((d) => d.c).join(", ") || "at least one match"}.`;

      const idle =
        `The fact <b>${fact.label}</b> is attached to <b>${subject.c}</b> using its <b>${facet.label}</b> address.` +
        ` <b>${riders.length}</b> chord${riders.length === 1 ? "" : "s"} share that address.`;

      this.innerHTML = `
      <div class="ancfacts">
        <div class="anclabel">choose a fact</div>
        ${factPills}
        <p class="ancsay detail-only">${fact.say}. It can safely follow the <b>${ANC_FACETS[fact.foot].label}</b> handle.</p>
      </div>
      <div class="dialbar">
        <div class="grp"><span>anchor facet</span><div class="ladder">${ladder}</div></div>
        <span class="ancslidehint">the broader address preserves fewer details</span>
      </div>
      <div class="ancrows">${rows}</div>
      <div class="anccover ${covers ? "ok" : "bad"}">${soundLine}</div>
      <div class="eqread" data-idle="${idle}">${idle}</div>
      <p class="twnote">Reuse is a lookup, and the soundness condition is explicit: the address must preserve every input the fact depends on, so a broader address can make the fact false.</p>
      <div class="ancfv detail-only">
        <div class="anclabel">the same condition, on code (stated, not computed here)</div>
        ${ANC_FV.map(
          (e) =>
            `<div class="ancfvrow"><span class="ancfvfact">${e.fact}</span>` +
            `<span class="ancfvhome">home facet: <b>${e.home}</b></span>` +
            `<span class="ancfvnote">${e.note}</span></div>`,
        ).join("")}
        <p class="twnote">We can prove transport and check the cover condition; whether a human declared the footprint correctly stays with the auditor.</p>
      </div>`;

      this.querySelectorAll("[data-anchor]").forEach((s) =>
        s.addEventListener("click", () => {
          const i = +s.dataset.anchor;
          if (this.anchor !== i) {
            this.anchor = i;
            this.render();
          }
        }),
      );
      this.querySelectorAll("[data-fact]").forEach((b) =>
        b.addEventListener("click", () => {
          if (this.factKey !== b.dataset.fact) {
            this.factKey = b.dataset.fact;
            this.render();
          }
        }),
      );
      this.querySelectorAll(".ancrow[data-play]").forEach((row) =>
        row.addEventListener("click", () => {
          try {
            playChord(row.dataset.play.split(",").map(Number));
          } catch (_) {}
        }),
      );
    }
  },
);

// Three precedents for the larger story: shared pointing, addressed proving
// computation, and proof reuse across a deliberately chosen boundary of change.
const PRIORART_CARDS = [
  {
    k: "uri",
    era: "1990s · the web",
    who: "URIs",
    what: "a shared way to point",
    line: "Systems agreed how to name a resource without agreeing how to store or serve it.",
    accent: "var(--ink-dim)",
    read: "Agree how to point before agreeing how to use. URI comparison even has several levels, from exact strings through increasingly informed normalization.",
  },
  {
    k: "bittorrent",
    era: "2001 · file sharing",
    who: "BitTorrent",
    what: "fetch by hash, verify on arrival",
    line: "Ask strangers for content by its hash, then check what they send you.",
    accent: "var(--ink-dim)",
    read: "The address is the integrity check. Files are split into pieces with their own hashes, so a bad piece is caught locally rather than after the whole download.",
  },
  {
    k: "nix",
    era: "2003 · builds",
    who: "Nix",
    what: "the inputs decide the address",
    line: "A build is named by everything that went into it, so it can be shared and reused.",
    accent: "var(--ink-dim)",
    read: "Addressing build inputs makes results reproducible and cacheable across machines. The same instinct as a lockfile, at the granularity of every dependency.",
  },
  {
    k: "unison",
    era: "2019 · a language",
    who: "Unison",
    what: "definitions stored by hash",
    line: "Code is keyed by the hash of its structure, and names are a separate mapping on top.",
    accent: "var(--ink-dim)",
    read: "The same split Ix arrived at later: identity from the structure, names as metadata beside it. Renaming a definition therefore costs nothing and breaks nothing.",
  },
  {
    k: "lurk",
    era: "ongoing · proving",
    who: "Lurk",
    what: "addressed values inside the evaluator",
    line: "Values are addressed as the program runs, so storage, evaluation and proofs share one handle.",
    accent: "var(--ink-dim)",
    read: "Compound values are content-derived pointers inside evaluation, which is also what makes memoization and commitments fall out of the same mechanism.",
  },
  {
    k: "ix",
    era: "ongoing · Lean",
    who: "Ix",
    what: "a proof that follows the address",
    line: "Address a declaration with local names left out, attach its proof, and reuse the proof wherever that address turns up.",
    accent: "var(--ink-dim)",
    read: "The full chain: choose what does not matter, derive the address, attach a claim, reuse it. Their claims even carry the address of the assumption set they depend on.",
  },
];
const PRIORART_IDLE =
  "These systems separate the identity needed by a task from incidental representation details. Hover a card for the specific connection.";
customElements.define(
  "prior-art",
  class extends HTMLElement {
    connectedCallback() {
      const cards = PRIORART_CARDS.map(
        (c) =>
          `<div class="pacard" data-k="${c.k}" style="--pa:${c.accent}">` +
          `<div class="pa-era">${c.era}</div>` +
          `<div class="pa-who">${c.who}</div>` +
          `<div class="pa-what">${c.what}</div>` +
          `<p class="pa-line">${c.line}</p></div>`,
      ).join("");
      this.innerHTML = `
      <div class="paline" aria-hidden="true"></div>
      <div class="pagrid">${cards}</div>
      <div class="eqread" data-idle="${PRIORART_IDLE}">${PRIORART_IDLE}</div>
      <p class="twnote">Each fixes one address per thing; we propose several per artifact, each explicit about which differences it ignores.</p>`;
      const read = this.querySelector(".eqread");
      this.querySelectorAll(".pacard").forEach((card) => {
        const c = PRIORART_CARDS.find((x) => x.k === card.dataset.k);
        card.addEventListener("mouseenter", () => {
          this.querySelectorAll(".pacard").forEach((x) =>
            x.classList.toggle("dim", x !== card),
          );
          card.classList.add("on");
          read.innerHTML = `<span class="pa-sw" style="background:${c.accent}"></span><b>${c.who}</b> · ${c.read}`;
        });
      });
      this.addEventListener("mouseleave", () => {
        this.querySelectorAll(".pacard").forEach((x) =>
          x.classList.remove("dim", "on"),
        );
        read.innerHTML = read.dataset.idle;
      });
    }
  },
);

// "verified attestation: the proofs check" (Shape A).
//
// The polyphonotopes-math Lean development proves the pitch-class-set normal
// form is canonical: primeForm picks the unique representative of the
// transpose-and-invert (Tn/TnI) class, and primeForm is idempotent. These are
// assumption-free finite combinatorics, so a passing `lake build` means the
// Lean 4 kernel has CHECKED them (not "tested", checked). We content-address
// that checked proof and register it as a riffcat claim: an Attestation that
// keys a property onto a subject FacetAddress, plus a recorded FOOTPRINT facet,
// the set of dimensions the fact actually depends on. For a prime-form fact the
// footprint is the structure-only facet (the order-free pitch encoding up to
// Tn/TnI), so the claim transports soundly to every voicing, octave-doubling,
// and transposition that shares that structural anchor, and no finer.
//
// NOT buildable today: running `lake build` needs a Lean toolchain off in CI,
// and the content-addressed claim register (attestation(property, subjectFacet,
// footprintFacet)) is not wired onto window.wasmBindings yet. So this renders a
// STATIC attested badge as a placeholder. The two future calls are named below
// and tried first; when they exist the badge fills from the engine, otherwise
// it shows the baked attestation unchanged. Either way the trust tiers stay
// honest: the kernel-checked part, the named cryptographic assumption, and the
// open Lean-model-versus-Rust gap are three separate lines, never blurred.

// The baked attestation (placeholder until lake-in-CI + the claim register
// land). Hashes are illustrative content addresses, shaped like the engine's.
const ATTEST_RECORD = {
  property: "pcs-normal-form-canonical",
  toolchain: "Lean 4 · polyphonotopes-math",
  build: "lake build · kernel-checked",
  theorems: [
    {
      name: "primeForm_correct",
      says: "prime form is the unique representative of the transpose-and-invert class",
    },
    {
      name: "primeForm_idempotent",
      says: "normalizing a normal form changes nothing",
    },
  ],
  // the address the proof is content-addressed at (the Lean development's digest)
  claim: "b3:7e91f4c2a8d05b13",
  // the SUBJECT the property is asserted onto: a structure-only facet address
  subjectFacet: "structure",
  subjectAddr: "3a0f9c1d77e2",
  // the FOOTPRINT facet: the dimensions the fact depends on (its home facet).
  // For a prime-form fact this is exactly structure-only, so the claim rides
  // every voicing and transposition sharing that anchor, and nothing finer.
  footprintFacet: "structure",
};

// Honest trust tiers, kept distinct on purpose (the overclaim lives in blurring
// them). checked = the kernel verified it, assumption = named and not a theorem,
// open = the gap we do not paper over.
const ATTEST_TIERS = [
  {
    k: "checked",
    t: "kernel-checked",
    s: "the Lean 4 kernel verified these theorems; lake build passing means checked, not merely tested. Assumption-free finite combinatorics over the twelve pitch classes.",
  },
  {
    k: "assumption",
    t: "named assumption",
    s: "no two distinct shapes collide in the content address: a stated cryptographic assumption on the digest, not a theorem. Named, not hidden.",
  },
  {
    k: "open",
    t: "open gap",
    s: "every theorem is about the Lean model of the encoder; the running Rust engine agrees by golden vectors and reading, not yet by verified extraction. We state this as integrity, up front.",
  },
];

const ATTEST_SHORT = (h) => (h || "").slice(0, 12);

customElements.define(
  "verified-attest",
  class extends HTMLElement {
    async connectedCallback() {
      this.rec = ATTEST_RECORD;
      // Future path: if the engine ever exposes a content-addressed claim register
      // and a lake status, fill the badge from it. Both are await-safe to miss.
      try {
        const b = await engineReady;
        if (b && typeof b.attestation === "function") {
          const live = JSON.parse(
            b.attestation(
              ATTEST_RECORD.property,
              ATTEST_RECORD.subjectFacet,
              ATTEST_RECORD.footprintFacet,
            ),
          );
          // expected shape: { claim, subjectAddr, build, checked: bool }
          if (live && live.claim) {
            this.rec = Object.assign({}, ATTEST_RECORD, live);
          }
        }
      } catch (_) {
        /* baked placeholder stands; this is a static-attested view */
      }
      this.live = !!(
        window.wasmBindings &&
        typeof window.wasmBindings.attestation === "function"
      );
      this.render();
    }
    render() {
      const r = this.rec;
      const checkClass = this.live ? "ok" : "static";
      const checkText = this.live
        ? "registered live"
        : "attested (static placeholder)";
      const theorems = r.theorems
        .map(
          (th) =>
            `<div class="vattest-thm" data-read="thm" data-name="${th.name}" data-says="${th.says}">` +
            `<span class="vattest-tick">checks</span><code>${th.name}</code></div>`,
        )
        .join("");
      const tiers = ATTEST_TIERS.map(
        (tier) =>
          `<span class="vattest-tier vattest-${tier.k}" data-read="tier" data-key="${tier.k}">${tier.t}</span>`,
      ).join("");
      const idle =
        `<b>${r.theorems.length}</b> theorems kernel-checked · property <code>${r.property}</code>` +
        ` anchored at the <b>${r.footprintFacet}</b> facet · the claim rides every voicing and transposition sharing that anchor, and no finer`;
      this.innerHTML = `
      <div class="vattest-badge ${checkClass}">
        <div class="vattest-seal" data-read="seal" title="content address of the checked proof">
          <span class="vattest-glyph">✓</span>
          <span class="vattest-hash">${ATTEST_SHORT(r.claim)}</span>
        </div>
        <div class="vattest-body">
          <div class="vattest-head">
            <span class="vattest-prop">${r.property}</span>
            <span class="vattest-state ${checkClass}">${checkText}</span>
          </div>
          <div class="vattest-tool">${r.toolchain} · ${r.build}</div>
          <div class="vattest-thms">${theorems}</div>
        </div>
      </div>
      <div class="vattest-claim" data-read="claim">
        <div class="vattest-crow"><span class="vattest-ck">property</span><span class="vattest-cv">${r.property}</span></div>
        <div class="vattest-crow"><span class="vattest-ck">subject facet</span><span class="vattest-cv">${r.subjectFacet} <span class="vattest-addr">${r.subjectAddr}</span></span></div>
        <div class="vattest-crow"><span class="vattest-ck">footprint facet</span><span class="vattest-cv">${r.footprintFacet}</span></div>
        <div class="vattest-crow"><span class="vattest-ck">claim address</span><span class="vattest-cv vattest-cclaim">${r.claim}</span></div>
      </div>
      <div class="vattest-tiers" data-read="tiers">${tiers}</div>
      <div class="eqread" data-idle="${idle}">${idle}</div>
      <p class="twnote">This is one rung up from the rest of the tour. Everywhere else riffcat shows you a match; here a verifier proves something about the map riffcat uses, that prime form is the true canonical form of the transpose-and-invert class, and the Lean 4 kernel checks it. We record that proof as a content-addressed claim and note the one facet its footprint covers, so it transports exactly as far as it is sound to and stops there. Running <code>lake build</code> needs a Lean toolchain, and the claim register is not wired yet, so the badge above is a static attestation standing in for the live one.</p>`;
      const read = this.querySelector(".eqread");
      const idleHTML = read.dataset.idle;
      const describe = (el) => {
        const kind = el.dataset.read;
        if (kind === "thm")
          return `<b>${el.dataset.name}</b> · ${el.dataset.says} · kernel-checked, assumption-free`;
        if (kind === "seal")
          return `<b>content address</b> · the digest of the checked proof; sharing this address is what lets the claim transport without re-proving`;
        if (kind === "claim")
          return `<b>the claim shape</b> · a property keyed onto a subject facet address, with a recorded footprint facet; transport is sound only when the anchor facet covers the footprint`;
        if (kind === "tier") {
          const tier = ATTEST_TIERS.find((x) => x.k === el.dataset.key);
          return `<b>${tier.t}</b> · ${tier.s}`;
        }
        if (kind === "tiers")
          return `three tiers, kept apart on purpose: <b>checked</b> by the kernel, an honestly <b>named assumption</b>, and the <b>open gap</b> between the Lean model and the Rust engine`;
        return idleHTML;
      };
      this.querySelectorAll("[data-read]").forEach((el) =>
        el.addEventListener("mouseenter", () => {
          read.innerHTML = describe(el);
        }),
      );
      this.addEventListener("mouseleave", () => {
        read.innerHTML = idleHTML;
      });
    }
  },
);

// The Forte-catalog chapter: a small catalog of named chords, each parsed from
// real notation and fingerprinted live (fingerprint_chord; the inverted sets go
// back through the engine via fingerprint_pcs), shown with its Tn-type, prime
// form, interval vector, and Forte-style set-class label. At the set-class stop
// the cards group by the A/B-distinguished Tn-type, the standard convention:
// the two major triads collapse to one 3-11B, the minor triad stands apart as
// its mirror 3-11A, and the explicit invert operation is the move between the
// two. The prime form on each card is the further "fold inversion" rung, where
// an A/B pair merges (3-11) and where the dominant seventh (4-27B) meets the
// half-diminished type (4-27A). The point of the page: riffcat lands on the
// published catalog from structure alone, the same dial the code chapters use.
//
// Forte numbers are a fixed, published fact about each prime form (Forte 1973),
// not something the engine emits, so they live here as a tiny lookup keyed by
// the engine's own prime_form output. Everything that moves (pitch classes,
// Tn-type, prime form, interval vector, the A/B letter, the symmetry, the
// addresses that drive grouping and color) comes straight from the engine.
// crates/riff-catalog-music/tests/demo_forte_catalog.rs replays this page's
// derivation in Rust and pins every rendered label to the published catalog.

// The catalog, in notation the vibe-grammars parser already reads. Each entry is
// (notation, the name a musician would say). Two major triads on purpose: C and
// G share a Tn-type (transposition already folded), so the grouping gesture has
// a real collapse to show while Am sits apart as the mirror. The engine does
// the rest.
const FORTE_CHORDS = [
  ["C", "major triad"],
  ["G", "major triad, a fifth up"],
  ["Am", "minor triad"],
  ["Caug", "augmented triad"],
  ["Bdim", "diminished triad"],
  ["Cmaj7", "major seventh"],
  ["G7", "dominant seventh"],
  ["Am7", "minor seventh"],
];

// Two facets, loosest last: the literal note set, then the set-class stop,
// which groups at the A/B-distinguished Tn-type (the standard catalog's own
// resolution) and annotates each card with the prime form it would further fold
// to. The page opens on set_class because that is where the catalog appears.
const FORTE_FACETS = [
  ["note_set", "note set"],
  ["set_class", "set class"],
];

// The ONE published fact the engine cannot derive from structure: Forte's NAME
// for a prime form (Forte 1973). Everything else on this page (which chords
// share a class, the Tn-type, the prime form, the interval vector, the A/B
// letter, the symmetry, the colors) is computed live by the engine. We key this
// tiny table on the engine's own prime_form output, so it names exactly what the
// engine canonicalizes to, and nothing more. (Names checked against Wikipedia's
// List of set classes, 2026-06-24.)
const FORTE_NAMES = {
  "0 3 7": "3-11", // major / minor triad
  "0 4 8": "3-12", // augmented triad (symmetric)
  "0 3 6": "3-10", // diminished triad (symmetric)
  "0 1 5 8": "4-20", // major seventh (symmetric)
  "0 3 5 8": "4-26", // minor seventh (symmetric)
  "0 2 5 8": "4-27", // dominant / half-diminished seventh
};

// The Forte label for a chord, DERIVED from the engine's own output, not
// declared by us. `fp` is the engine fingerprint of the set as it is shown;
// `fpInv` is the engine fingerprint of its inversion.
//   base   = the published name of the prime form (the one lookup above)
//   sym    = inversionally symmetric: the set and its inversion share a Tn-type,
//            so A and B coincide and there is no letter
//   letter = "A" when the shown Tn-type already IS the prime form (the A side),
//            "B" when it is the inverted side; "" when symmetric
// Because prime_form is the TnI canonical form, fp and fpInv share it, so
// folding inversion (the prime form) is where the A/B pair merges.
function forteLabel(fp, fpInv) {
  const primeKey = fp.prime_form.join(" ");
  const base = FORTE_NAMES[primeKey];
  if (!base) return null;
  const tnf = fp.transposition_normal_form.join(" ");
  const sym = tnf === fpInv.transposition_normal_form.join(" ");
  const letter = sym ? "" : tnf === primeKey ? "A" : "B";
  return { id: base + letter, fold: base, sym, letter };
}

// Forte's angle-bracket spelling of an interval vector (the six interval-class
// counts), rendered straight from the engine's live count.
const forteIvText = (iv) =>
  "<" + iv.map((n) => (n > 9 ? "X" : String(n))).join("") + ">";

customElements.define(
  "forte-catalog",
  class extends HTMLElement {
    async connectedCallback() {
      this.facet = "set_class";
      this.inverted = false; // the INVERT operation: flip each chord by the I generator
      this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
      try {
        const b = await engineReady;
        this.data = FORTE_CHORDS.map(([notation, name]) => {
          try {
            const fp = JSON.parse(b.fingerprint_chord(notation));
            // The inverted chord, addressed by the SAME engine (fingerprint_pcs
            // with the invert flag): its Tn-type, prime form, and interval
            // vector all come from here, so the demo never does pitch-class math.
            const fpInv = JSON.parse(
              b.fingerprint_pcs(JSON.stringify(fp.pitch_classes), true),
            );
            return { notation, name, fp, fpInv };
          } catch {
            return null;
          }
        }).filter(Boolean);
        this.render();
      } catch (e) {
        this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`;
      }
    }
    // The engine fingerprint a card currently shows, and its counterpart: the
    // parsed chord and its inversion, swapping when the INVERT operation is on.
    // Both are engine output; the A/B letter is derived from the pair, never baked.
    shownFp(d) {
      return this.inverted ? d.fpInv : d.fp;
    }
    otherFp(d) {
      return this.inverted ? d.fp : d.fpInv;
    }
    // The pitch classes, Tn-type, and Forte label a card currently shows, all read
    // straight off the engine output.
    pcsFor(d) {
      return this.shownFp(d).pitch_classes;
    }
    tnfFor(d) {
      return this.shownFp(d).transposition_normal_form;
    }
    forteFor(d) {
      return forteLabel(this.shownFp(d), this.otherFp(d));
    }
    render() {
      const ladder = FORTE_FACETS.map(
        ([k, label]) =>
          `<span class="stop ${k === this.facet ? "on" : ""}" data-facet="${k}">${label}</span>`,
      ).join("");
      const onClass = this.facet === "set_class";

      // The card's address for grouping/color, both engine facet addresses: at
      // set_class we key on the Tn-type's content address (transposition_normal),
      // the A/B-distinguished resolution, so major and minor get DIFFERENT colors
      // (the whole point of A vs B); at note_set we key on the literal note-set
      // hex. The prime form (the inversion fold) is shown per card as the further
      // rung.
      const addrFor = (d) =>
        onClass
          ? this.shownFp(d).transposition_normal
          : this.shownFp(d).note_set;

      // Group by that address: at set_class this groups by Tn-type, so a 3-11A and
      // a 3-11B are two groups, and folding inversion (the prime form) is the
      // further step that would merge them, called out in the readout and caption.
      const groups = new Map(); // address -> [shown notations]
      for (const d of this.data) {
        const a = addrFor(d);
        if (!groups.has(a)) groups.set(a, []);
        groups.get(a).push(this.inverted ? `inv(${d.notation})` : d.notation);
      }

      const cards = this.data
        .map((d, i) => {
          const pcs = this.pcsFor(d);
          const tnf = this.tnfFor(d);
          const a = addrFor(d);
          const cls = "fcat-" + a.replace(/[^a-zA-Z0-9]/g, "");
          const col = chipColor(a);
          const notes = pcs
            .map((pc) => `<span class="nchip">${NOTE_NAMES[pc]}</span>`)
            .join("");
          const sfp = this.shownFp(d);
          const f = this.forteFor(d);
          const forteTag =
            onClass && f ? `<span class="fcat-num">${f.id}</span>` : "";
          // The A/B form (the Tn-type) is the headline at set_class; the prime form
          // is the further "fold inversion" rung where the A/B pair would merge.
          // Both Tn-type and prime form are the engine's, the fold name is derived.
          const aform = onClass
            ? `<span class="fcat-pf">${f && f.sym ? "Tn-type = prime" : "Tn-type"} [${tnf.join(" ")}]` +
              `${f && !f.sym ? ` <em>fold inversion → ${f.fold} prime [${sfp.prime_form.join(" ")}]</em>` : ""}</span>`
            : "";
          // interval vector: the engine's live count, in Forte's angle-bracket
          // spelling. It is invariant under transposition and inversion, so it is
          // exactly the published vector for the class (the prose says as much).
          const iv = `<span class="fcat-iv">iv ${forteIvText(sfp.interval_vector)}</span>`;
          const shownName = this.inverted ? `inv(${d.notation})` : d.notation;
          return (
            `<div class="fcatcard ${cls}" data-eq="${cls}" style="--chip:${col}">` +
            `<div class="fcat-h">` +
            `<button class="playbtn" data-i="${i}">▶</button>` +
            `<span class="shapedot" style="--chip:${col}" title="${onClass ? "Tn-type " + tnf.join(",") : "note set " + a.slice(0, 10)}">${chipSymbol(a)}</span>` +
            `<span class="fcat-name"><b>${shownName}</b> <span class="vsub">${d.name}</span></span>` +
            `${forteTag}</div>` +
            `<div class="fcat-notes"><span class="nchips">${notes}</span></div>` +
            `<div class="fcat-meta">${aform}${iv}</div></div>`
          );
        })
        .join("");

      const n = groups.size;
      const gtxt = [...groups.values()]
        .map((names) =>
          names.length > 1 ? `<b>${names.join(" = ")}</b>` : names[0],
        )
        .join(" · ");
      const idle = onClass
        ? `<b>${n}</b> Tn-types (A/B-distinguished) across ${this.data.length} chords${this.inverted ? ", inverted" : ""}` +
          ` · ${gtxt} · folding inversion would merge each A with its B · live, in your browser`
        : `<b>${n}</b> note sets across ${this.data.length} chords · each chord its own notes`;

      const note = onClass
        ? `In the <b>standard</b> set-class convention, inversionally related sets stay distinct and carry a letter: the <b>major</b> triad is <b>3-11B</b> (Tn-type [0,4,7]) and the <b>minor</b> triad is <b>3-11A</b> (Tn-type [0,3,7]). Transposition is already folded, so C and G share one 3-11B address; inversion is the remaining move, the one between A and B. Hit <b>invert</b> above and watch the major triads (3-11B) land on the minor type 3-11A, and the dominant seventh (4-27B) land on the half-diminished 4-27A; the inversionally symmetric ones (augmented <b>3-12</b>, diminished <b>3-10</b>, major seventh <b>4-20</b>, minor seventh <b>4-26</b>) carry no letter and invert to themselves. The further <b>fold inversion</b> rung is the TnI <b>prime form</b> shown on each card, where A and B merge: 3-11A and 3-11B both become the unlettered <b>3-11</b> [0,3,7]. riffcat is not given the catalog; the Tn-type and prime form are computed from the structure, then content-addressed by the same facet machinery the code chapters use (<code>fingerprint_chord</code>). The interval vectors it counts match Forte's published ones, line for line.`
        : `At the <b>note-set</b> facet each chord is just its set of pitches, so every one of these eight is its own address. Loosen the dial to <b>set class</b> and the A/B-distinguished Forte numbers appear; the four inversionally symmetric chords carry no letter.`;

      this.innerHTML = `
      <div class="dialbar">
        <div class="grp"><span>facet</span><div class="ladder">${ladder}</div></div>
        <div class="grp"><span>operation</span>
          <button data-op="invert" aria-pressed="${this.inverted}" ${onClass ? "" : "disabled"}>invert (I)</button>
        </div>
      </div>
      <div class="fcatgrid">${cards}</div>
      <div class="eqread" data-idle="${idle}">${idle}</div>
      <p class="twnote">${note}</p>`;

      this.querySelectorAll("[data-facet]").forEach((s) =>
        s.addEventListener("click", () => {
          if (this.facet !== s.dataset.facet) {
            this.facet = s.dataset.facet;
            this.render();
          }
        }),
      );
      this.querySelectorAll("[data-op='invert']").forEach((btn) =>
        btn.addEventListener("click", () => {
          this.inverted = !this.inverted;
          this.render();
        }),
      );
      // Play the chord as currently shown: inverted pitch classes when invert is on,
      // so the operation is audible, not just labeled.
      this.querySelectorAll(".playbtn").forEach((btn) =>
        btn.addEventListener("click", () =>
          playChord(this.pcsFor(this.data[+btn.dataset.i])),
        ),
      );

      // Hover a card to light every chord in the same Tn-type and name them in the
      // readout: the collapse made legible, exactly the facet-primer gesture.
      this.querySelectorAll(".fcatcard").forEach((card) => {
        card.addEventListener("mouseenter", () => {
          const kin = this.querySelectorAll("." + card.dataset.eq);
          this.querySelectorAll(".fcatcard").forEach((x) =>
            x.classList.add("dim"),
          );
          kin.forEach((x) => {
            x.classList.remove("dim");
            x.classList.add("lit");
          });
          const read = this.querySelector(".eqread");
          if (!read) return;
          const names = [...kin].map((x) =>
            x.querySelector(".fcat-name b").textContent.trim(),
          );
          read.innerHTML =
            kin.length > 1
              ? `<b>${kin.length}</b> share this ${onClass ? "Tn-type (same A/B form)" : "note set"}: <b>${names.join(" = ")}</b>`
              : `<b>${names[0]}</b> is alone at the ${onClass ? "Tn-type (its own A/B form)" : "note-set"} facet`;
        });
        card.addEventListener("mouseleave", () => {
          this.querySelectorAll(".fcatcard").forEach((x) =>
            x.classList.remove("dim", "lit"),
          );
          const read = this.querySelector(".eqread");
          if (read) read.innerHTML = read.dataset.idle || "";
        });
      });
    }
  },
);

// Interval-vector fingerprint: a chord's six interval-class counts shown as a
// compact strip of tiny bars, computed live by the same engine (fingerprint_chord,
// interval_vector field). The interval vector is a projection facet: it counts
// the unordered intervals present and forgets register, voicing, and root, so it
// is invariant under transposition and inversion. Two chords with the same vector
// share one harmonic signature; C major and A minor are the standing example
// (both set class 3-11), so picking one highlights the other.
// Reuses .dialbar/.ladder/.stop/.riffs/.riffrow/.riffname/.nchips/.nchip/.playbtn/
// .shapedot/.eqread/.twnote/.vsub and the chipColor + engineReady + playChord
// helpers already in app.js; the bar strip is the only genuinely new visual.
const IVF_CHORDS = ["C", "Am", "Cmaj7", "G7", "Caug", "Cdim"];
// The six interval classes, shortest first. ic6 (the tritone) is the lone
// self-inverse interval, which is part of why the vector survives inversion.
const IVF_IC = [
  ["ic1", "m2", "minor 2nd / major 7th"],
  ["ic2", "M2", "major 2nd / minor 7th"],
  ["ic3", "m3", "minor 3rd / major 6th"],
  ["ic4", "M3", "major 3rd / minor 6th"],
  ["ic5", "P4", "perfect 4th / perfect 5th"],
  ["ic6", "TT", "the tritone, its own inverse"],
];
// A vector keys an equivalence class: stringify it so same-signature chords match.
const ivfKey = (vec) => vec.join("-");
const ivfTotal = (vec) => vec.reduce((a, b) => a + b, 0);

customElements.define(
  "ivf-fingerprint",
  class extends HTMLElement {
    async connectedCallback() {
      this.sel = 0;
      this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
      try {
        const b = await engineReady;
        this.data = IVF_CHORDS.map((c) => {
          try {
            return { c, fp: JSON.parse(b.fingerprint_chord(c)) };
          } catch {
            return null;
          }
        }).filter(Boolean);
        if (!this.data.length) throw new Error("no chord parsed");
        this.render();
      } catch (e) {
        this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`;
      }
    }
    // Tiny bar strip for one interval vector. The widest count present sets the
    // full-height bar, so a vector reads as a shape at a glance; an empty class is
    // a faint floor, never a gap. Bars are colored from the vector's own class
    // color, so two chords with the same signature also wear the same color.
    strip(vec, color, big) {
      const peak = Math.max(1, ...vec);
      const cls = big ? "ivf-strip ivf-big" : "ivf-strip";
      const bars = vec
        .map((n, i) => {
          const h = Math.round(12 + (big ? 40 : 22) * (n / peak));
          const on = n > 0 ? "on" : "off";
          return (
            `<span class="ivf-col" title="${IVF_IC[i][2]}: ${n}">` +
            `<span class="ivf-bar ${on}" style="height:${h}px;--ivc:${color}"></span>` +
            `<span class="ivf-n">${n}</span>` +
            (big ? `<span class="ivf-lab">${IVF_IC[i][1]}</span>` : "") +
            `</span>`
          );
        })
        .join("");
      return `<span class="${cls}">${bars}</span>`;
    }
    render() {
      // Group by interval vector: every chord sharing a signature is one class.
      const groups = new Map();
      for (const d of this.data) {
        const k = ivfKey(d.fp.interval_vector);
        if (!groups.has(k)) groups.set(k, []);
        groups.get(k).push(d.c);
      }
      const rows = this.data
        .map((d, i) => {
          const vec = d.fp.interval_vector;
          const color = chipColor(d.fp.set_class);
          const notes = d.fp.pitch_classes
            .map((pc) => `<span class="nchip">${NOTE_NAMES[pc]}</span>`)
            .join("");
          return (
            `<div class="riffrow ivf-row ${i === this.sel ? "on" : ""}" data-i="${i}">` +
            `<button class="playbtn" data-i="${i}">▶ play</button>` +
            `<span class="riffname">${d.c}</span>` +
            `<span class="nchips">${notes}</span>` +
            this.strip(vec, color, false) +
            `<span class="ivf-vec"><${""}${vec.join("")}></span>` +
            `<span class="shapedot" style="--chip:${color}" title="interval vector &lt;${vec.join("")}&gt;">${chipSymbol(d.fp.set_class)}</span></div>`
          );
        })
        .join("");
      const sel = this.data[this.sel];
      const svec = sel.fp.interval_vector;
      const skey = ivfKey(svec);
      const share = (groups.get(skey) || []).filter((c) => c !== sel.c);
      const selColor = chipColor(sel.fp.set_class);
      const total = ivfTotal(svec);
      // The detail panel: the chosen chord's six-count signature at full size, the
      // angle-bracket vector notation set theory writes, and an honest line about
      // who else shares it. The vector is a projection; it forgets ordering and
      // register, so it cannot tell two chords apart that happen to share counts.
      const shareLine = share.length
        ? `the same vector as <b>${share.join(", ")}</b>: same harmonic content, transposition and inversion aside`
        : `unique among these chords at this projection`;
      const detail =
        `<div class="ivf-detail">` +
        `<div class="ivf-dh"><b>${sel.c}</b> · interval vector <span class="ivf-bra">&lt;${svec.join(" ")}&gt;</span>` +
        ` <span class="vsub">${total} interval${total === 1 ? "" : "s"} in all</span></div>` +
        this.strip(svec, selColor, true) +
        `<div class="ivf-share">${shareLine}</div></div>`;
      const n = groups.size;
      const gtxt = [...groups.values()]
        .map((cs) => (cs.length > 1 ? `<b>${cs.join(" = ")}</b>` : cs[0]))
        .join(" · ");
      this.innerHTML =
        `
      <div class="dialbar"><div class="grp"><span>chord</span><div class="ladder">` +
        this.data
          .map(
            (d, i) =>
              `<span class="stop ${i === this.sel ? "on" : ""}" data-i="${i}">${d.c}</span>`,
          )
          .join("") +
        `</div></div></div>
      <div class="riffs">${rows}</div>
      ${detail}
      <div class="eqread"><b>${n}</b> distinct signature${n === 1 ? "" : "s"} among ${this.data.length} chords · ${gtxt}</div>
      <p class="twnote">The interval vector counts the unordered intervals inside a chord, one bin per interval class from the minor second up to the tritone. It keeps no order, no octave, no root, so it is a <b>transposition- and inversion-invariant</b> harmonic signature: shift the chord to any key or turn it upside down and the six numbers hold. That is why <b>C major and A minor</b> share one vector here (both are set class 3-11). It is a projection, honestly partial: it can tell you two chords have the same interval content, not that they are the same chord. The engine reads it straight off the structure (<code>fingerprint_chord</code>).</p>`;
      this.querySelectorAll(".stop[data-i]").forEach((s) =>
        s.addEventListener("click", () => this.pick(+s.dataset.i)),
      );
      this.querySelectorAll(".ivf-row").forEach((r) =>
        r.addEventListener("click", (e) => {
          if (!e.target.closest(".playbtn")) this.pick(+r.dataset.i);
        }),
      );
      this.querySelectorAll(".playbtn").forEach((btn) =>
        btn.addEventListener("click", (e) => {
          e.stopPropagation();
          playChord(this.data[+btn.dataset.i].fp.pitch_classes);
        }),
      );
    }
    pick(i) {
      if (i !== this.sel && this.data[i]) {
        this.sel = i;
        this.render();
      }
    }
  },
);

// The three rungs of the structural ladder on a pitch-class set, on one chord.
// Each rung is a looser facet: note_set (the literal pitch classes) -> the
// transposition normal form (the same set rotated to its minimal, tightest-packed
// reading, transposition forgotten) -> set_class / prime form (inversion folded in
// too). We climb one chord up the ladder and name what each rung drops.
//
// The middle rung reads the engine's transposition_normal address and
// transposition_normal_form reading (the minimal-rotation, transposition-
// invariant canonical form matching polyphonotopes-math's normalFormBits). The
// Forte badges on the rungs are DERIVED per forteLabel from the engine's output
// on the chord and its inversion (fingerprint_pcs), never baked.
const TR_RUNGS = [
  {
    key: "note_set",
    rung: "the literal notes",
    forgets: "nothing yet: the actual pitch classes, exactly as written",
    keeps: "register folded to one octave; the bare set of pitch classes",
  },
  {
    key: "transposition_normal",
    rung: "transposition forgotten",
    forgets:
      "where the set sits: every transposition reads as one tightest-packed rotation",
    keeps:
      "the inside spacing AND the handedness: the A/B-distinguished Tn-type, still apart from its mirror",
  },
  {
    key: "set_class",
    rung: "fold inversion",
    forgets:
      "the mirror as well: a shape and its inversion share one prime form, so the A and B types merge and the letter drops",
    keeps: "only the interval content: Allen Forte's catalog address",
  },
];
// A chord whose three rungs are all visibly distinct, so the ladder reads as a
// climb and not a collapse. G major works: written as {D,G,B} the literal set is
// not its own tightest rotation (rung 1 != rung 2), and a major triad is not
// inversion-symmetric, so the prime form folds it further (rung 2 != rung 3). Its
// Tn-type is [0,4,7] (3-11B), and folding inversion lands it on the prime form
// [0,3,7], the unlettered 3-11 class the set-class chapters land on. (Any
// replacement chord needs a FORTE_NAMES entry for its prime form: the rung
// badges are derived via forteLabel.)
const TR_CHORD = "G";
// Render the chosen rung's address as a small set of pitch-class chips, using the
// engine's own field for that rung. The literal set always shows; the normal-form
// rung shows the rotated reading; the set-class rung shows the prime form.
const trReading = (fp, key) => {
  if (key === "set_class") return fp.prime_form;
  if (key === "transposition_normal") return fp.transposition_normal_form;
  return fp.pitch_classes;
};
const trPcLabel = (pc) =>
  ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"][
    ((pc % 12) + 12) % 12
  ];
customElements.define(
  "three-rungs",
  class extends HTMLElement {
    async connectedCallback() {
      this.rung = "note_set";
      this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
      try {
        const b = await engineReady;
        this.fp = JSON.parse(b.fingerprint_chord(TR_CHORD));
        // The chord's inversion, back through the same engine: forteLabel derives
        // the rung badges (the A/B id and the fold name) from the pair.
        this.fpInv = JSON.parse(
          b.fingerprint_pcs(JSON.stringify(this.fp.pitch_classes), true),
        );
        this.label = forteLabel(this.fp, this.fpInv);
        this.labelInv = forteLabel(this.fpInv, this.fp);
        this.render();
      } catch (e) {
        this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`;
      }
    }
    addrFor(key) {
      if (key === "set_class") return this.fp.set_class;
      if (key === "transposition_normal") return this.fp.transposition_normal;
      return this.fp.note_set;
    }
    // The Forte badge for a rung, derived from the engine pair: the Tn rung wears
    // the A/B-lettered id, the set-class rung the unlettered fold name.
    forteFor(key) {
      if (!this.label) return "";
      if (key === "transposition_normal") return this.label.id;
      if (key === "set_class") return this.label.fold;
      return "";
    }
    render() {
      const fp = this.fp;
      const ladder = TR_RUNGS.map(
        (r) =>
          `<span class="stop ${r.key === this.rung ? "on" : ""}" data-rung="${r.key}">${r.rung}</span>`,
      ).join("");
      // The chord climbs the same ladder; each rung is a row, lit when selected,
      // showing its address-color dot and the set reading the engine returns there.
      const rows = TR_RUNGS.map((r, i) => {
        const addr = this.addrFor(r.key);
        const reading = trReading(fp, r.key);
        const chips = reading
          .map((pc) => `<span class="nchip">${trPcLabel(pc)}</span>`)
          .join("");
        const on = r.key === this.rung;
        // the named step INTO this rung: the climb from rung 2 to rung 3 is "fold
        // inversion", the move that merges the A/B pair, so we label it explicitly.
        const stepTag =
          r.key === "set_class"
            ? `<span class="fcat-num" title="the step that merges ${this.label.id} and ${this.labelInv.id}">fold inversion ↑</span>`
            : "";
        const forte = this.forteFor(r.key);
        const forteTag = forte ? `<span class="fcat-num">${forte}</span>` : "";
        return (
          `<div class="rungrow ${on ? "on" : ""}" data-rung="${r.key}">` +
          `<span class="rung-ix">${i + 1}</span>` +
          `<span class="rung-name">${r.rung}</span>` +
          `<span class="nchips">${chips}</span>` +
          `${stepTag}${forteTag}` +
          `<button class="playbtn" data-i="${i}">▶ hear it</button>` +
          `<span class="shapedot" style="--chip:${chipColor(addr)}" title="${r.key} ${addr.slice(0, 10)}">${chipSymbol(addr)}</span></div>`
        );
      }).join("");
      const sel = TR_RUNGS.find((r) => r.key === this.rung);
      const selForte = this.forteFor(sel.key);
      const forteNote = selForte ? ` · Forte <b>${selForte}</b>` : "";
      const idle = `rung <b>${TR_RUNGS.indexOf(sel) + 1}</b> of 3${forteNote} · forgets ${sel.forgets} · keeps ${sel.keeps}`;
      const tnf = fp.transposition_normal_form.join(",");
      const tnfInv = this.fpInv.transposition_normal_form.join(",");
      const pf = fp.prime_form.join(",");
      this.innerHTML = `
      <div class="dialbar"><div class="grp"><span>climb</span><div class="ladder">${ladder}</div></div></div>
      <div class="riffs rungs">${rows}</div>
      <div class="eqread" data-idle="${idle}">${idle}</div>
      <p class="twnote">One chord (<b>${TR_CHORD}</b> major), three readings of "the same." The first rung is the literal set of pitch classes. The second slides the set to its tightest-packed rotation, so every transposition reads alike: the same move a listener makes hearing a riff moved up a fifth as still the riff. This is the <b>Tn-type</b>, the A/B-distinguished form: G major's is [${tnf}], Forte's <b>${this.label.id}</b>, and it is still apart from its mirror, the minor type <b>${this.labelInv.id}</b> [${tnfInv}]. The third rung is the explicit <b>fold inversion</b> step: it forgets the mirror too and lands on the TnI <b>prime form</b> [${pf}], where ${this.label.id} and ${this.labelInv.id} merge into the unlettered <b>${this.label.fold}</b>, Forte's catalog address. Each step up forgets exactly one more thing, and the top rung is honest that it folds inversion: equal at a lower rung is always equal at every rung above it.</p>`;
      this.querySelectorAll("[data-rung]").forEach((el) =>
        el.addEventListener("click", () => {
          const k = el.dataset.rung;
          if (k && this.rung !== k) {
            this.rung = k;
            this.render();
          }
        }),
      );
      this.querySelectorAll(".playbtn").forEach((btn) =>
        btn.addEventListener("click", (e) => {
          e.stopPropagation();
          const r = TR_RUNGS[+btn.dataset.i];
          playChord(trReading(this.fp, r.key));
        }),
      );
    }
  },
);

// "two fingerprints" chapter: the compiler's metadata hash (byte-exact, flips on
// any character change) set beside riffcat's structural fingerprint (built to
// survive the edits that move the metadata hash but keep the shape). This is an
// illustrative compare, not a live recompile: the metadata-hash stand-in is a
// real digest over the exact source text (so it genuinely flips on a whitespace),
// and the structural fingerprint is shown as the shape it would resolve to and
// holds across the same edits. The live structural compute lives in the
// "drive the dial" chapter; here the point is the contrast of the two axes.
//
// Source quote (Sourcify docs, "Finding Auxdatas in the Bytecode", 2024-02-12):
// the metadata hash "acts as a fingerprint of the compilation ... the slightest
// change in the compiler settings or even a whitespace in any of the source
// files will cause a change in the metadata hash." This is a docs/org statement,
// so it is attributed to Sourcify, not an individual. The Main-vs-Meta framing
// the next chapter uses is @kuzdogan's, in argotorg/sourcify #1643.

// One baseline function and three edits that each move the metadata hash. The
// structural fingerprint is deliberately insensitive to all three: whitespace is
// not part of the shape, a rename drops out at names-blind, a constant churn
// drops out once constants are set aside. Each edit returns the edited source so
// the metadata-hash stand-in can be recomputed over the literal text.
const MHA_BASE = `function previewMint(uint256 shares) public view returns (uint256) {
    return _convertToAssets(shares, Math.Rounding.Up);
}`;
const MHA_EDITS = [
  {
    key: "none",
    label: "original",
    note: "the verified source, as compiled",
    apply: (s) => s,
  },
  {
    key: "ws",
    label: "+ a whitespace",
    note: "one blank line added; nothing else touched",
    apply: (s) => s.replace("public view", "public  view"),
  },
  {
    key: "rename",
    label: "rename a parameter",
    note: "shares becomes amount throughout",
    apply: (s) => s.replace(/shares/g, "amount"),
  },
  {
    key: "const",
    label: "change a constant",
    note: "rounding direction flipped Up to Down",
    apply: (s) => s.replace("Math.Rounding.Up", "Math.Rounding.Down"),
  },
];

// A small, dependency-free digest over the LITERAL text, shown as a stand-in for
// the metadata hash. It is not the compiler's hash; it is here only to flip
// honestly on any character change, exactly as the metadata hash does. FNV-1a,
// rendered as 12 hex chars to read like one.
function mhaTextHash(s) {
  let h = 0x811c9dc5 >>> 0;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  // widen to a longer-looking digest by folding a second pass with a salt
  let g = 0x9e3779b1 ^ h;
  for (let i = s.length - 1; i >= 0; i--) {
    g ^= s.charCodeAt(i);
    g = Math.imul(g, 0x85ebca77) >>> 0;
  }
  return (
    h.toString(16).padStart(8, "0") + g.toString(16).padStart(8, "0")
  ).slice(0, 12);
}

// The structural fingerprint is the SAME across every edit here, because none of
// the three edits change the shape at names-and-constants-blind. A fixed digest
// over the structure stands in for it; it is what the live engine resolves this
// function to at the structure facet, and it does not move below.
const MHA_STRUCT = "9b2f04c7d1ae";

customElements.define(
  "metadata-axes",
  class extends HTMLElement {
    connectedCallback() {
      this.sel = 0;
      this.render();
    }
    render() {
      const cur = MHA_EDITS[this.sel];
      const src = cur.apply(MHA_BASE);
      const baseSrc = MHA_BASE;
      const meta = mhaTextHash(src);
      const metaBase = mhaTextHash(baseSrc);
      const metaMoved = meta !== metaBase;
      const structMoved = false; // by construction: none of these edits change the shape

      const tabs = MHA_EDITS.map(
        (e, i) =>
          `<button class="mha-tab ${i === this.sel ? "on" : ""}" data-i="${i}">${e.label}</button>`,
      ).join("");

      // two digest cards, side by side: the metadata hash (flips) and the
      // structural fingerprint (holds). The verdict word under each is the honest
      // claim, no more.
      const metaCard = `<div class="mha-card ${metaMoved ? "moved" : ""}">
        <div class="mha-card-h">metadata hash<span class="mha-axis">stand-in · flips on any character</span></div>
        <div class="mha-fp mha-meta">${meta}</div>
        <div class="mha-verd ${metaMoved ? "flip" : "hold"}">${metaMoved ? "flipped" : "unchanged"}</div>
        <div class="mha-q">answers: is this the identical compilation</div>
      </div>`;
      const structCard = `<div class="mha-card ${structMoved ? "moved" : ""}">
        <div class="mha-card-h">structural fingerprint<span class="mha-axis">riffcat · shape-exact</span></div>
        <div class="mha-fp mha-struct">${MHA_STRUCT}</div>
        <div class="mha-verd ${structMoved ? "flip" : "hold"}">${structMoved ? "flipped" : "held"}</div>
        <div class="mha-q">answers: is this the same code, names and constants aside</div>
      </div>`;

      const read =
        this.sel === 0
          ? `the unedited source · both fingerprints agree here, because nothing has changed yet`
          : metaMoved && !structMoved
            ? `<b>${cur.note}</b> · the metadata hash <span class="mha-w-flip">flipped</span>, the structural fingerprint <span class="mha-w-hold">held</span>`
            : `${cur.note}`;

      this.innerHTML = `
      <div class="mha-quote detail-only">
        <span class="mha-qmark">Sourcify, on the metadata hash:</span>
        it &ldquo;acts as a fingerprint of the compilation &hellip; the slightest change in the compiler
        settings or even a whitespace in any of the source files will cause a change in the metadata hash.&rdquo;
      </div>
      <div class="mha-controls"><span class="mha-controls-l">edit the source</span>${tabs}</div>
      <div class="mha-stage">
        <div class="codepanel mha-srcpanel">
          <div class="cphead"><b>previewMint</b> <span class="mha-srcnote">${cur.note}</span></div>
          <pre class="code">${solHi(src)}</pre>
        </div>
        <div class="mha-cards">${metaCard}<div class="mha-vs">vs</div>${structCard}</div>
      </div>
      <div class="eqread">${read}</div>
      <p class="twnote">Two axes, not rivals: the exact-identity hash beside a deliberately insensitive one, and riffcat sits beside Sourcify's hash, not in place of it.</p>
      <p class="twnote mha-honest">The metadata hash here is a stand-in computed over the literal text, not a recompile; the structural fingerprint is the shape the engine resolves this function to.</p>`;

      this.querySelectorAll(".mha-tab").forEach((b) =>
        b.addEventListener("click", () => {
          const i = +b.dataset.i;
          if (i === this.sel) return;
          this.sel = i;
          this.render();
        }),
      );
    }
  },
);

// "Main vs Meta": the bytecode wall, in Kaan's own words. From raw onchain
// bytecode you cannot cleanly tell which bytes are code (Main) from which are
// metadata / immutables / libraries / constructor arguments (Meta), so a
// byte-level similarity match has to guess where the line is. riffcat sidesteps
// the guess by reading the source AST, where the dimensions riffcat dials
// (structure, names, constants, types) are separated by construction. No engine:
// a hand-laid SVG that hovers like facet-lattice (a node lights, the .eqread
// readout updates, mouseleave restores the idle line). Copy + diagram only.
//
// Vocabulary is held to Sourcify's SSOT on purpose: the Meta regions are named
// with the VerA transformation reasons (auxdata, immutable, library, constructor),
// "ride the recompilation" is their verb, and riffcat is the source-level
// COMPLEMENT, never a rival byte scheme.

// The interleaved bytecode strip: each cell is a span of onchain bytecode, tagged
// Main (code, the part a similarity search wants) or one of the Meta reasons. The
// point of the picture is that the Meta spans are scattered THROUGH the Main code,
// not parked in one trailing block, so there is no single clean cut.
const BW_STRIP = [
  { w: 16, kind: "main" },
  { w: 5, kind: "immutable", r: "immutable" },
  { w: 13, kind: "main" },
  { w: 6, kind: "library", r: "library" },
  { w: 10, kind: "main" },
  { w: 5, kind: "immutable", r: "immutable" },
  { w: 18, kind: "main" },
  { w: 7, kind: "auxdata", r: "auxdata" },
  { w: 9, kind: "main" },
  { w: 8, kind: "constructor", r: "constructor" },
  { w: 6, kind: "auxdata", r: "auxdata" },
];

// What each region is, in the brief's vocabulary, plus the readout line a hover
// writes. Meta entries name the VerA transformation reason so the picture reads
// as their concept, not riffcat coinage.
const BW_LEGEND = {
  main: {
    t: "Main",
    s: "the code itself: the bytes a similarity search actually wants to compare.",
  },
  immutable: {
    t: "Meta · immutable",
    s: "immutable variable values, patched into the runtime bytecode at deploy. A transformation, not the code's shape.",
  },
  library: {
    t: "Meta · library",
    s: "library addresses linked at deploy. Same shape, different bytes, depending on where the library landed.",
  },
  auxdata: {
    t: "Meta · auxdata",
    s: "CBOR metadata hash appended by the compiler. It moves on a whitespace change, and it can sit more than once in the strip.",
  },
  constructor: {
    t: "Meta · constructor",
    s: "constructor arguments trailing the deployed bytes. Deploy-specific, not part of the code's shape.",
  },
};

// The source side: the four dimensions riffcat reads off the AST, already
// separate by construction. These are exactly the dimensions the dial turns
// (the earlier chapters: full, names-blind, structure), shown here as columns so
// the contrast with the run-on strip is literal.
const BW_DIMS = [
  {
    k: "structure",
    t: "structure",
    s: "the shape of the syntax tree: control flow, the calls, how it is built. This is what survives the dial all the way down.",
  },
  {
    k: "names",
    t: "names",
    s: "identifiers and labels. Read off the AST as their own dimension, so dropping them is one setting of the dial, not a guess.",
  },
  {
    k: "constants",
    t: "constants",
    s: "literal values. Their own dimension too, which is why a constant change can move identity without touching structure.",
  },
  {
    k: "types",
    t: "types",
    s: "declared types. Separated by construction at the source level, where a byte strip has long since flattened them away.",
  },
];

customElements.define(
  "byte-wall",
  class extends HTMLElement {
    connectedCallback() {
      // Geometry. Left column: the opaque onchain strip (Main/Meta interleaved).
      // Right column: the source dimensions, each its own clean band.
      const stripX = 36,
        stripY = 92,
        stripW = 300,
        stripH = 30;
      const total = BW_STRIP.reduce((a, c) => a + c.w, 0);
      let cx = stripX;
      const cells = BW_STRIP.map((c, i) => {
        const w = (c.w / total) * stripW;
        const x = cx;
        cx += w;
        const cls = c.kind === "main" ? "bw-main" : "bw-meta bw-" + c.kind;
        return (
          `<rect class="bw-cell ${cls}" data-k="${c.kind}" x="${x.toFixed(1)}" y="${stripY}"` +
          ` width="${(w - 1).toFixed(1)}" height="${stripH}" rx="2"/>`
        );
      }).join("");

      const dimX = 432,
        dimW = 252,
        dimH = 30,
        dimGap = 12,
        dimY0 = 56;
      const dims = BW_DIMS.map((d, i) => {
        const y = dimY0 + i * (dimH + dimGap);
        return (
          `<g class="bw-dim" data-k="${d.k}" transform="translate(${dimX},${y})">` +
          `<rect class="bw-dimbox" x="0" y="0" width="${dimW}" height="${dimH}" rx="4"/>` +
          `<text class="bw-dimt" x="12" y="${dimH / 2 + 4}">${d.t}</text></g>`
        );
      }).join("");

      const idle =
        "Hover the strip or a source dimension. On the left, Main (code) and Meta (the transformations) interleave, with no clean line between them. On the right, the source already separates them.";

      this.innerHTML = `
      <blockquote class="say bw-say">we do not know which parts of the onchain bytecode is Main vs Meta &middot; find potential similar bytecodes, ignore the Meta parts
        <span class="bw-cite">@kuzdogan, Sourcify, on the similarity work (argotorg/sourcify #1643, 2024)</span>
      </blockquote>
      <svg viewBox="0 0 720 300" class="bw-fig" role="img" aria-label="interleaved onchain bytecode versus the separated source dimensions">
        <text x="${stripX}" y="40" class="latcap">onchain bytecode (one strip)</text>
        <text x="${dimX}" y="40" class="latcap sem">the source, by construction</text>
        <line x1="384" y1="48" x2="384" y2="276" class="latdivide"/>
        ${cells}
        <text x="${stripX}" y="${stripY + stripH + 22}" class="bw-flow">Main and Meta interleave; a byte match guesses the cut</text>
        ${dims}
        <path class="lathandoff" d="M ${stripX + stripW + 6} ${stripY + 15} C 392 ${stripY + 15}, 392 150, ${dimX - 8} 150" marker-end="url(#bw-arr)"/>
        <text x="384" y="246" text-anchor="middle" class="bw-ride">ride the recompilation, read the source</text>
        <defs><marker id="bw-arr" markerWidth="9" markerHeight="9" refX="6" refY="3" orient="auto">
          <path d="M0,0 L6,3 L0,6 z" class="latarrhead"/></marker></defs>
      </svg>
      <div class="eqread" data-idle="${idle}">${idle}</div>
      <p class="twnote">Sourcify's similarity search already strips the trailing auxdata before comparing bytecode, which is the right instinct. The wall is that the other Meta, immutables, libraries, and constructor arguments, does not sit in one clean trailing block: it is patched <b>through</b> the code as deploy-time transformations, so a byte match has to guess where Main ends and Meta begins. riffcat steps one level up. On the source AST the dimensions are separate by construction: <b>structure</b> is its own thing, and <b>names</b>, <b>constants</b>, and <b>types</b> are each their own dimension you can keep or drop. That is the same choice from the earlier chapters, and it is why dropping names is a setting here rather than a guess. It rides the recompilation Sourcify already does; it does not redo the byte split.</p>`;

      const read = this.querySelector(".eqread");
      const say = (t, s) => {
        read.innerHTML = `<b>${t}</b> &middot; ${s}`;
      };
      this.querySelectorAll(".bw-cell").forEach((cell) =>
        cell.addEventListener("mouseenter", () => {
          const k = cell.dataset.k;
          this.querySelectorAll(".bw-cell").forEach((x) =>
            x.classList.toggle("bw-lit", x.dataset.k === k),
          );
          const L = BW_LEGEND[k];
          say(L.t, L.s);
        }),
      );
      this.querySelectorAll(".bw-dim").forEach((g) =>
        g.addEventListener("mouseenter", () => {
          this.querySelectorAll(".bw-dim").forEach((x) =>
            x.classList.remove("on"),
          );
          g.classList.add("on");
          const d = BW_DIMS.find((x) => x.k === g.dataset.k);
          say(d.t, d.s);
        }),
      );
      this.addEventListener("mouseleave", () => {
        this.querySelectorAll(".bw-cell").forEach((x) =>
          x.classList.remove("bw-lit"),
        );
        this.querySelectorAll(".bw-dim").forEach((x) =>
          x.classList.remove("on"),
        );
        read.innerHTML = read.dataset.idle;
      });
    }
  },
);

// What we sampled: the honest-numbers beat. Two ledgers, side by side. The first
// is what the demo already measures (exact counts over distinct Sourcify source
// files, and one null-floor calibration); the second is what is NOT yet measured
// (precision and recall versus a naive name/bytecode baseline on a labelled
// slice). The second ledger is rendered as explicitly PENDING: no number is
// invented for it. Click a measured row for how it was produced; the pending
// rows describe the measurement that would fill them. Static (no engine); the
// counts are transcribed from the vuln-reach measurement, so they match the
// other chapters exactly and this chapter cannot drift from them at runtime.
const SAMPLED_MEASURED = [
  {
    what: "ERC-4626 vulnerable shape, exact-source reach",
    unit: "distinct source files",
    n: "741",
    kind: "under",
    how: "The largest single Sourcify source_hash for each of two vulnerable convertToShares shapes (OZ 153, solmate 588). One source_hash is one exact file content, so this is the count an exact-source match keyed on the dominant file already groups together. Exact within the ERC4626.sol population, and a floor: flattened and oddly-vendored files were not classified.",
  },
  {
    what: "ERC-4626 vulnerable shape, structure reach",
    unit: "distinct source files",
    n: "1,344",
    kind: "under",
    how: "The same two vulnerable structure fingerprints, summed across every source variant that carries them (OZ 228 over 8 variants, solmate 1,116 over 27). 100% of the ERC4626.sol-named OZ and solmate populations were classified, so within that corpus this is the full count, not a sample. It is a floor for the same reason as the exact number, and the shape-minus-exact gap (+603) is itself a floor: more variants only widen it.",
  },
  {
    what: "Multicall fuzzy match, chance baseline",
    unit: "null calibration",
    n: "score",
    kind: "null",
    how: "Not a count. The weighted-containment score an unrelated function is expected to reach against the vulnerable Multicall shape by chance, with the OZ patched form used as a clean negative that sits below the catches. This calibrates where a fuzzy score stops being noise. It is one shape's floor, reported alongside the marginal cases the score alone cannot certify, not a corpus-wide error rate.",
  },
];
const SAMPLED_PENDING = [
  {
    what: "Precision vs a naive name/bytecode baseline",
    needs: "labelled slice",
    how: "Of the contracts riffcat flags as a given shape, what fraction a labelled ground truth agrees with, measured against a baseline that matches on function name or on bytecode hash. We do not have a labelled slice yet, so we report no number here rather than a flattering one.",
  },
  {
    what: "Recall vs a naive name/bytecode baseline",
    needs: "labelled slice",
    how: "Of the contracts that truly carry a shape, what fraction riffcat recovers, against the same baseline. The reach numbers above suggest where shape matching gains over exact match, but a gain in reach is not recall until the population is labelled.",
  },
];
customElements.define(
  "sampled-ledger",
  class extends HTMLElement {
    connectedCallback() {
      this.measured = SAMPLED_MEASURED;
      this.pending = SAMPLED_PENDING;
      this.sel = 0;
      this.render();
    }
    render() {
      const mRows = this.measured
        .map((r, i) => {
          const hue = r.kind === "null" ? 38 : 145;
          const tag = r.kind === "null" ? "chance baseline" : "undercount";
          return (
            `<tr class="lxrow sampled-mrow ${i === this.sel ? "on" : ""}" data-i="${i}" style="--hue:${hue}">` +
            `<td class="lx-shape">${r.what}</td>` +
            `<td class="sampled-unit">${r.unit}</td>` +
            `<td class="lx-verd ok">${tag}</td>` +
            `<td class="lx-n lx-reach">${r.n}</td></tr>`
          );
        })
        .join("");
      const pRows = this.pending
        .map(
          (r) =>
            `<tr class="lxrow sampled-prow" style="--hue:215">` +
            `<td class="lx-shape">${r.what}</td>` +
            `<td class="sampled-unit">${r.needs}</td>` +
            `<td class="lx-verd sampled-pend">not measured</td>` +
            `<td class="lx-n sampled-pendn">pending</td></tr>`,
        )
        .join("");
      this.innerHTML = `
      <table class="vledger sampled-table"><thead><tr><th>what the number is</th><th>counted over</th><th>kind</th><th>value</th></tr></thead><tbody>${mRows}</tbody></table>
      <div class="vbody codepanel"></div>
      <p class="vledger-cap">Not measured yet: precision and recall against a labelled baseline; left blank rather than faked.</p>`;
      this.querySelectorAll(".sampled-mrow").forEach((row) =>
        row.addEventListener("click", () => {
          this.sel = +row.dataset.i;
          this.querySelectorAll(".sampled-mrow").forEach((x, i) =>
            x.classList.toggle("on", i === this.sel),
          );
          this.renderBody();
        }),
      );
      this.renderBody();
    }
    renderBody() {
      const r = this.measured[this.sel];
      const hue = r.kind === "null" ? 38 : 145;
      const badge = r.kind === "null" ? "chance baseline" : "undercount";
      this.querySelector(".vbody").innerHTML =
        `<div class="vhead" style="--hue:${hue}"><span class="vbadge">${badge}</span> <b>${r.what}</b>` +
        `<span class="vsub">${r.unit}</span><span class="vfp">${r.n}</span></div>` +
        `<p class="vrole sampled-how">${r.how}</p>`;
    }
  },
);

// The cheap yes. hevm ships a syntactic fast-path in EVM/SymExec.hs:
// equivalenceCheck opens with `case bytecodeA == bytecodeB of True -> pure
// mempty`, returning equivalent with no solver call. That is EXACT byte-equality
// only: the trailing solc metadata tail counts, there is no facet and no
// normalization. This chapter generalizes that one check to a facet dial and is
// explicit about the soundness condition: a cheap yes is a discharge only at a
// facet that forgets nothing hevm observes (return value, storage, success).
// Coarser facets (forget constants) only PARTITION and PRIORITIZE; they do not
// discharge. The chapter is fully STATIC: no engine call, no hevm run. The hex
// is schematic and labelled as such, never presented as live hevm output. All
// top-level names are CY_-prefixed to avoid collision with app.js.
//
// Attribution is at the work level: argotorg/hevm (= ethereum/hevm), AGPL-3.0.
// We did not invent the fast-path; we generalize hevm's existing one.

// The four dial stops, fine to coarse: each forgets one more thing than the last.
// `forgets` names what the facet drops; `sound` records whether agreement at
// this facet is a sound discharge (forgets nothing behavioral) or only triage.
const CY_FACETS = [
  {
    key: "exact",
    label: "exact",
    forgets: "nothing: raw byte-equality, hevm's own check",
    kind: "discharge",
    note: "This is hevm's shipped fast-path: byte-identical bytecode is surely equivalent, no solver. It is narrow: the trailing solc metadata tail counts, so two builds of the same code can miss.",
  },
  {
    key: "meta",
    label: "forget metadata",
    forgets: "the trailing solc CBOR/auxdata tail",
    kind: "discharge",
    note: "The honest win: the metadata tail is bytes hevm also ignores, so forgetting it forgets nothing behavioral. This widens hevm's exact yes to pairs that differ only in the tail, which its own == misses.",
  },
  {
    key: "names",
    label: "forget names",
    forgets: "symbol and label naming (non-behavioral renaming)",
    kind: "candidate",
    note: "Forgetting names is sound ONLY if the renaming touches nothing behavioral, and riffcat does not yet certify that. So a match here is a candidate to check, not a discharge.",
  },
  {
    key: "const",
    label: "forget constants",
    forgets: "literal constant values",
    kind: "triage",
    note: "Forgetting constants is generally NOT behavior-complete: hevm observes the exact returned and stored values. A match here only partitions and prioritizes the work. It never discharges. See the PUSH1 3 vs PUSH1 4 pair below.",
  },
];

// Schematic illustrative bytecode-ish pairs. The hex is hand-written to make the
// facet behavior legible; it is NOT compiler output and NOT hevm output. `addr`
// gives, per facet, the content address each side computes at that facet: two
// sides sharing an address are "same at the facet". `truth` is the actual
// behavioral relation (what a verifier would decide), used only to mark the one
// case where a facet match is a FALSE collision (the counterexample).
const CY_PAIRS = [
  {
    key: "identical",
    title: "byte-for-byte identical",
    blurb:
      "The floor hevm already discharges for free: the two bytecodes are the same bytes.",
    a: { hex: "6080604052348015...600436106100...a264697066", tail: "" },
    b: { hex: "6080604052348015...600436106100...a264697066", tail: "" },
    addr: {
      exact: ["x91", "x91"],
      meta: ["x91", "x91"],
      names: ["x91", "x91"],
      const: ["x91", "x91"],
    },
    truth: "equivalent",
  },
  {
    key: "metatail",
    title: "same code, different metadata tail",
    blurb:
      "Identical Main bytecode; only the trailing solc metadata (CBOR/swarm hash) differs. Two builds of the same source.",
    a: {
      hex: "6080604052348015...600436106100...",
      tail: "a2646970667358221220aa11",
    },
    b: {
      hex: "6080604052348015...600436106100...",
      tail: "a2646970667358221220bb22",
    },
    addr: {
      exact: ["x4e", "x7c"],
      meta: ["x4e", "x4e"],
      names: ["x4e", "x4e"],
      const: ["x4e", "x4e"],
    },
    truth: "equivalent",
  },
  {
    key: "relabel",
    title: "same shape, non-behavioral relabeling",
    blurb:
      "Same control and data flow and the same constants; the two differ only in naming that, here, carries no behavior.",
    a: { hex: "6080...PUSH4 1a2b...JUMPDEST...PUSH1 03 RETURN", tail: "" },
    b: { hex: "6080...PUSH4 9f0e...JUMPDEST...PUSH1 03 RETURN", tail: "" },
    addr: {
      exact: ["xd1", "xa8"],
      meta: ["xd1", "xa8"],
      names: ["x33", "x33"],
      const: ["x33", "x33"],
    },
    truth: "equivalent (here), but unverified by riffcat",
  },
  {
    key: "push3v4",
    title: "same shape, one constant changed",
    blurb:
      "hevm's own counterexample: one returns PUSH1 3, the other PUSH1 4. Same shape, different behavior.",
    a: { hex: "6080...600360005260206000F3  (returns 3)", tail: "" },
    b: { hex: "6080...600460005260206000F3  (returns 4)", tail: "" },
    addr: {
      exact: ["x60", "x71"],
      meta: ["x60", "x71"],
      names: ["x60", "x71"],
      const: ["x05", "x05"],
    },
    truth: "NOT equivalent: returns 3 vs returns 4",
  },
];

customElements.define(
  "cy-cheap-yes",
  class extends HTMLElement {
    connectedCallback() {
      this.facet = "meta"; // open on the honest win, one stop past hevm's exact ==
      this.render();
    }
    // For one pair at the current facet: do the two sides share an address?
    same(p) {
      const [u, v] = p.addr[this.facet];
      return u === v;
    }
    // The status of a pair at the current facet: how the facet relates the pair
    // AND whether that relation is sound. The single hazard case is a coarse-facet
    // collision of behaviorally-distinct programs (push3v4 at forget-constants).
    status(p) {
      const F = CY_FACETS.find((f) => f.key === this.facet);
      if (!this.same(p))
        return {
          cls: "cy-split",
          word: "different at this facet",
          line: "the dial does not relate these two; run the verifier as usual",
        };
      if (F.kind === "discharge")
        return {
          cls: "cy-yes",
          word: "sound yes",
          line: "same address at a facet that forgets nothing behavioral, so this is a discharge: no solver call",
        };
      if (F.kind === "candidate")
        return {
          cls: "cy-cand",
          word: "candidate, check it",
          line: "same address, but riffcat has not certified this facet forgets nothing behavioral, so it is a candidate for the verifier",
        };
      // triage facet: a match only partitions/prioritizes. Flag the false collision.
      if (p.truth.indexOf("NOT") === 0)
        return {
          cls: "cy-hazard",
          word: "NOT a yes",
          line:
            "these share an address here but are not equivalent (" +
            p.truth.replace(/^NOT equivalent: /, "") +
            "): a coarse facet only partitions, it cannot discharge",
        };
      return {
        cls: "cy-cand",
        word: "candidate, check it",
        line: "same address at a coarse facet: this only prioritizes the pair, the verifier still decides",
      };
    }
    render() {
      const F = CY_FACETS.find((f) => f.key === this.facet);
      const ladder = CY_FACETS.map(
        (f) =>
          `<span class="stop ${f.key === this.facet ? "on" : ""}" data-facet="${f.key}">${f.label}</span>`,
      ).join("");

      const rows = CY_PAIRS.map((p) => {
        const s = this.status(p);
        const sideA = `<code class="cy-hex">${p.a.hex}${p.a.tail ? `<span class="cy-tail">${p.a.tail}</span>` : ""}</code>`;
        const sideB = `<code class="cy-hex">${p.b.hex}${p.b.tail ? `<span class="cy-tail">${p.b.tail}</span>` : ""}</code>`;
        return `<div class="cy-pair ${s.cls}" data-key="${p.key}">
        <div class="cy-pair-h"><b>${p.title}</b><span class="cy-verd ${s.cls}">${s.word}</span></div>
        <div class="cy-blurb">${p.blurb}</div>
        <div class="cy-sides">
          <div class="cy-side"><span class="cy-lab">A</span>${sideA}</div>
          <div class="cy-rel ${this.same(p) ? "cy-eq" : "cy-ne"}">${this.same(p) ? "same address" : "different"}</div>
          <div class="cy-side"><span class="cy-lab">B</span>${sideB}</div>
        </div>
        <div class="cy-line">${s.line}</div>
      </div>`;
      }).join("");

      const condClass =
        F.kind === "discharge"
          ? "cy-cond-ok"
          : F.kind === "candidate"
            ? "cy-cond-cand"
            : "cy-cond-haz";
      const condWord =
        F.kind === "discharge"
          ? "covers hevm's footprint: a match here is a sound yes"
          : F.kind === "candidate"
            ? "not yet certified to cover hevm's footprint: a match here is a candidate"
            : "does not cover hevm's footprint: a match here only partitions and prioritizes";

      this.innerHTML = `
      <blockquote class="say cy-say">case bytecodeA == bytecodeB of True -&gt; pure mempty
        <span class="cy-cite">the a==b and similar checks are ONLY syntactic checks. If they are true, then they are surely equivalent.</span>
        <span class="cy-cite cy-attr">paraphrased from argotorg/hevm, EVM/SymExec.hs (AGPL-3.0); read-only, not executed here</span>
      </blockquote>
      <div class="dialbar"><div class="grp"><span>facet</span><div class="ladder cy-ladder">${ladder}</div></div></div>
      <div class="cy-cond ${condClass}"><span class="cy-cond-f">${F.label}</span> forgets ${F.forgets} &middot; ${condWord}</div>
      <div class="cy-pairs">${rows}</div>
      <div class="eqread cy-read">${F.note}</div>
      <p class="twnote">hevm's == already proves the byte-identical floor; the facet generalization stays a sound discharge only where the facet forgets nothing hevm observes (return, storage, success), and everywhere coarser it only partitions and prioritizes.</p>`;

      this.querySelectorAll("[data-facet]").forEach((s) =>
        s.addEventListener("click", () => {
          if (this.facet !== s.dataset.facet) {
            this.facet = s.dataset.facet;
            this.render();
          }
        }),
      );
    }
  },
);

// "the seat, filled": a concrete occupant for the empty verdict seat.
//
// Static, buildable now. Renders synchronously: every snippet below is real,
// short text read off argotorg/EquiVM at the pinned commit
// 723d612a797fb4cf1dbe4c5ebf82a7124011ec87 (Examples/ERC20/ERC20.sol,
// Examples/ERC20/Spec.lean, Solm/Equiv.lean, Examples/ERC20/Correct.lean,
// Reasoning/MISSPEC.md). No Lean, no lake, no wasm call. The #print axioms line
// quoted is the one MISSPEC.md documents verbatim for the Truth example (the
// ERC20 boundary has the same named shape); the panel says so in the readout.
//
// Idiom: the facet-lattice / prior-art hover-and-read pattern. A column of
// cited code panels; hover (or focus) one and the .eqread explains the riffcat
// connection for that panel. Reuses .codepanel/.cphead/.eqread/.twnote/.vbug/
// .vsub and the theme vars. All top-level names are prefixed `sf` to avoid
// collision with app.js.

const SF_PIN = "argotorg/EquiVM @ 723d612a";

// Each panel: a short, real, cited snippet plus the framing read on hover. The
// `read` strings stay in Register A: structural, content-address, facet, shape,
// localize, anchor. `code` is verbatim-short from the pinned files.
const SF_PANELS = [
  {
    k: "sol",
    head: "ERC20.sol",
    src: "Examples/ERC20/ERC20.sol",
    accent: "var(--ink-dim)",
    code: '// Runtime bytecode is generated with:\n//   solc --bin-runtime --evm-version shanghai ERC20.sol\n// optimizer OFF.\nfunction transfer(address to, uint256 value) external returns (bool) {\n    require(balanceOf[msg.sender] >= value, "ERC20: insufficient balance");\n    balanceOf[msg.sender] -= value;\n    balanceOf[to] += value;\n    emit Transfer(msg.sender, to, value);\n    return true;\n}',
    read: "The artifact under the proof. The bytecode side is solc output, pinned to the exact invocation (solc shanghai, optimizer OFF). This is the provenance leg: which source became which bytecode. riffcat would localize this transfer subtree across the corpus; the proof is what fills the seat for one such candidate.",
  },
  {
    k: "spec",
    head: "the Solm spec (a forgetting, so a facet)",
    src: "Examples/ERC20/Spec.lean",
    accent: "var(--a)",
    code: '-- The Solidity contract emits the standard events, but the current Solm\n-- statement tracks storage and return values only.\ndef transferTransition : TransitionDecl :=\n  { name := "transfer"\n    params := [{ name := "to", ty := addr }, { name := "value", ty := uint256 }]\n    returnType := some (.elem .bool)\n    body :=\n      [ .require (.binary .eq (.env .callvalue) (.intLit 0)),\n        .letDecl "fromBalance" (some uint256) (.storage (balanceOfRef sender)),\n        .require (.binary .ge (.var "fromBalance") (.var "value")),\n        ... .return (.boolLit true) ] }',
    read: "The elegant point. The spec keeps storage, control, and return; it drops events, gas, and revert strings (the header says so: storage and return values only). That is exactly a facet: a forgetting that defines a syntactic equivalence class. The spec is a hand-authored facet projection of the contract, the same move riffcat makes by content-addressing at a chosen dimension subset.",
  },
  {
    k: "split",
    head: "the case-split equivalence statement",
    src: "Solm/Equiv.lean",
    accent: "var(--cool)",
    code: "inductive runtimeEquivalenceFor (cfg : Config) (contract : ContractDecl) ... : Prop where\n  | execution      : -- both return: accountMapEquiv storage + ABI return match\n  | noDispatch     : -- spec selector miss, EVM reverts\n  | decodingFailed : -- spec ABI decode fails, EVM reverts\n  | outOfGas       : -- EVM runs out of gas\n\n-- (Equiv.lean) There is intentionally no case for `evmRes = .error e`:\n-- a real EVM exception leaves the equivalence unmatchable, so the proof\n-- fails rather than silently equating a crash with a revert.",
    read: "The verdict's shape: for all initial states, calldata, and gas, the two executions land in one of four cases. execution means equal storage at every slot (accountMapEquiv) and the return bytes are the ABI encoding of the spec's return. This is what a filled seat looks like: not a hand-wave, a total case split with the unmatched-crash case left deliberately open.",
  },
  {
    k: "thm",
    head: "the theorem, per contract",
    src: "Examples/ERC20/Correct.lean",
    accent: "var(--cool)",
    code: "/-- The deployed ERC20 runtime bytecode refines the Solm specification,\n    for every initial state. -/\ntheorem erc20Correct :\n    runtimeEquivalence!?! erc20Config erc20Bytecode erc20Contract := by\n  refine ...\n  -- dispatch on the selector, route each of the six function bodies\n  -- to its own correctness obligation; else revert.",
    read: "The fact that anchors. erc20Correct is a kernel-checked theorem about one bytecode against one spec. Because the spec is a facet, the fact rides that facet address: it transports to every artifact that is behavior-complete at the spec's footprint (storage, control, return). Prove once on the representative, recognize the class. riffcat localizes the class; this is the fact that anchors to it.",
  },
  {
    k: "ax",
    head: "the #print axioms trust boundary",
    src: "Reasoning/MISSPEC.md",
    accent: "var(--warm)",
    code: "#print axioms truthCorrect  -- documented verbatim in MISSPEC.md\n[propext, Classical.choice, Quot.sound,\n ByteArray_zeroes_size, byteArray_zeroes_toList,\n truthSelectorBytes, truthValidJumps]\n-- no sorryAx, no native_decide. The ERC20 proof names the same\n-- per-contract facts (its keccak selector bytes + valid-jump set).",
    read: "What it trusts, named in the open. Not 'verified': this bytecode equals this TRUSTED hand-written spec, under a trusted Lean EVM (evmlean), over a bounded fragment, with per-contract keccak-selector and jumpdest axioms (and a zero-content axiom). The boundary is the message: a facet address is the scope a fact stays sound in, and here the proof states its scope rather than hiding it.",
  },
];

const SF_FOOTPRINT =
  "Footprint the proof stays sound in: storage layout, control flow, return shape. The Solm spec forgets at least this much, so the obligation and the anchor agree.";

const SF_CAVEATS = [
  "Not 'verified' past: this bytecode equals this trusted, hand-written Solm spec, under a trusted Lean EVM (evmlean), over a bounded Solidity fragment (no events, dynamic arrays, revert strings, transient storage), with per-contract keccak-selector and jumpdest axioms.",
  "The outOfGas case means a non-terminating EVM run is currently equivalent to any spec (a known one-sided caveat in EquiVM's TODO). So the equivalence is not yet total in that direction.",
  "No automated riffcat to EquiVM pipeline exists. riffcat localizes a candidate and a facet; turning that into a Solm spec and a hand proof is unbuilt engineering. This panel cites a real proof statically; it does not run Lean.",
];

const SF_IDLE =
  "The previous chapter left two columns empty. This one fills them with a single, cited occupant: a Lean proof. Hover a panel to read how it sits against riffcat. Source: " +
  SF_PIN +
  ", read-only at the pin.";

customElements.define(
  "seat-filled",
  class extends HTMLElement {
    connectedCallback() {
      const panels = SF_PANELS.map(
        (p) =>
          `<div class="sf-panel codepanel" data-k="${p.k}" tabindex="0" style="--sf:${p.accent}">` +
          `<div class="cphead sf-head"><b>${p.head}</b>` +
          `<span class="vsub sf-src">${p.src}</span></div>` +
          `<pre class="sf-code">${sfEsc(p.code)}</pre></div>`,
      ).join("");
      const caveats = SF_CAVEATS.map((c) => `<li>${c}</li>`).join("");
      this.innerHTML = `
      <p class="vbug sf-bug">The seat from <b>prove it</b>, now occupied. riffcat <b>localizes</b> a candidate at a facet and states the obligation; a verifier <b>adjudicates</b>. Below is one verifier's filled verdict, cited statically from <span class="sf-pin">${SF_PIN}</span>.</p>
      <div class="sf-foot"><span class="sf-foot-mark">anchor</span><span>${SF_FOOTPRINT}</span></div>
      <div class="sf-grid">${panels}</div>
      <div class="eqread sf-read" data-idle="${sfAttr(SF_IDLE)}">${SF_IDLE}</div>
      <div class="sf-caveats"><span class="sf-caveats-h">what this does not claim</span><ul>${caveats}</ul></div>
      <p class="twnote">Two riffcat threads meet here. First, transport: the theorem is a fact anchored to the spec's facet address, so it rides every artifact that shares that address (same storage, control, and return up to the spec's forgetting), prove once and recognize the class. Second, the spec itself is a facet: it keeps storage, control, and return and drops events, gas, and revert strings, which is the same forgetting riffcat performs when it content-addresses at a dimension subset. riffcat localizes and addresses; EquiVM decides, and names exactly what it trusts. The seat is filled by a fact, not a feeling.</p>`;
      const read = this.querySelector(".sf-read");
      const show = (p) => {
        this.querySelectorAll(".sf-panel").forEach((x) =>
          x.classList.toggle("dim", x.dataset.k !== p.k),
        );
        read.innerHTML = `<span class="sf-sw" style="background:${p.accent}"></span><b>${p.head}</b> &middot; ${p.read}`;
      };
      this.querySelectorAll(".sf-panel").forEach((el) => {
        const p = SF_PANELS.find((x) => x.k === el.dataset.k);
        el.addEventListener("mouseenter", () => {
          el.classList.add("on");
          show(p);
        });
        el.addEventListener("focus", () => {
          el.classList.add("on");
          show(p);
        });
        el.addEventListener("blur", () => el.classList.remove("on"));
      });
      this.addEventListener("mouseleave", () => {
        this.querySelectorAll(".sf-panel").forEach((x) =>
          x.classList.remove("dim", "on"),
        );
        read.innerHTML = read.dataset.idle;
      });
    }
  },
);

function sfEsc(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}
function sfAttr(s) {
  return String(s).replace(/&/g, "&amp;").replace(/"/g, "&quot;");
}

// Same source, two instantiations (merklization chapter B). A static, hand-built
// illustration: one generic source (RPow<Pair, N>, a perfect binary tree of
// depth N) instantiated two ways (N=4, N=8). It shows, per dimension, where the
// two facet addresses AGREE (one generic skeleton) and where they DIVERGE
// (the kept type spelling). No engine call: the per-dimension agreement is
// hand-asserted from the source spellings, and the honest gap is labelled on
// screen. The chips are colored from short hand-picked digests so agree=one
// color, diverge=two colors, matching the chip idiom used live elsewhere.
//
// All top-level names are prefixed `ti` to avoid collision with app.js. Reuses
// engineReady-free (synchronous render), chipColor, and the .eqread .twnote
// .vledger .lxrow .lx-shape .lx-fp .vsub .vlead .vbug classes plus theme vars.

// The two instantiations of one generic source. `spell` is the source type
// spelling the Types dimension would record (syntactic, per the spike: the
// engine keeps the spelling, not a resolved monomorphization).
const TI_INSTANCES = [
  {
    key: "n4",
    arg: "N = 4",
    spell: "RPow<Pair, 4>",
    note: "a depth-4 perfect binary tree",
  },
  {
    key: "n8",
    arg: "N = 8",
    spell: "RPow<Pair, 8>",
    note: "a depth-8 perfect binary tree",
  },
];

// One row per dimension riffcat addresses on the syntactic side. `agree` is the
// hand-asserted fact for THIS pair: do the two instantiations land on the same
// per-dimension digest. structure/names/constants agree (one generic skeleton,
// same identifiers, same literals in the source); types diverges because the
// kept type spelling differs (RPow<Pair,4> vs RPow<Pair,8>). The digests are
// short illustrative stand-ins (clearly labelled as such in the footer), chosen
// so a shared digest renders one color and a split renders two.
const TI_DIMS = [
  {
    key: "structure",
    lab: "structure",
    agree: true,
    why: "the same generic skeleton: same nodes, same child edges, same shape",
    d4: "5d3a91c0",
    d8: "5d3a91c0",
  },
  {
    key: "names",
    lab: "names",
    agree: true,
    why: "the same identifiers in the source (RPow, Pair); the instantiation changed neither",
    d4: "7adf2b14",
    d8: "7adf2b14",
  },
  {
    key: "constants",
    lab: "constants",
    agree: true,
    why: "no literal in the generic body changed between the two instantiations",
    d4: "ffb45408",
    d8: "ffb45408",
  },
  {
    key: "types",
    lab: "types",
    agree: false,
    why: "the kept type SPELLING differs (RPow<Pair, 4> vs RPow<Pair, 8>), so the per-node type field differs",
    d4: "c81f6d22",
    d8: "3a0e77b9",
  },
];

// The two facets the chapter contrasts, drawn straight from the spike's beat:
// structure-only AGREES (the family shares one address); a facet that keeps the
// type spelling DIVERGES (the two part). Each names the dimensions it keeps.
const TI_FACETS = [
  {
    key: "structure",
    lab: "structure",
    keeps: ["structure"],
    verdict: "agree",
  },
  {
    key: "keepstypes",
    lab: "keeps types",
    keeps: ["structure", "names", "constants", "types"],
    verdict: "diverge",
  },
];

const tiShort = (h) => (h || "").slice(0, 8);
// One chip for a (dimension, instance) cell, colored from its short digest so
// agreement renders one shared color across the two instances and divergence
// renders two.
function tiChip(digest, label) {
  const col =
    window.wasmBindings && window.wasmBindings.chip_color
      ? (() => {
          try {
            return window.wasmBindings.chip_color(digest);
          } catch (_) {
            return null;
          }
        })()
      : null;
  const fall = (() => {
    let a = 0;
    for (let i = 0; i < 8; i++) a = (a * 131 + digest.charCodeAt(i)) >>> 0;
    return `oklch(72% 0.13 ${((a * 137.508) % 360).toFixed(1)})`;
  })();
  const c = typeof chipColor === "function" ? chipColor(digest) : col || fall;
  return `<span class="chip ti-chip" style="--chip:${c}" title="${label}">${tiShort(digest)}</span>`;
}

customElements.define(
  "two-instantiations",
  class extends HTMLElement {
    connectedCallback() {
      // facet the reader has selected; default to the structure facet (the agree
      // beat) so the page opens on "it is one skeleton".
      this.facet = "structure";
      this.render();
    }
    // does this dimension survive (get kept by) the current facet?
    kept(dimKey) {
      return TI_FACETS.find((f) => f.key === this.facet).keeps.includes(dimKey);
    }
    render() {
      const facet = TI_FACETS.find((f) => f.key === this.facet);
      // facet selector (two stops), mirroring the .ladder idiom
      const ladder = TI_FACETS.map(
        (f) =>
          `<span class="stop ti-stop ${f.key === this.facet ? "on" : ""}" data-facet="${f.key}">${f.lab}</span>`,
      ).join("");
      // the two instantiation header cards
      const heads = TI_INSTANCES.map(
        (u) =>
          `<div class="ti-inst"><div class="ti-inst-arg">${u.arg}</div>` +
          `<code class="ti-spell">${u.spell}</code>` +
          `<div class="ti-inst-note">${u.note}</div></div>`,
      ).join("");
      // per-dimension comparison rows. A dimension dropped by the current facet is
      // greyed (not part of this address). A kept dimension shows the two chips and
      // an agree/diverge tag computed only over the kept dimensions.
      const rows = TI_DIMS.map((d) => {
        const on = this.kept(d.key);
        const tag = !on
          ? `<span class="ti-tag ti-off">dropped at this facet</span>`
          : d.agree
            ? `<span class="ti-tag ti-agree">agree</span>`
            : `<span class="ti-tag ti-diverge">diverge</span>`;
        return (
          `<tr class="ti-row ${on ? "" : "ti-rowoff"}" data-dim="${d.key}">` +
          `<td class="ti-dimname">${d.lab}</td>` +
          `<td class="ti-cell">${on ? tiChip(d.d4, "N=4 " + d.lab) : `<span class="ti-dash">-</span>`}</td>` +
          `<td class="ti-cell">${on ? tiChip(d.d8, "N=8 " + d.lab) : `<span class="ti-dash">-</span>`}</td>` +
          `<td class="ti-verdict">${tag}</td></tr>`
        );
      }).join("");
      // the address line: at this facet, fold the kept dimensions. They agree iff
      // every kept dimension agrees; that is the whole beat.
      const keptDims = TI_DIMS.filter((d) => this.kept(d.key));
      const allAgree = keptDims.every((d) => d.agree);
      const addrLine = allAgree
        ? `the two instantiations share <b>one address</b> at the ${facet.lab} facet: it is one generic skeleton`
        : `the two instantiations land on <b>two addresses</b> at the ${facet.lab} facet: the kept type spelling parts them`;
      const idle = `${facet.lab} facet keeps ${facet.keeps.length} dimension${facet.keeps.length === 1 ? "" : "s"} (${facet.keeps.join(", ")}). Hover a dimension to read why it agrees or diverges.`;
      this.innerHTML = `
      <div class="dialbar"><div class="grp"><span>facet</span><div class="ladder">${ladder}</div></div>
        <div class="ti-addr ${allAgree ? "ti-addr-agree" : "ti-addr-diverge"}">${addrLine}</div></div>
      <div class="ti-source">
        <div class="ti-generic"><span class="ti-glabel">one generic source</span><code class="ti-gspell">RPow&lt;Pair, N&gt;</code>
          <span class="ti-gnote">instantiated two ways, below; same generic code, different N</span></div>
        <div class="ti-insts">${heads}</div>
      </div>
      <table class="vledger ti-table">
        <thead><tr><th>dimension</th><th>N = 4</th><th>N = 8</th><th>at this facet</th></tr></thead>
        <tbody>${rows}</tbody>
      </table>
      <div class="eqread ti-read" data-idle="${idle}">${idle}</div>
      <p class="twnote ti-gap"><b>The honest gap, on screen.</b> riffcat's Types dimension keeps the syntactic type <em>spelling</em> at each node, not a resolved monomorphization. So the divergence you see at the types facet is the <b>syntactic shadow</b> of the instantiation (the spelling <code>RPow&lt;Pair, 4&gt;</code> vs <code>RPow&lt;Pair, 8&gt;</code>), not the monomorphized residue (the flat combinator nests the two unroll to, which a local Merkle address would see as two unrelated shapes). A local content address sees either the one generic form or the N concrete forms, never the indexed <b>family</b> that relates them. Showing a genuine pre- versus post-monomorphization pair would need new producer output (a lowering that emits the monomorphized graph); this illustration uses the source spelling the engine keeps today. The digests shown are short illustrative stand-ins, not live engine output: this chapter is hand-built to name the edge of the method, in the spirit of <em>what we sampled</em>.</p>`;
      this.querySelectorAll("[data-facet]").forEach((s) =>
        s.addEventListener("click", () => {
          if (this.facet !== s.dataset.facet) {
            this.facet = s.dataset.facet;
            this.render();
          }
        }),
      );
      const read = this.querySelector(".ti-read");
      this.querySelectorAll(".ti-row").forEach((row) =>
        row.addEventListener("mouseenter", () => {
          const d = TI_DIMS.find((x) => x.key === row.dataset.dim);
          const on = this.kept(d.key);
          read.innerHTML = on
            ? `<b>${d.lab}</b> · ${d.agree ? "agree" : "diverge"}: ${d.why}`
            : `<b>${d.lab}</b> · dropped at the ${facet.lab} facet, so it is not part of this address: ${d.why}`;
        }),
      );
      this.addEventListener("mouseleave", () => {
        read.innerHTML = read.dataset.idle;
      });
    }
  },
);

// "the engine checks too" (page_key engine-checks-too). A STATIC, CITED chapter:
// it renders synchronously over baked, illustrative data and makes no live
// engine call and runs no Lean. The claim is the spike-note plan (Phases 0-2 of
// /workspace/lean-riffcat-lockstep-spike-2026-06-24.md), not a live proof.
//
// Three parts, all prose-as-data:
//   (1) the lockstep bench: a corpus of canonical Graph inputs hashed by two
//       cores, Rust (the engine that ships) and a Lean executable spec, with the
//       output bytes required equal in CI. This is mechanism (a), the existing
//       golden.rs/conformance.rs move with Lean added as a third path. Both
//       sides call the SAME blake3 C library, so the hash is identical bytes.
//   (2) the laws ledger: what Lean would PROVE about the abstract construction
//       (section 2 of the spike): determinism/order-independence, the facet
//       refinement lattice, anchor/transport soundness, AnonymousShape
//       correctness, SCC/WL termination, encoding injectivity.
//   (3) the trust tiers, kept apart on purpose: kernel-checked laws, one NAMED
//       assumption (blake3 collision-resistance is a Lean axiom, not a theorem),
//       and the open gap (the proofs are about the Lean model; the shipping Rust
//       is held to it by golden vectors and reading, not yet verified extraction).
//
// All top-level names are prefixed `lck` to avoid app.js collisions. Reuses the
// .twnote/.eqread/.shapedot/.codepanel/.cphead/.vledger classes and theme vars;
// new visuals are the .lck* classes in the css field. The digest chips are baked
// illustrative content addresses styled like the engine's, NOT live engine
// output: a small deterministic string-to-color so the bytes read as addresses
// without claiming a run. If chipColor is present it is reused for fidelity.

// A deterministic illustrative color from a baked hex string, so a digest chip
// looks like the engine's content-address chips without any engine call. Falls
// back to the app's chipColor when that helper is available (same look as the
// live chapters); otherwise an in-page hash keeps it self-contained.
function lckChip(hex) {
  try {
    if (typeof chipColor === "function") return chipColor(hex);
  } catch (_) {}
  let h = 0;
  for (let i = 0; i < hex.length; i++) h = (h * 31 + hex.charCodeAt(i)) >>> 0;
  const hue = h % 360,
    sat = 60 + ((h >> 9) % 25),
    lit = 58 + ((h >> 17) % 12);
  return `hsl(${hue} ${sat}% ${lit}%)`;
}
const lckShort = (h) => (h || "").slice(0, 10);

// The corpus rows. Each is a canonical Graph input plus ONE baked digest the
// two cores must agree on byte-for-byte. The shapes are the adversarial ones the
// spike names as the corpus's real job (the SCHEMA_VERSION 2 cases, symmetric
// SCCs, the empty and self-loop edges): exactly the subtle-invariant class that
// has bitten this engine before. `bytes` is illustrative (baked), not a run.
const LCK_CORPUS = [
  {
    id: "f(1,2) vs f(2,1)",
    shape: "argument order kept at the constants facet",
    bytes: "b3:1f7a4cd0",
    why: "the SCHEMA_VERSION 2 bump fixed a collision here; the corpus pins it so neither core can regress it.",
  },
  {
    id: "flat-edge-role swap",
    shape: "two edges whose roles are exchanged",
    bytes: "b3:9c20e1b8",
    why: "another SCHEMA_VERSION 2 case: role is part of the canonical key, so the swap must move the digest.",
  },
  {
    id: "symmetric SCC",
    shape: "a cycle whose nodes are pairwise interchangeable",
    bytes: "b3:4e83fa17",
    why: "exercises CondenseScc plus the Weisfeiler-Leman fold; isomorphic components must hash equal, WL-equivalent ones may collide by design.",
  },
  {
    id: "duplicate (ordinal,label)",
    shape: "two children sharing an ordinal and label",
    bytes: "b3:70b6c95d",
    why: "forces the child sort to break ties on digest, not on key, so AnonymousShape stays name-blind.",
  },
  {
    id: "empty graph",
    shape: "no nodes, no edges",
    bytes: "b3:00d1aa30",
    why: "the degenerate framing case: the magic plus schema_version header must still commit.",
  },
  {
    id: "single self-loop",
    shape: "one node, one recursive edge to itself",
    bytes: "b3:c4f209ee",
    why: "the smallest cycle: the recursive-edge path and the cycle policy both fire on one node.",
  },
];

// The two cores on the bench. Same input corpus, same blake3 library, output
// bytes required equal in CI. Rust is the engine that ships; Lean is the
// executable spec where the rules are also theorems.
const LCK_CORES = [
  {
    k: "rust",
    t: "Rust",
    sub: "the engine that ships",
    note: "riff-catalog-core: the dimension-tagged graph model, the canonical encoder (MAGIC plus schema_version framing), the bottom-up fold, the facet address, the SCC condensation. About 1200 to 1600 LOC, pure and deterministic.",
  },
  {
    k: "lean",
    t: "Lean 4",
    sub: "the rules as theorems",
    note: "a Lean executable spec mirroring the same core, built to run (lake exe) and reproduce the same digests once the toolchain lands. The laws below are what it is built to state and prove over this model.",
  },
];

// The laws Lean would carry (spike section 2), easiest to hardest. status is
// the honest state: `proved` = a theorem about the abstract construction once
// the spec lands; `axiom` = a named assumption, not a theorem. Nothing here is
// claimed run today.
const LCK_LAWS = [
  {
    k: "det",
    t: "determinism",
    status: "proved",
    says: "same (policy, graph) gives the same digest; insensitive to field and edge insertion order and to internal node-id choice. The Rust gets this from sorting; the spec shows the sorts canonicalize.",
  },
  {
    k: "lattice",
    t: "facet refinement lattice",
    status: "proved",
    says: "equal at a finer facet implies equal at every coarser one. A facet address over a dimension set commits the sorted per-dimension digests, so equality at the larger set forces equality at every subset. This is the lattice the facet UI and the claims layer lean on.",
  },
  {
    k: "anchor",
    t: "anchor / transport soundness",
    status: "proved",
    says: "a fact transports along a facet address exactly when the facet covers the fact's footprint. The per-dimension digest for dimension d is a function only of the d-tagged fields plus the skeleton: one-directional dimension purity.",
  },
  {
    k: "anon",
    t: "AnonymousShape correctness",
    status: "target",
    says: "names never enter the digest. Renaming all node keys by any injection leaves every AnonymousShape digest unchanged. The deepest correctness law, touching the tree fold, the graph record, and the whole WL path. The prototype got this wrong before.",
  },
  {
    k: "term",
    t: "SCC / WL termination",
    status: "target",
    says: "SCC condensation is well-defined (Tarjan gives a partition, emitted reverse-topologically) and WL refinement terminates (monotone partition refinement, capped at the member count). Honest caveat: 1-WL is incomplete on pathological regular graphs, so anonymous equality is WL-equivalence under policy, not isomorphism.",
  },
  {
    k: "inj",
    t: "encoding injectivity",
    status: "target",
    says: "the canonical byte encoding is injective: distinct graphs at a facet produce distinct pre-image bytes, from length prefixes plus domain-separating tags. So distinct digests imply distinct shapes, unless blake3 collides.",
  },
];

// The trust tiers, kept apart on purpose (the overclaim lives in blurring them).
const LCK_TIERS = [
  {
    k: "checked",
    t: "kernel-checked",
    s: "three of the six laws above are now theorems verified by the Lean 4 kernel (determinism, the refinement lattice, anchor transport); the other three are stated targets. Checked, not merely tested.",
  },
  {
    k: "axiom",
    t: "one named assumption",
    s: "blake3 collision-resistance is a Lean axiom, not a theorem. Everything is sound GIVEN a collision-free hash. The same blake3 C library runs on both sides, so it is never re-implemented and never drifts. Named in the open.",
  },
  {
    k: "open",
    t: "the open gap",
    s: "the laws are about the Lean model of the encoder. The shipping Rust is held to it by the golden vectors and by reading, not yet by verified extraction (that is the research-grade Aeneas/Kani follow-on). Stated up front.",
  },
];

// Honest scope line, always visible: what the substrate already is, and what the
// chapter does NOT claim.
const LCK_SCOPE =
  "The golden vectors are the actual lockstep, and the existing golden.rs and dual-path conformance.rs already do the within-language version of this byte-for-byte check. The Lean third path now exists and passes: a Lean port reproduces this corpus byte for byte, 254 checks green, on a pure-Lean blake3 checked against the reference vectors. So the lockstep is real, not a plan. Three of the six laws above are now proved kernel-clean (determinism, the refinement lattice, anchor transport); the other three are stated targets, and blake3 collision-resistance is a named axiom. A live Lean run needs the deferred toolchain, so the digests shown here are baked, not computed in your browser.";

customElements.define(
  "lockstep-bench",
  class extends HTMLElement {
    connectedCallback() {
      // renders synchronously: static cited chapter, no engine call, no Lean run.
      this.sel = 0; // selected corpus row
      this.render();
    }
    render() {
      const idle =
        "Hover a corpus row to see the adversarial shape it pins, a law to read what Lean proves, or a trust tier to keep the three apart. Nothing here runs live: it is the spike-note plan over the existing golden substrate.";

      // (1) the bench: corpus rows, each with a chip per core, required equal.
      const rows = LCK_CORPUS.map((c, i) => {
        const col = lckChip(c.bytes);
        const chip = (coreK) =>
          `<span class="shapedot lck-dot" style="--chip:${col}" title="${coreK} digest ${lckShort(c.bytes)} (baked, illustrative)"></span>`;
        return (
          `<tr class="lck-row ${i === this.sel ? "on" : ""}" data-i="${i}">` +
          `<td class="lck-shape"><code>${c.id}</code> <span class="lck-sub">${c.shape}</span></td>` +
          `<td class="lck-cell">${chip("rust")}<span class="lck-hex">${lckShort(c.bytes)}</span></td>` +
          `<td class="lck-eq" title="required equal in CI">=</td>` +
          `<td class="lck-cell">${chip("lean")}<span class="lck-hex">${lckShort(c.bytes)}</span></td></tr>`
        );
      }).join("");
      const heads = LCK_CORES.map(
        (cr) =>
          `<div class="lck-core lck-core-${cr.k}" data-core="${cr.k}"><b>${cr.t}</b><span class="lck-coresub">${cr.sub}</span></div>`,
      ).join('<div class="lck-vs">held to the same bytes</div>');

      // (2) the laws ledger.
      const laws = LCK_LAWS.map(
        (l) =>
          `<button class="lck-law" data-law="${l.k}"><span class="lck-law-st lck-st-${l.status}">${l.status === "proved" ? "proved" : l.status === "axiom" ? "axiom" : "to prove"}</span>${l.t}</button>`,
      ).join("");

      // (3) the trust tiers.
      const tiers = LCK_TIERS.map(
        (t) =>
          `<span class="lck-tier lck-tier-${t.k}" data-tier="${t.k}">${t.t}</span>`,
      ).join("");

      this.innerHTML = `
      <div class="lck-bench codepanel">
        <div class="cphead lck-head"><b>one corpus, two cores</b> <span class="lck-sub">the output bytes must match, exactly, in CI &middot; baked here, checked green in the Lean port</span></div>
        <div class="lck-cores">${heads}</div>
        <table class="vledger lck-ledger">
          <thead><tr><th>canonical graph (corpus input)</th><th>Rust digest</th><th></th><th>Lean digest</th></tr></thead>
          <tbody>${rows}</tbody>
        </table>
        <p class="lck-cap">Both sides call the same blake3 C library, so the hash is identical bytes. The corpus is the actual lockstep: byte-equality on a fixed hash is exact and has no oracle ambiguity. Its strength is the corpus's coverage, so it is seeded from the adversarial shapes that have bitten this engine before.</p>
      </div>
      <div class="lck-laws-wrap">
        <div class="lck-laws-label">above the bytes, Lean carries the laws the product rests on</div>
        <div class="lck-laws">${laws}</div>
      </div>
      <div class="lck-tiers-wrap">
        <div class="lck-tiers-label">three tiers, kept apart on purpose</div>
        <div class="lck-tiers">${tiers}</div>
      </div>
      <div class="eqread" data-idle="${idle}">${idle}</div>
      <p class="twnote">${LCK_SCOPE} The self-referential part is on theme: the claim that the Lean rules equal the Rust bytes on this corpus is itself an <b>anchored equivalence</b>, the exact shape riffcat is built to record. A claim, at a stated facet (the byte-identity of the digest function over the corpus), against the corpus root as the anchor, modulo one named assumption (the blake3 axiom). riffcat could ingest its own two cores and address them. Honestly: the proofs are mostly confidence and ecosystem fit, the golden vectors are the load-bearing lockstep, and the real bug class they catch is the subtle-invariant kind, names leaking into a digest, an order that fails to canonicalize, that has slipped past review on this engine before.</p>`;

      const read = this.querySelector(".eqread");
      const idleHTML = read.dataset.idle;

      this.querySelectorAll(".lck-row").forEach((row) =>
        row.addEventListener("mouseenter", () => {
          const c = LCK_CORPUS[+row.dataset.i];
          this.querySelectorAll(".lck-row").forEach((x) =>
            x.classList.toggle("lit", x === row),
          );
          read.innerHTML = `<span class="sw" style="background:${lckChip(c.bytes)}"></span><b>${c.id}</b> &middot; ${c.why}`;
        }),
      );
      this.querySelectorAll(".lck-core").forEach((el) =>
        el.addEventListener("mouseenter", () => {
          const cr = LCK_CORES.find((x) => x.k === el.dataset.core);
          read.innerHTML = `<b>${cr.t}</b> &middot; ${cr.note}`;
        }),
      );
      this.querySelectorAll(".lck-law").forEach((el) =>
        el.addEventListener("mouseenter", () => {
          const l = LCK_LAWS.find((x) => x.k === el.dataset.law);
          const tag = l.status === "axiom" ? "named assumption" : "proved law";
          read.innerHTML = `<b>${l.t}</b> <span class="lck-read-st">(${tag})</span> &middot; ${l.says}`;
        }),
      );
      this.querySelectorAll(".lck-tier").forEach((el) =>
        el.addEventListener("mouseenter", () => {
          const t = LCK_TIERS.find((x) => x.k === el.dataset.tier);
          read.innerHTML = `<b>${t.t}</b> &middot; ${t.s}`;
        }),
      );
      this.addEventListener("mouseleave", () => {
        this.querySelectorAll(".lck-row").forEach((x) =>
          x.classList.remove("lit"),
        );
        read.innerHTML = idleHTML;
      });
    }
  },
);

// the fold (merklization chapter A). Shows how a facet address is MADE: a unit
// lowers to a graph; per-node digests are computed bottom-up in layers
// (node.local = context-free, by invariant I3; node.tree = the Merkle fold over
// children in skeleton order; graph.full = the whole-graph digest). Then a facet
// (a subset of dimensions) is chosen and "equal at a facet" is the address.
//
// LIVE: the wasm wrapper now exports window.wasmBindings.node_digests(fixtureJson,
// mode), a thin serializer over the per-node NodeHashes that digest_graph already
// computes (no new hashing): per-node local/tree, per-component digests, and the
// graph digest, in the shape FOLD_SHAPE documents below. The component lowers a
// baked solc fixture (b.fixture("demo")), folds one small fn-like unit, and
// renders those real digests. If the export is ever missing or errors, it falls
// back to the baked example (FOLD_BAKED) and the copy says so, so the chapter is
// safe either way; the live path is the default.
//
// All top-level names are prefixed `fold` to avoid collisions with app.js. Reuses
// engineReady, chipColor, the .dialbar/.ladder/.stop/.eqread/.twnote/.live-note
// classes and the SVG .latedge/.latarrhead theme; new visuals are the .fold*
// classes in the css field.

// The facet ladder: each facet is a SUBSET of dimensions. Named by what it
// FORGETS, matching the engine's named constructors (structure_only,
// names_blind). Ordered fine -> coarse so sliding right forgets more, the dial
// metaphor used everywhere else.
const FOLD_FACETS = [
  {
    key: "full",
    label: "full",
    dims: ["structure", "names", "constants", "types"],
    gloss: "every dimension counts",
  },
  {
    key: "names-blind",
    label: "names-blind",
    dims: ["structure", "constants", "types"],
    gloss: "names dropped; constants and types still count",
  },
  {
    key: "structure",
    label: "structure-only",
    dims: ["structure"],
    gloss: "shape only; names, constants, and types all dropped",
  },
];

// The JSON shape the wasm export node_digests(fixtureJson, mode) returns, the
// shape this component (and the baked FOLD_BAKED stand-in) consumes:
//   {
//     nodes:   [ { id:str, kind:str, parent:str|null, order:int,
//                  local:{ <dim>:hex }, tree:{ <dim>:hex } }, ... ],
//     components: [ { index:int, members:[str], digests:{ <dim>:hex } }, ... ],
//     graph:   { <dim>:hex }
//   }
// `id` is each node's stable canonical key (a string); `parent` is the parent's
// id, or null at the root. `local` mirrors NodeHashes.local (context-free,
// invariant I3), `tree` mirrors NodeHashes.tree (the Merkle subtree fold),
// `graph` mirrors GraphHashes.graph, `components` mirrors GraphHashes.components
// (singletons on this acyclic unit; carried for the locality chapter). `order` is
// the child's position in its parent's skeleton, so the fold is deterministic.
// The live payload carries no (x,y); foldLayout assigns those for the SVG. The
// baked FOLD_BAKED uses small integer ids and hand-placed (x,y) for the same
// shape; both render identically.
const FOLD_SHAPE =
  "node_digests(fixtureJson, mode) -> the shape documented above";

// A small lowered unit: a tiny function with two statements and a handful of leaf
// nodes, laid out by hand so the bottom-up fold reads in layers. Ids are stable;
// (x,y) are SVG coordinates; parent/order define the skeleton. The digests are
// illustrative stand-ins (8 hex chars), only used when the live export is absent:
// the engine computes the real fold. The point of the example is the SHAPE of the
// fold, not these specific bytes, and the copy says so.
const FOLD_BAKED = {
  nodes: [
    {
      id: 0,
      kind: "fn",
      parent: null,
      order: 0,
      x: 360,
      y: 60,
      local: {
        structure: "a1b2c3d4",
        names: "11119999",
        constants: "00000000",
        types: "ee00ee00",
      },
      tree: {
        structure: "f0f01d00",
        names: "f0f01d11",
        constants: "f0f01d22",
        types: "f0f01d33",
      },
    },
    {
      id: 1,
      kind: "assign",
      parent: 0,
      order: 0,
      x: 200,
      y: 152,
      local: {
        structure: "b2c3d4e5",
        names: "22228888",
        constants: "00000000",
        types: "ee11ee11",
      },
      tree: {
        structure: "a1f01d00",
        names: "a1f01d11",
        constants: "a1f01d22",
        types: "a1f01d33",
      },
    },
    {
      id: 2,
      kind: "return",
      parent: 0,
      order: 1,
      x: 520,
      y: 152,
      local: {
        structure: "c3d4e5f6",
        names: "33337777",
        constants: "00000000",
        types: "ee22ee22",
      },
      tree: {
        structure: "b2f01d00",
        names: "b2f01d11",
        constants: "b2f01d22",
        types: "b2f01d33",
      },
    },
    {
      id: 3,
      kind: "var x",
      parent: 1,
      order: 0,
      x: 120,
      y: 252,
      local: {
        structure: "d4e5f607",
        names: "4444aaaa",
        constants: "00000000",
        types: "ee33ee33",
      },
      tree: {
        structure: "d4e5f607",
        names: "4444aaaa",
        constants: "00000000",
        types: "ee33ee33",
      },
    },
    {
      id: 4,
      kind: "add",
      parent: 1,
      order: 1,
      x: 280,
      y: 252,
      local: {
        structure: "e5f60718",
        names: "5555bbbb",
        constants: "00000000",
        types: "ee44ee44",
      },
      tree: {
        structure: "e5f01d00",
        names: "e5f01d11",
        constants: "e5f01d22",
        types: "e5f01d33",
      },
    },
    {
      id: 5,
      kind: "const 7",
      parent: 4,
      order: 0,
      x: 232,
      y: 340,
      local: {
        structure: "99887766",
        names: "00000000",
        constants: "7c057707",
        types: "ee55ee55",
      },
      tree: {
        structure: "99887766",
        names: "00000000",
        constants: "7c057707",
        types: "ee55ee55",
      },
    },
    {
      id: 6,
      kind: "var y",
      parent: 4,
      order: 1,
      x: 344,
      y: 340,
      local: {
        structure: "d4e5f607",
        names: "6666cccc",
        constants: "00000000",
        types: "ee33ee33",
      },
      tree: {
        structure: "d4e5f607",
        names: "6666cccc",
        constants: "00000000",
        types: "ee33ee33",
      },
    },
    {
      id: 7,
      kind: "var x",
      parent: 2,
      order: 0,
      x: 520,
      y: 252,
      local: {
        structure: "d4e5f607",
        names: "4444aaaa",
        constants: "00000000",
        types: "ee33ee33",
      },
      tree: {
        structure: "d4e5f607",
        names: "4444aaaa",
        constants: "00000000",
        types: "ee33ee33",
      },
    },
  ],
  components: [],
  graph: {
    structure: "9f01d9ce",
    names: "9f01d9c1",
    constants: "9f01d9c2",
    types: "9f01d9c3",
  },
};

// The graph digest renders as a synthetic node beneath the root in the SVG.
const FOLD_GRAPH_NODE = { id: "graph", x: 360, y: 408 };

// Layer assignment for the bottom-up fold: a node's layer is its max depth below.
// Leaves are layer 0; a parent is one past its deepest child. The fold reveals one
// layer at a time, deepest first, so a tree digest only appears once every child's
// tree digest already has. graph.full is the final layer.
function foldLayers(nodes) {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const kids = new Map(nodes.map((n) => [n.id, []]));
  for (const n of nodes) if (n.parent != null) kids.get(n.parent).push(n.id);
  const depth = new Map();
  const visit = (id) => {
    if (depth.has(id)) return depth.get(id);
    const cs = kids.get(id);
    const d = cs.length === 0 ? 0 : 1 + Math.max(...cs.map(visit));
    depth.set(id, d);
    return d;
  };
  nodes.forEach((n) => visit(n.id));
  const maxD = Math.max(...nodes.map((n) => depth.get(n.id)));
  // layer index in reveal order: leaves first (layer 0), root last, matching the
  // engine's bottom-up fold (a parent digest folds its children's tree digests).
  const layerOf = (n) => depth.get(n.id);
  return { layerOf, layerCount: maxD + 1, byId, kids };
}

// Place nodes in the SVG when the payload carries none. The baked example hand-
// codes (x,y) for a pretty layout; the live engine export carries only the tree
// (id/kind/parent/order), so we lay it out here: depth sets the row (root at the
// top, leaves at the bottom), and siblings spread evenly across the width in
// skeleton (parent, order) order. Same viewBox the baked layout targets (720 x
// 446), with a top/bottom margin so the synthetic graph.full node still fits
// below the root. Mutates the nodes in place; a no-op once every node has an x.
function foldLayout(nodes) {
  if (nodes.every((n) => typeof n.x === "number")) return;
  const byId = new Map(nodes.map((n) => [n.id, n]));
  const kids = new Map(nodes.map((n) => [n.id, []]));
  let root = null;
  for (const n of nodes) {
    if (n.parent != null && kids.has(n.parent)) kids.get(n.parent).push(n);
    else root = n;
  }
  // Children in skeleton order so the drawing matches the fold order.
  for (const cs of kids.values())
    cs.sort((a, b) => (a.order || 0) - (b.order || 0));
  const depthOf = new Map();
  const setDepth = (n, d) => {
    depthOf.set(n.id, d);
    for (const c of kids.get(n.id)) setDepth(c, d + 1);
  };
  if (root) setDepth(root, 0);
  const maxDepth = Math.max(0, ...nodes.map((n) => depthOf.get(n.id) || 0));
  // Order leaves left-to-right by a depth-first walk, then place every node at
  // the average x of its own leaves so parents sit centered over their subtree.
  const VIEW_W = 720,
    TOP = 60,
    ROWH = maxDepth > 0 ? Math.min(96, (372 - TOP) / maxDepth) : 0;
  const leaves = [];
  const collect = (n) => {
    const cs = kids.get(n.id);
    if (cs.length === 0) leaves.push(n);
    else cs.forEach(collect);
  };
  if (root) collect(root);
  const slot = VIEW_W / (leaves.length + 1);
  leaves.forEach((n, i) => {
    n._lx = slot * (i + 1);
  });
  const place = (n) => {
    const cs = kids.get(n.id);
    if (cs.length === 0) {
      n.x = Math.round(n._lx);
    } else {
      cs.forEach(place);
      n.x = Math.round(cs.reduce((s, c) => s + c.x, 0) / cs.length);
    }
    n.y = Math.round(TOP + (depthOf.get(n.id) || 0) * ROWH);
  };
  if (root) place(root);
  for (const n of nodes) {
    delete n._lx;
    if (typeof n.x !== "number") {
      n.x = 360;
      n.y = 60;
    }
  }
}

const foldShort = (h) => (h || "").slice(0, 8);

customElements.define(
  "fold-merkle",
  class extends HTMLElement {
    async connectedCallback() {
      this.facet = "full";
      this.step = 0; // 0 = locals only; then one step per tree layer; final = graph.full
      this.live = false;
      this.innerHTML = `<p class="live-note">booting the wasm engine\u2026</p>`;
      let data = FOLD_BAKED;
      try {
        const b = await engineReady;
        // The assumed thin export. Present once the wasm wrapper serializes the
        // per-node digests digest_graph already computes; absent today, so we fall
        // through to the baked example and say so in the readout.
        if (b && typeof b.node_digests === "function" && b.fixture) {
          const fx = b.fixture("demo");
          if (fx) {
            const parsed = JSON.parse(b.node_digests(fx, "shape"));
            if (parsed && parsed.nodes && parsed.nodes.length) {
              data = parsed;
              this.live = true;
            }
          }
        }
      } catch (_) {
        /* keep the baked example; the copy labels it */
      }
      this.data = data;
      // The live engine export carries the tree but no coordinates; lay it out so
      // the SVG can draw it. The baked example already has (x,y), so this is a
      // no-op there.
      foldLayout(data.nodes);
      this.layers = foldLayers(data.nodes);
      // total steps: 1 (locals) + layerCount (tree layers) + 1 (graph)
      this.maxStep = 1 + this.layers.layerCount;
      this.render();
    }

    // Which dimensions does the current facet keep, in canonical order.
    facetDims() {
      return FOLD_FACETS.find((f) => f.key === this.facet).dims;
    }

    // The address of a node (or the graph) at the current facet: join the kept
    // dimensions' short digests. This is the "equal at a facet" value.
    addressOf(dimsMap) {
      return this.facetDims()
        .map((d) => foldShort(dimsMap[d] || ""))
        .join("\u00b7");
    }

    // Reveal state for a node: at step 0 only locals show; a node's tree digest
    // appears when the step has reached its layer (deepest first).
    nodeState(n) {
      if (this.step === 0) return "local";
      return this.step >= this.layers.layerOf(n) + 1 ? "tree" : "local";
    }
    graphRevealed() {
      return this.step >= this.maxStep;
    }

    render() {
      const d = this.data;
      const ladder = FOLD_FACETS.map(
        (f) =>
          `<span class="stop ${f.key === this.facet ? "on" : ""}" data-facet="${f.key}">${f.label}</span>`,
      ).join("");

      // Edges: parent -> child, from the skeleton.
      const edges = d.nodes
        .filter((n) => n.parent != null)
        .map((n) => {
          const p = this.layers.byId.get(n.parent);
          return `<line x1="${p.x}" y1="${p.y + 16}" x2="${n.x}" y2="${n.y - 16}" class="latedge"/>`;
        })
        .join("");
      // Root -> graph edge (dashed), shown once the graph digest lands.
      const root = d.nodes.find((n) => n.parent == null);
      const graphEdge = `<line x1="${root.x}" y1="${root.y + 16}" x2="${FOLD_GRAPH_NODE.x}" y2="${FOLD_GRAPH_NODE.y - 18}" class="foldgraphedge ${this.graphRevealed() ? "on" : ""}"/>`;

      const nodeSvg = d.nodes
        .map((n) => {
          const st = this.nodeState(n);
          const map = st === "tree" ? n.tree : n.local;
          const col = chipColor(
            this.facetDims()
              .map((dim) => map[dim] || "")
              .join(""),
          );
          const hex = this.facetDims()
            .map((dim) => foldShort(map[dim] || "").slice(0, 4))
            .join("")
            .slice(0, 8);
          return (
            `<g class="foldnode ${st}" data-id="${n.id}" transform="translate(${n.x},${n.y})">` +
            `<rect x="-56" y="-16" width="112" height="32" rx="5"/>` +
            `<circle class="folddot" cx="-42" cy="0" r="5" style="fill:${col}"/>` +
            `<text class="foldkind" x="6" y="-2" text-anchor="middle">${n.kind}</text>` +
            `<text class="foldhex" x="6" y="11" text-anchor="middle">${hex}</text>` +
            `</g>`
          );
        })
        .join("");

      const gv = this.graphRevealed();
      const gcol = chipColor(
        this.facetDims()
          .map((dim) => d.graph[dim] || "")
          .join(""),
      );
      const graphSvg =
        `<g class="foldnode graph ${gv ? "on" : ""}" data-id="graph" transform="translate(${FOLD_GRAPH_NODE.x},${FOLD_GRAPH_NODE.y})">` +
        `<rect x="-78" y="-18" width="156" height="36" rx="6"/>` +
        `<circle class="folddot" cx="-60" cy="0" r="6" style="fill:${gcol}"/>` +
        `<text class="foldkind" x="10" y="-3" text-anchor="middle">graph.full</text>` +
        `<text class="foldhex" x="10" y="11" text-anchor="middle">${gv ? this.addressOf(d.graph) : "not folded yet"}</text>` +
        `</g>`;

      // Readout: where we are in the fold, in words.
      const facetMeta = FOLD_FACETS.find((f) => f.key === this.facet);
      let phase;
      if (this.step === 0)
        phase = `step 1: every node gets its <b>local</b> digest, its own content for the kept dimensions only, with <b>no context</b> (invariant I3).`;
      else if (!gv)
        phase = `step ${this.step + 1}: a parent folds its local content with its children's <b>tree</b> digests in skeleton order; deeper layers settled first.`;
      else
        phase = `last step: the kept node trees fold into one <b>graph.full</b> address. That join is the facet address: <b>equal at ${facetMeta.label}</b> means equal here.`;
      const src = this.live
        ? `per-node digests are computed <b>live</b> by the engine (riffcat, in this page as wasm), not baked`
        : `per-node digests shown are a <b>baked illustrative</b> example (the live per-node read fell through; see the note)`;
      const idle = `facet <b>${facetMeta.label}</b>: ${facetMeta.gloss} \u00b7 ${phase} \u00b7 ${src}`;

      const prov = this.live
        ? ` This unit is folded <b>live</b> in your browser: the engine lowers a baked solc fixture, computes the per-node <code>local</code>/<code>tree</code> digests inside <code>digest_graph</code>, and the wasm wrapper serializes them straight to this view, so the digests above are the real bytes, not stand-ins.`
        : ` The digests above are a <b>baked illustrative</b> example: the engine computes these per-node digests inside <code>digest_graph</code>, and the wasm wrapper can serialize them, but the live read fell through here, so these specific bytes are stand-ins for the shape of the fold.`;
      const atStart = this.step === 0,
        atEnd = this.step >= this.maxStep;
      this.innerHTML = `
      <div class="dialbar">
        <div class="grp"><span>fold</span>
          <button class="foldbtn" data-act="reset" ${atStart ? "disabled" : ""}>\u21ba reset</button>
          <button class="foldbtn" data-act="prev" ${atStart ? "disabled" : ""}>\u2190 back</button>
          <button class="foldbtn" data-act="next" ${atEnd ? "disabled" : ""}>step \u2192</button>
          <button class="foldbtn" data-act="play" ${atEnd ? "disabled" : ""}>play \u25b6</button>
        </div>
        <div class="grp"><span>facet</span><div class="ladder">${ladder}</div></div>
      </div>
      <svg viewBox="0 0 720 446" class="foldsvg" role="img" aria-label="a lowered unit folding bottom-up into a facet address">
        <defs><marker id="foldarr" markerWidth="9" markerHeight="9" refX="6" refY="3" orient="auto"><path d="M0,0 L6,3 L0,6 z" class="latarrhead"/></marker></defs>
        <text x="628" y="36" text-anchor="end" class="foldcap">leaves at the bottom \u00b7 root at the top \u00b7 graph.full below</text>
        ${edges}
        ${graphEdge}
        ${nodeSvg}
        ${graphSvg}
      </svg>
      <div class="eqread" data-idle="${idle}">${idle}</div>
      <p class="twnote">Each node is hashed alone first (invariant I3, no context), then a parent folds its own content with its children's tree digests, leaves first, root last, and graph.full lands at the end.${prov}</p>`;

      this.read = this.querySelector(".eqread");
      this.querySelectorAll("[data-facet]").forEach((s) =>
        s.addEventListener("click", () => {
          if (this.facet !== s.dataset.facet) {
            this.facet = s.dataset.facet;
            this.render();
          }
        }),
      );
      this.querySelectorAll("[data-act]").forEach((b) =>
        b.addEventListener("click", () => this.act(b.dataset.act)),
      );

      // Hover a node to read its local vs tree digests at the current facet.
      this.querySelectorAll(".foldnode").forEach((g) =>
        g.addEventListener("mouseenter", () => this.describe(g)),
      );
      this.addEventListener("mouseleave", () => {
        if (this.read) this.read.innerHTML = this.read.dataset.idle;
      });
    }

    describe(g) {
      if (!this.read) return;
      const id = g.dataset.id;
      if (id === "graph") {
        this.read.innerHTML = this.graphRevealed()
          ? `<b>graph.full</b> at ${this.facet}: <span style="color:var(--warm)">${this.addressOf(this.data.graph)}</span> \u00b7 this is the facet address every artifact with this shape shares`
          : `<b>graph.full</b> has not folded yet \u00b7 step to the end`;
        return;
      }
      const n = this.layers.byId.get(+id);
      const treeShown = this.nodeState(n) === "tree";
      this.read.innerHTML =
        `<b>${n.kind}</b> \u00b7 local <span style="color:var(--cool)">${this.addressOf(n.local)}</span>` +
        (treeShown
          ? ` \u00b7 tree <span style="color:var(--warm)">${this.addressOf(n.tree)}</span> (local folded with its children)`
          : ` \u00b7 tree not folded yet at this step`) +
        ` \u00b7 dimensions kept: ${this.facetDims().join(", ")}`;
    }

    act(a) {
      if (this._timer) {
        clearInterval(this._timer);
        this._timer = null;
      }
      if (a === "reset") {
        this.step = 0;
        this.render();
        return;
      }
      if (a === "prev") {
        this.step = Math.max(0, this.step - 1);
        this.render();
        return;
      }
      if (a === "next") {
        this.step = Math.min(this.maxStep, this.step + 1);
        this.render();
        return;
      }
      if (a === "play") {
        // Respect reduced-motion: jump to the end rather than animate.
        if (
          window.matchMedia &&
          window.matchMedia("(prefers-reduced-motion: reduce)").matches
        ) {
          this.step = this.maxStep;
          this.render();
          return;
        }
        this.step = 0;
        this.render();
        this._timer = setInterval(() => {
          if (this.step >= this.maxStep) {
            clearInterval(this._timer);
            this._timer = null;
            return;
          }
          this.step += 1;
          this.render();
        }, 850);
      }
    }
    disconnectedCallback() {
      if (this._timer) clearInterval(this._timer);
    }
  },
);

// "where it came from" (nav: provenance). LIVE. Two thin bindings over the
// facade compute everything the chapter shows:
//   origin_shape(specJson)                 -> the structural address WITH and WITHOUT
//                                             the EdgeRole::Origin provenance
//                                             edges, plus the engine's own
//                                             match verdict.
//   origin_containment(a, b, "structure")  -> the containment fraction of two
//                                             lowerings and, engine-derived,
//                                             exactly which nodes diverge.
// The JS only assembles the input graphs (a synthetic stand-in for fe's origin
// facts); no address, fraction, or divergence verdict is computed here. If the
// bindings are missing the chapter says so rather than baking a result. Reuses
// .dialbar/.ladder/.stop/.eqread/.twnote and the .latarrhead marker theme; new
// visuals are the .prov* rules in index.html.

// Beat 1 input: a source expression and its lowered MIR, one bundle. The two
// subtrees are the shape; `origin` is the attribution that rides along. Extra
// per-node keys (lab/stage/x/y) are ignored by the Rust deserializer and used
// only to draw.
const PROV_B1 = {
  owner: "pkg:token",
  unit: "transfer",
  unit_kind: "mir.body",
  nodes: [
    {
      id: "src_ret",
      kind: "src.return",
      lab: "return",
      stage: "src",
      x: 150,
      y: 60,
    },
    {
      id: "src_add",
      kind: "src.binexpr",
      fields: [["structure", "op", "+"]],
      lab: "a + b",
      stage: "src",
      x: 150,
      y: 142,
    },
    {
      id: "src_a",
      kind: "src.name",
      fields: [["names", "name", "a"]],
      lab: "a",
      stage: "src",
      x: 100,
      y: 224,
    },
    {
      id: "src_b",
      kind: "src.name",
      fields: [["names", "name", "b"]],
      lab: "b",
      stage: "src",
      x: 200,
      y: 224,
    },
    { id: "body", kind: "mir.body", lab: "body", stage: "mir", x: 556, y: 60 },
    {
      id: "add",
      kind: "mir.add",
      fields: [["structure", "op", "add"]],
      lab: "add",
      stage: "mir",
      x: 500,
      y: 142,
    },
    { id: "ret", kind: "mir.ret", lab: "ret", stage: "mir", x: 624, y: 142 },
    { id: "la", kind: "mir.local", lab: "a", stage: "mir", x: 452, y: 224 },
    { id: "lb", kind: "mir.local", lab: "b", stage: "mir", x: 556, y: 224 },
  ],
  children: [
    ["src_ret", "expr", 0, "src_add"],
    ["src_add", "lhs", 0, "src_a"],
    ["src_add", "rhs", 1, "src_b"],
    ["body", "stmt", 0, "add"],
    ["add", "lhs", 0, "la"],
    ["add", "rhs", 1, "lb"],
    ["body", "stmt", 1, "ret"],
  ],
  edges: [],
  origin: [
    ["body", "lowered_from", "src_ret"],
    ["add", "lowered_from", "src_add"],
    ["la", "lowered_from", "src_a"],
    ["lb", "lowered_from", "src_b"],
    ["ret", "lowered_from", "src_ret"],
  ],
};

// Beat 2 input: two lowerings of one source (p = hash(a, b); q = a * 2; return
// p + q) that differ only in whether q's multiply was strength-reduced. B is
// derived from A so they truly differ at one operator (mir.mul -> mir.shl) and
// its literal, and nowhere else.
const PROV_B2_A = {
  owner: "pkg:token",
  unit: "scale",
  unit_kind: "mir.body",
  nodes: [
    { id: "body", kind: "mir.body" },
    { id: "s0", kind: "mir.assign" },
    { id: "call", kind: "mir.call" },
    { id: "ca", kind: "mir.local" },
    { id: "cb", kind: "mir.local" },
    { id: "s1", kind: "mir.assign" },
    { id: "op", kind: "mir.mul" },
    { id: "ma", kind: "mir.local" },
    { id: "k", kind: "mir.const", fields: [["constants", "value", "2"]] },
    { id: "s2", kind: "mir.return" },
    { id: "add", kind: "mir.add" },
    { id: "pu", kind: "mir.local" },
    { id: "qu", kind: "mir.local" },
  ],
  children: [
    ["body", "stmt", 0, "s0"],
    ["s0", "expr", 0, "call"],
    ["call", "arg", 0, "ca"],
    ["call", "arg", 1, "cb"],
    ["body", "stmt", 1, "s1"],
    ["s1", "expr", 0, "op"],
    ["op", "lhs", 0, "ma"],
    ["op", "rhs", 1, "k"],
    ["body", "stmt", 2, "s2"],
    ["s2", "expr", 0, "add"],
    ["add", "lhs", 0, "pu"],
    ["add", "rhs", 1, "qu"],
  ],
  edges: [],
  origin: [],
};
const PROV_B2_B = JSON.parse(JSON.stringify(PROV_B2_A));
PROV_B2_B.nodes.find((n) => n.id === "op").kind = "mir.shl";
PROV_B2_B.nodes.find((n) => n.id === "k").fields = [
  ["constants", "value", "1"],
];

// Display scaffold for the two lowerings: the three source statements, with the
// operator node id whose divergence flags the row and the source span its
// provenance records. Marking is driven by the engine's divergent-node set, not
// by this table.
const PROV_B2_ROWS = [
  {
    id: "s0",
    op: "call",
    a: "p = hash(a, b)",
    b: "p = hash(a, b)",
    src: "hash(a, b)",
  },
  { id: "s1", op: "op", a: "q = a * 2", b: "q = a &lt;&lt; 1", src: "a * 2" },
  { id: "s2", op: "add", a: "return p + q", b: "return p + q", src: "p + q" },
];

customElements.define(
  "provenance-rides",
  class extends HTMLElement {
    async connectedCallback() {
      this.view = "rides"; // "rides" (beat 1) | "diverge" (beat 2)
      this.prov = true; // beat 1: are the origin edges drawn
      this.b1 = null;
      this.b2 = null;
      this.err = null;
      this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
      try {
        const b = await engineReady;
        if (
          b &&
          typeof b.origin_shape === "function" &&
          typeof b.origin_containment === "function"
        ) {
          this.b1 = JSON.parse(b.origin_shape(JSON.stringify(PROV_B1)));
          this.b2 = JSON.parse(
            b.origin_containment(
              JSON.stringify(PROV_B2_A),
              JSON.stringify(PROV_B2_B),
              "structure",
            ),
          );
        } else {
          this.err =
            "the origin bindings are not on window.wasmBindings in this build";
        }
      } catch (e) {
        this.err = String((e && e.message) || e);
      }
      this.render();
    }

    render() {
      const tabs = [
        ["rides", "origin links"],
        ["diverge", "compare two lowerings"],
      ]
        .map(
          ([k, l]) =>
            `<span class="stop ${this.view === k ? "on" : ""} " data-view="${k}">${l}</span>`,
        )
        .join("");
      const provBtn =
        this.view === "rides" && !this.err
          ? `<div class="grp"><span>origin edges</span><button class="prov-toggle" aria-pressed="${this.prov}">${this.prov ? "shown" : "hidden"}</button></div>`
          : "";
      const body = this.err
        ? `<p class="live-note"><b>not live:</b> ${this.err}. Nothing is shown baked; this chapter runs on the engine.</p>`
        : this.view === "rides"
          ? this.ridesView()
          : this.divergeView();
      this.innerHTML = `
      <div class="dialbar"><div class="grp"><span>show</span><div class="ladder">${tabs}</div></div>${provBtn}</div>
      ${body}`;
      this.querySelectorAll("[data-view]").forEach((s) =>
        s.addEventListener("click", () => {
          if (this.view !== s.dataset.view) {
            this.view = s.dataset.view;
            this.render();
          }
        }),
      );
      const pt = this.querySelector(".prov-toggle");
      if (pt)
        pt.addEventListener("click", () => {
          this.prov = !this.prov;
          this.render();
        });
      this.wireRead();
    }

    ridesView() {
      const d = this.b1,
        spec = PROV_B1;
      const by = new Map(spec.nodes.map((n) => [n.id, n]));
      const child = spec.children
        .map(([p, , , c]) => {
          const a = by.get(p),
            b = by.get(c);
          return `<line class="prov-child" x1="${a.x}" y1="${a.y + 16}" x2="${b.x}" y2="${b.y - 16}"/>`;
        })
        .join("");
      const origin = !this.prov
        ? ""
        : spec.origin
            .map(([s, , t]) => {
              const a = by.get(s),
                b = by.get(t);
              return `<line class="prov-origin" x1="${a.x - 44}" y1="${a.y}" x2="${b.x + 44}" y2="${b.y}" marker-end="url(#prov-arr)"/>`;
            })
            .join("");
      const nodes = spec.nodes
        .map(
          (n) =>
            `<g class="prov-node prov-${n.stage}" data-id="${n.id}" transform="translate(${n.x},${n.y})">` +
            `<rect x="-42" y="-16" width="84" height="32" rx="5"/>` +
            `<text class="prov-lab" y="-2">${n.lab}</text>` +
            `<text class="prov-kind" y="10">${n.kind}</text></g>`,
        )
        .join("");
      const w = d.with_origin,
        wo = d.without_origin;
      const chip = (hex) =>
        `<span class="chip prov-chip" style="--chip:${chipColor(hex)}">${hex.slice(0, 12)}</span>`;
      const cmp = `<div class="prov-cmp prov-cmp-one">
      <span class="prov-cmp-lab">structural address</span>${chip(this.prov ? w.structure : wo.structure)}
      <span class="prov-cmp-verdict ${d.structure_matches ? "ok" : "bad"}">${d.structure_matches ? "unchanged" : "changed unexpectedly"}</span>
    </div>`;
      const idle = `The dashed links show where the generated operations came from. Hide them and the structural address remains unchanged.`;
      const note = `Fold riffcat into a compiler's origin tracing and this is the payoff. The lowering above is a small stand-in for fe's origin facts: the source expression and its lowered form are the <b>structure</b>, and the <code>EdgeRole::Origin</code> edges are the <b>attribution</b>, each lowered node recording where it came from. The engine excludes origin edges from the structural fold, so attaching a full attribution graph leaves every facet address unchanged, structure and full alike, and the fingerprint the catalog dedups on stays put. Provenance is queryable payload that <b>rides along</b>; it does not move the shape. One honest label: folding an instrumentation <code>trace_events</code> payload in <em>would</em> move the address, a deliberate versioned cost, kept off here.`;
      return `
      <svg viewBox="0 0 720 268" class="prov-svg" role="img" aria-label="a source expression and its lowered MIR, linked by provenance edges">
        <defs><marker id="prov-arr" markerWidth="9" markerHeight="9" refX="7" refY="3" orient="auto"><path d="M0,0 L6,3 L0,6 z" class="latarrhead"/></marker></defs>
        <text x="150" y="28" text-anchor="middle" class="prov-col">source expression</text>
        <text x="556" y="28" text-anchor="middle" class="prov-col">lowered MIR</text>
        ${child}${origin}${nodes}
      </svg>
      ${cmp}
      <div class="eqread" data-idle="${idle}">${idle}</div>
      <p class="twnote">${note}</p>`;
    }

    divergeView() {
      const d = this.b2;
      const divA = new Set(d.a_only.map((n) => n.id));
      const divB = new Set(d.b_only.map((n) => n.id));
      const pct = Math.round(d.a_in_b * 100);
      const panel = (which, div) => {
        const rows = PROV_B2_ROWS.map((r) => {
          const marked = div.has(r.id) || div.has(r.op);
          const text = which === "a" ? r.a : r.b;
          return `<div class="prov-stmt ${marked ? "diverge" : ""}"><span class="prov-stmt-id">${r.id}</span><code>${text}</code>${marked ? `<span class="prov-flag">changed</span>` : ""}</div>`;
        }).join("");
        return `<div class="prov-panel"><div class="prov-panel-h">${which === "a" ? "before the pass" : "after the pass"}</div>${rows}</div>`;
      };
      const bar = `<div class="prov-bar-wrap"><span class="bar prov-bar" style="width:${pct}%"></span><span class="prov-bar-num">${pct}%</span></div>`;
      const site = PROV_B2_ROWS.find((r) => divA.has(r.op));
      const where = site
        ? `The divergence localizes to <code>${site.id}</code>'s operator, which provenance records came from <code>${site.src}</code> in the source. `
        : "";
      const flagged = d.a_only
        .map((n) => `<span class="prov-tag">${n.id} <em>${n.kind}</em></span>`)
        .join("");
      const idle = `Two lowerings of one source, differing by a single pass. The engine reports ${pct}% structural containment and flags exactly the nodes that differ.`;
      return `
      <div class="prov-panels">${panel("a", divA)}${panel("b", divB)}</div>
      <div class="prov-cont"><span class="prov-cont-lab">structural containment (engine)</span>${bar}</div>
      <div class="prov-flagged"><span class="prov-flagged-lab">nodes the engine flags as divergent</span>${flagged}</div>
      <div class="eqread" data-idle="${idle}">${idle}</div>
      <p class="twnote">${where}The other two, <code>s1</code> and <code>body</code>, differ because a changed node bubbles up its spine, the honest behavior of a bottom-up Merkle address: the smallest changed subtree is the site, its ancestors follow. riffcat re-finds <em>same shape except here</em>, and the provenance says where. Every number and every flagged node here is computed by the engine at render time over the two graphs the page hands it; nothing is baked.</p>
      <p class="twnote">The two graphs need not come from the same compiler run. Hand the engine the artifact maps from two feature branches of fe or sonatina (its backend IR), or from two different compilers of one contract, and the read is the same: the structural address is a function of shape alone, so who built the graph never enters the digest, only the shape they emitted does. Two toolchains that lower a function to the same shape land on the same address, and the catalog files them under it as the same shape, uncoordinated; where they part, this same containment reports the fraction still shared and flags exactly the nodes that moved. Honest bound: the two lowerings on this page are hand-built stand-ins, not compiler output, and comparing real cross-compiler maps is sound only at a level where both toolchains project into one graph vocabulary; the recognition is structural, same shape, not proved equivalence, which is why this is a facet and not a raw byte match.</p>`;
    }

    wireRead() {
      const read = this.querySelector(".eqread");
      if (!read) return;
      if (this.view === "rides") {
        const lab = new Map(PROV_B1.nodes.map((n) => [n.id, n.lab]));
        const origOf = new Map(PROV_B1.origin.map(([s, , t]) => [s, t]));
        this.querySelectorAll(".prov-node").forEach((g) =>
          g.addEventListener("mouseenter", () => {
            const id = g.dataset.id,
              tgt = origOf.get(id);
            read.innerHTML = tgt
              ? `<b>${lab.get(id)}</b> lowered from <b>${lab.get(tgt)}</b> in the source (an <code>EdgeRole::Origin</code> edge, excluded from the fold)`
              : `<b>${lab.get(id)}</b> is a source node; the lowered nodes point back at it`;
          }),
        );
      }
      this.addEventListener("mouseleave", () => {
        read.innerHTML = read.dataset.idle;
      });
    }
  },
);

const LOCLIM_VIEWS = [
  {
    key: "scc",
    label: "cycle / SCC (ships)",
    idle: "A recursive region has no leaf to begin the bottom-up fold from. The engine condenses each strongly connected component to one unit and addresses the component as a whole. Hover a piece.",
  },
  {
    key: "inst",
    label: "instantiation family (open edge)",
    idle: "One generic source, a family of concrete shapes once instantiated. The shape's identity depends on the instantiation context, which a context-free node digest excludes by design (I3). Hover a row.",
  },
];
const LOCLIM_STRUCT_COLOR = "var(--ink-dim)";
const LOCLIM_FAMILY = [
  {
    hov: "n4",
    name: "RPow<Pair, 4>",
    shape: "a depth-4 nest, finite and acyclic, nothing recursive remains",
    struct: "7f3a2c",
    types: "a14e90",
    typesColor: "var(--warm)",
  },
  {
    hov: "n8",
    name: "RPow<Pair, 8>",
    shape:
      "a depth-8 nest, a different concrete structure with different behaviour",
    struct: "7f3a2c",
    types: "c0d255",
    typesColor: "var(--cool)",
  },
];
const LOCLIM_HOVERS = {
  noleaf:
    "A, B, C form a cycle. The fold is post-order: a parent waits on its children's tree digests. Here every node waits on a child that waits back, so there is no base case and the acyclic fold raises CycleDetected rather than starting.",
  condense:
    "CondenseScc (Tarjan SCC plus a Weisfeiler-Leman refine) raises the unit of addressing from the node to the component. The whole SCC gets one ComponentHash; no context-free identity is finer than the SCC (invariant I4). This is the non-local workaround that ships.",
  wl: "Inside the component, members are distinguished by their WL color, not by node key order. Under AnonymousShape no key-derived bytes enter any digest, so the colors stand in for who-is-who within the cycle (invariant I5).",
  content:
    "A member's outgoing cross-component edge points at a shape it depends on, so it is folded into the member's initial color: it is content.",
  context:
    "A member's incoming cross-component edge says who points at this component. That is context, not content, and deliberately stays out of the digest (invariant I3). One comment in condensed.rs draws this exact content-versus-context line.",
  generic:
    "RPow<Pair, N> is one recursive thing in source. Hash this pre-instantiation form and every N shares one address, but which instantiation is lost. This is the cyclic, condense-scc side of the boundary.",
  n4: "After instantiation at N=4 the recursion is unrolled away: a finite acyclic nest with its own subtree digest. Concrete and addressable, but the family relationship to N=8 is gone.",
  n8: "N=8 is a different concrete shape with different behaviour. At structure-only it shares the family address with N=4; keeping the type spelling splits them. No single local digest carries both readings.",
  openedge:
    "Instantiation is a semantic resolution: the type arguments live at the call site, outside the subtree the fold can see. A local syntactic hash cannot hold it. The engine keeps a syntactic type spelling today, not a resolved instantiation, so this stays an open edge.",
};
const LOCLIM_SCC_NOTE = `<p class="twnote">A node's tree digest is a pure function of its own local content plus its children's tree digests (acyclic.rs). A cycle has no leaf, so there is no base case and the plain fold cannot start. <b>CondenseScc</b> handles this by raising the unit it addresses: each strongly connected component is condensed to one unit, members are refined by Weisfeiler-Leman color, and the component is hashed as a whole. The deliberate asymmetry is the point: a member's <b>outgoing</b> cross edge is content (it is folded into the color), while an <b>incoming</b> cross edge is <b>context</b> and stays out (invariant I3). When a single local digest cannot capture the whole, the method raises the unit it addresses rather than smuggling context into the bytes.</p>`;
const LOCLIM_INST_NOTE = `<p class="twnote">This is the same content-versus-context line, now on the open side. A generic <code>RPow&lt;Pair, N&gt;</code> is one recursive source; instantiating it gives a family of distinct finite acyclic shapes, one per N. A local digest is forced to pick: hash the generic form and every N collapses to one address (which instantiation is lost), or hash a monomorphized form and each N is its own address (the family relationship is gone, and the type arguments from the call site get baked into a digest advertised as a local subtree fold). The equality that matters here, <em>same generic, instantiated here</em> versus <em>same shape, different instantiation</em>, is a semantic fact about resolution, and resolution is not a property of any one subtree. So instantiation context is naturally <b>two facets</b>, never one contested address, and the resolved instantiation is not represented today.</p>`;
customElements.define(
  "locality-limit",
  class extends HTMLElement {
    connectedCallback() {
      this.view = "scc";
      this.render();
    }
    render() {
      const tabs = LOCLIM_VIEWS.map(
        (v) =>
          `<span class="stop ${v.key === this.view ? "on" : ""}" data-view="${v.key}">${v.label}</span>`,
      ).join("");
      const idle = LOCLIM_VIEWS.find((v) => v.key === this.view).idle;
      this.innerHTML = `
      <div class="dialbar"><div class="grp"><span>case</span><div class="ladder">${tabs}</div></div></div>
      <div class="loclim-stage">${this.view === "scc" ? this.sccSvg() : this.instSvg()}</div>
      <div class="eqread" data-idle="${idle}">${idle}</div>
      ${this.view === "scc" ? LOCLIM_SCC_NOTE : LOCLIM_INST_NOTE}
      <p class="twnote loclim-foot">Pre-baked illustration. The per-node <code>local</code> and <code>tree</code> digests and the per-component <code>ComponentHash</code> plus WL member colors are all computed by the engine today inside <code>digest_graph</code>, but the wasm wrapper surfaces only graph-level addresses, so the digests shown here are short stand-ins, not a live read of those exports. The SCC condensation, the WL refine, and the content-vs-context edge rule are real in the engine (<code>condensed.rs</code>); the instantiation case is an <b>open edge</b>: the engine keeps a syntactic type spelling, not a resolved instantiation, so the relationship between the family members is not addressed today.</p>`;
      this.querySelectorAll("[data-view]").forEach((s) =>
        s.addEventListener("click", () => {
          if (this.view !== s.dataset.view) {
            this.view = s.dataset.view;
            this.render();
          }
        }),
      );
      this.wire();
    }
    wire() {
      const read = this.querySelector(".eqread");
      if (!read) return;
      const clear = () =>
        this.querySelectorAll("[data-hov]").forEach((x) =>
          x.classList.remove("on"),
        );
      this.querySelectorAll("[data-hov]").forEach((g) =>
        g.addEventListener("mouseenter", () => {
          clear();
          g.classList.add("on");
          read.innerHTML = LOCLIM_HOVERS[g.dataset.hov] || read.dataset.idle;
        }),
      );
      this.addEventListener("mouseleave", () => {
        clear();
        read.innerHTML = read.dataset.idle;
      });
    }
    sccSvg() {
      return `
      <svg viewBox="0 0 720 312" class="lattice loclim-svg" role="img" aria-label="a strongly connected component condensed to one unit of addressing">
        <defs><marker id="loclim-arr" markerWidth="9" markerHeight="9" refX="6" refY="3" orient="auto"><path d="M0,0 L6,3 L0,6 z" class="latarrhead"/></marker></defs>
        <text x="172" y="26" text-anchor="middle" class="latcap">acyclic fold: no leaf to start from</text>
        <text x="548" y="26" text-anchor="middle" class="latcap sem">condense-scc: raise the unit to the component</text>
        <line x1="360" y1="40" x2="360" y2="296" class="latdivide"/>

        <g class="loclim-cyc" data-hov="noleaf">
          <path class="loclim-cedge" d="M 120 112 C 95 145, 95 159, 120 192" marker-end="url(#loclim-arr)"/>
          <path class="loclim-cedge" d="M 141 206 C 188 226, 235 188, 269 158" marker-end="url(#loclim-arr)"/>
          <path class="loclim-cedge" d="M 269 143 C 232 110, 185 104, 141 97" marker-end="url(#loclim-arr)"/>
          <g class="loclim-node" transform="translate(120,90)"><circle r="22"/><text y="5">A</text></g>
          <g class="loclim-node" transform="translate(120,214)"><circle r="22"/><text y="5">B</text></g>
          <g class="loclim-node" transform="translate(290,150)"><circle r="22"/><text y="5">C</text></g>
          <text x="172" y="284" text-anchor="middle" class="loclim-flow bad">every node waits on a child digest that waits on it</text>
        </g>

        <g class="loclim-comp" data-hov="condense">
          <rect class="loclim-scc" x="446" y="66" width="204" height="118" rx="12"/>
          <text x="548" y="58" text-anchor="middle" class="loclim-scclab">one SCC = one address</text>
          <g class="loclim-icyc">
            <path class="loclim-iedge" d="M 486 113 C 474 121, 474 129, 486 137" marker-end="url(#loclim-arr)"/>
            <path class="loclim-iedge" d="M 501 149 C 536 156, 562 140, 591 128" marker-end="url(#loclim-arr)"/>
            <path class="loclim-iedge" d="M 591 122 C 562 110, 536 95, 501 101" marker-end="url(#loclim-arr)"/>
          </g>
          <g class="loclim-mem" data-hov="wl" transform="translate(486,98)"><circle r="15" style="fill:var(--cool)"/><text y="4">A</text></g>
          <g class="loclim-mem" data-hov="wl" transform="translate(486,152)"><circle r="15" style="fill:var(--warm)"/><text y="4">B</text></g>
          <g class="loclim-mem" data-hov="wl" transform="translate(606,125)"><circle r="15" style="fill:var(--a)"/><text y="4">C</text></g>
          <text x="548" y="176" text-anchor="middle" class="loclim-wllab">members keyed by WL color, not by node order</text>
        </g>
        <g class="loclim-out" data-hov="content">
          <path class="loclim-cedge content" d="M 606 140 C 678 156, 678 218, 606 232" marker-end="url(#loclim-arr)"/>
          <g class="loclim-ext" transform="translate(606,248)"><circle r="16"/><text y="4">D</text></g>
          <text x="548" y="232" text-anchor="middle" class="loclim-edgelab content">outgoing cross edge: folded in as content</text>
        </g>
        <g class="loclim-in" data-hov="context">
          <path class="loclim-cedge context" d="M 430 256 C 470 244, 470 200, 448 178"/>
          <g class="loclim-ext ctx" transform="translate(414,262)"><circle r="16"/><text y="4">E</text></g>
          <text x="500" y="296" text-anchor="middle" class="loclim-edgelab context">incoming cross edge: context, kept out (I3)</text>
        </g>
      </svg>`;
    }
    instSvg() {
      const row = (m) =>
        `<div class="loclim-inst-row" data-hov="${m.hov}">` +
        `<span class="loclim-inst-name">${m.name}</span>` +
        `<span class="loclim-inst-shape">${m.shape}</span>` +
        `<span class="loclim-inst-pair">` +
        `<span class="loclim-addr struct" title="structure-only address"><span class="shapedot" style="--chip:${LOCLIM_STRUCT_COLOR}"></span>${m.struct}</span>` +
        `<span class="loclim-addr types" title="keeps the type spelling"><span class="shapedot" style="--chip:${m.typesColor}"></span>${m.types}</span>` +
        `</span></div>`;
      return `
      <div class="loclim-inst">
        <div class="loclim-inst-head" data-hov="generic"><span class="loclim-inst-gen">RPow&lt;Pair, N&gt;</span><span class="vsub">one generic source: a perfect binary tree of depth N, recursive before instantiation</span></div>
        <div class="loclim-inst-cols"><span class="loclim-col-l">name &amp; concrete shape</span><span class="loclim-col-r">structure-only&nbsp;&nbsp;·&nbsp;&nbsp;keeps types</span></div>
        ${LOCLIM_FAMILY.map(row).join("")}
        <div class="loclim-inst-edge" data-hov="openedge"><span class="loclim-edge-mark">open</span> the address that says <em>these are one generic</em> and the address that says <em>which instantiation</em> are not the same address. A context-free local digest cannot hold both, and the resolved instantiation is not represented today.</div>
      </div>`;
    }
  },
);
