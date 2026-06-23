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
      addEventListener("TrunkApplicationStarted", () => resolve(window.wasmBindings), { once: true }));

const badge = document.getElementById("engine");
engineReady.then(() => { badge.textContent = "wasm live ✓"; })
  .catch(() => { badge.textContent = "wasm failed"; badge.classList.add("bad"); });

const short = (hex) => (hex || "").slice(0, 8);

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
  for (let i = 0; i < 10 && i < digest.length; i++) h = (h * 131 + digest.charCodeAt(i)) >>> 0;
  return h % 360;
}

// Compact, human label for a yul function name (the name is shown for people,
// never folded into the fingerprint).
function chipLabel(name) {
  let s = name.replace(/^fun_/, "").replace(/^constructor_\w*?_(\d+)$/, "constructor")
    .replace(/^constructor_/, "constructor·").replace(/_\d+$/, "");
  return s.length > 18 ? s.slice(0, 17) + "…" : s;
}

// The five dimensions a fingerprint decomposes into, shown shortest-name first.
const DIMS = [["structure", "struct"], ["names", "names"], ["constants", "const"], ["types", "types"], ["trace_events", "trace"]];

// Render one function as a chip carrying its class + color + the per-dimension
// digests (so a hover can show which dimensions a twin-network shares vs differs
// on: the "faceted preimage signature" that explains why two differently-named
// functions are the same shape).
function chip(u, facet, lib) {
  const d = u.facets[facet];
  const dims = DIMS.map(([k]) => short(u.digests[k]).slice(0, 6)).join(",");
  return `<span class="chip ${eqKey(d)}" style="--hue:${digestHue(d)}" data-eq="${eqKey(d)}"`
    + ` data-name="${u.name}" data-fp="${short(d)}" data-dims="${dims}"${lib ? ` data-lib="${lib}"` : ""}`
    + ` title="${u.name}">${chipLabel(u.name)}</span>`;
}

// Build the dimension breakdown for a hovered chip vs its lit network: a strip
// of cells (green = identical across the whole network, rose = varies) plus a
// plain-language note. This is what makes "they differ only in their names"
// legible: the names cell goes rose, every other cell stays green.
function dimStrip(c, lit) {
  const mine = c.dataset.dims.split(",");
  const arrs = [...lit].map((x) => x.dataset.dims.split(","));
  const same = (i) => arrs.every((a) => a[i] === mine[i]);
  const cells = DIMS.map(([, lab], i) =>
    `<span class="dimcell ${same(i) ? "same" : "diff"}">${lab} ${mine[i]}</span>`).join("");
  const diff = DIMS.filter((_, i) => !same(i)).map(([, lab]) => lab);
  const note = lit.length === 1 ? "unique here at this facet"
    : diff.length === 0 ? `all <b>${lit.length}</b> identical in every dimension`
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
  host.addEventListener("mouseover", (e) => {
    const c = e.target.closest(".chip");
    if (!c || !host.contains(c)) return;
    const lit = host.querySelectorAll("." + c.dataset.eq);
    host.querySelectorAll(".eqgrid").forEach((g) => g.classList.add("focused"));
    lit.forEach((x) => x.classList.add("lit"));
    const read = host.querySelector(".eqread");
    if (read) read.innerHTML = describe(c, lit);
  });
  host.addEventListener("mouseout", (e) => {
    if (!e.target.closest(".chip")) return;
    host.querySelectorAll(".chip.lit").forEach((x) => x.classList.remove("lit"));
    host.querySelectorAll(".eqgrid").forEach((g) => g.classList.remove("focused"));
    const read = host.querySelector(".eqread");
    if (read) read.innerHTML = read.dataset.idle || "";
  });
}

const CH = [
  {
    nav: "start",
    kicker: "the one idea",
    title: "“The same” has a dial.",
    lede: "Compile a contract and most of what comes out is not unique. The question is how strict you want “the same” to be.",
    body: `
      <p>Same in every detail? Same if you ignore the names? Ignore the embedded constants and the types too?
      Each setting is a <em>facet</em>, and two pieces of code can <b>rhyme</b> at a facet even when they read
      differently. Everything else exists to keep the dial honest.</p>
      <p>Two artifacts that land on the same fingerprint at a facet are <em>twins</em> there. Drag the dial looser
      and twins multiply; tighten it and they split. The next chapter lets you turn that dial yourself, on a real
      contract, computed in your browser.</p>`,
  },
  {
    nav: "drive the dial",
    kicker: "live, in your browser",
    title: "Same color, same shape.",
    lede: "Every function a small token compiled to, one chip each, colored by its fingerprint. Loosen the dial and watch the colors merge. Hover a chip to light its twins.",
    body: `<live-dial></live-dial>`,
  },
  {
    nav: "dedup",
    kicker: "the headline",
    title: "Your compiler repeats itself, and now you can see it.",
    lede: "Nine ordinary contracts, every function the compiler emitted, bucketed by fingerprint.",
    body: `
      <div class="figure"><div class="cap">yul-fn classes, by facet (9-contract corpus)</div>
        <table>
          <tr><th>facet</th><th>distinct classes</th><th>dedup</th></tr>
          <tr><td>all (every detail)</td><td class="n">7,939 → 1,713</td><td>78.4%</td></tr>
          <tr><td>structure only (shape)</td><td class="n">→ 481</td><td>93.9%</td></tr>
        </table></div>
      <p>At the shape facet, <b>94%</b> of the functions are structural duplicates of each other. The compiler has
      been repeating itself this whole time with no way to see it. The generated helpers dedup perfectly: that
      consistency is an exploitable asset, not a flaw. The chapter you just drove is this, on one contract.</p>`,
  },
  {
    nav: "twins",
    kicker: "twins by shape",
    title: "Seaport rhymes with a 200-line ERC20.",
    lede: "Names-blind, the most-audited hand-written-assembly contract on Ethereum shares fingerprint classes with a toy token.",
    body: `
      <div class="say">“That is Seaport, fetched verified from Sourcify and recompiled with its own pinned 0.8.24.
      Names-blind it shares <b>37</b> function-level fingerprint classes with the little ERC20 we built locally:
      the same panic helpers, the same checked arithmetic, the same memory allocator, the same
      address→uint256 mapping accessor. Twins by shape, not by name.”</div>
      <p>Lead with the 37 shared classes, not the ratio: a 591-class contract against a 68-class one. The rhyme is
      the recognizable scaffolding every contract carries.</p>`,
  },
  {
    nav: "triage",
    kicker: "what hashing buys an auditor",
    title: "Verified does not tell you what is in it.",
    lede: "Every function checked for twins elsewhere. Twins = machinery. No twins = the novel surface where reading time goes.",
    body: `
      <div class="figure"><div class="cap">novel functions vs total (mainnet, measured)</div>
        <table>
          <tr><th>contract</th><th>novel / total</th><th>read</th></tr>
          <tr><td>a token verified the same morning</td><td class="n">23 / 169</td><td>86% machinery</td></tr>
          <tr><td>ERC-4337 EntryPoint</td><td class="n">206 / 601</td><td>genuinely novel</td></tr>
        </table></div>
      <p>“This unverified contract is 94% standard machinery. Here are the three functions that are actually new.
      Audit those.” The query agrees with auditor intuition in both directions, and it speaks straight to the
      public worry that <em>verified is not safe</em>.</p>`,
  },
  {
    nav: "library",
    kicker: "provenance",
    title: "Shared machinery, lit across libraries.",
    lede: "Three contracts, each vendoring one library's mul·div, fingerprinted live. Hover a chip: the machinery they share lights up across all three rows, but each library's mul·div stands alone.",
    body: `<live-xref></live-xref>`,
  },
  {
    nav: "in the wild",
    kicker: "the same, on real code, version-proof",
    title: "Four unrelated contracts, one OpenZeppelin surface.",
    lede: "An ERC20, an NFT, a soulbound voucher, and Vouch, fetched from Sourcify, compared at the source level.",
    body: `
      <div class="figure"><div class="cap">shared OZ chunks (sol-fn, names-blind), of 4 unrelated contracts</div>
        <table>
          <tr><td><code>Context._msgSender</code>, <code>Initializable</code></td><td class="n">4 / 4</td></tr>
          <tr><td><code>ReentrancyGuard</code>, <code>Strings</code>, <code>Ownable</code>, <code>Math</code></td><td class="n">3 / 4</td></tr>
          <tr><td><code>ERC721</code> core, <code>ERC20</code> surface</td><td class="n">2 / 4</td></tr>
        </table></div>
      <p>This is at the <em>source</em> level (<code>sol-fn</code>), which does not drift with the compiler version
      the way Yul does. So it is the right substrate for “same library code,” and the shared chunks are recognizable
      by name. Compiler version is recorded as provenance, never folded into the shape.</p>`,
  },
  {
    nav: "sourcify",
    kicker: "you already run this dial",
    title: "Sourcify's partial-vs-exact match is a facet.",
    lede: "At the bytecode level the CBOR metadata trailer is split into its own node.",
    body: `
      <p>The <b>Structure</b> facet forgets the metadata value: that is a Sourcify <em>partial match</em>, two
      contracts that are Structure-twins here. A Constants-bearing facet keeps it: that is an <em>exact match</em>.
      Full-vs-partial match, and the Verifier Alliance transformations schema (cborAuxdata, library masks), are
      facet normalization you already ship. Here is the general dial.</p>
      <div class="say">“You invented this twice. The question riffcat answers is what the payload of a
      ‘similar contracts’ surface should carry.”</div>`,
  },
  {
    nav: "trust",
    kicker: "sameness that is not structural",
    title: "Claims are inputs, and the corpus has a receipt.",
    lede: "Witnessed equivalence assertions, attestations that gate queries, and a commitment over the whole corpus.",
    body: `
      <p>A <b>claim</b> is an attributable, removable assertion that two things are equivalent: validity is the
      auditor's job, and a wrong claim merges things <em>visibly</em>, never silently. An <b>attestation</b> is a
      one-sided property (“verified-total”) that can gate a query. And the <b>corpus root</b> is a canonical,
      order-independent fingerprint of the whole corpus: recompute it anywhere, <code>--check</code> fails loudly.</p>
      <pre class="cmd"><span class="c"># the integrity receipt; conditional claims pin to it</span>
riffcat --corpus demo/corpus root --unit yul-fn --mode shape</pre>`,
  },
  {
    nav: "coda",
    kicker: "what this is not",
    title: "One shared contract, not a takeover.",
    lede: "",
    body: `
      <p>NOT an omnilingual compiler. NOT a shared internal IR. NOT a database (JSONL today, rent a store later).
      NOT a standards proposal. One shared thing: the canonical form plus the facet vocabulary, versioned.</p>
      <p>EVM is the first corpus, not the spouse: the architecture is level-tagged and language-agnostic by
      construction. fe-emitted Yul ingests through the same two doors. Compilers meet at the artifact level.</p>
      <div class="say">It rides the recompilation Sourcify already does at verification time. No crawler, no new
      compiler. The fingerprint is a post-pass over output you already produce and discard.</div>`,
  },
];

customElements.define("tour-app", class extends HTMLElement {
  connectedCallback() {
    this.i = 0;
    const nav = document.getElementById("nav");
    nav.innerHTML = CH.map((c, k) => `<button data-k="${k}">${k === 0 ? "·" : k}. ${c.nav}</button>`).join("");
    nav.querySelectorAll("button").forEach((b) =>
      b.addEventListener("click", () => this.go(+b.dataset.k)));
    document.addEventListener("keydown", (e) => {
      if (e.target.closest("live-dial")) return; // let the dial keep focus
      if (e.key === "ArrowRight") this.go(this.i + 1);
      if (e.key === "ArrowLeft") this.go(this.i - 1);
    });
    this.render();
  }
  go(k) { if (k >= 0 && k < CH.length) { this.i = k; this.render(); } }
  render() {
    const c = CH[this.i];
    document.querySelectorAll("#nav button").forEach((b, k) =>
      b.setAttribute("aria-current", k === this.i ? "true" : "false"));
    this.innerHTML = `
      <div class="stage">
        <div class="kicker">${c.kicker}</div>
        <h1>${c.title}</h1>
        ${c.lede ? `<p class="lede">${c.lede}</p>` : ""}
        ${c.body}
      </div>
      <div class="pager">
        <button data-d="-1" ${this.i === 0 ? "disabled" : ""}>← prev</button>
        <span class="count">${this.i + 1} / ${CH.length} · ${c.nav}</span>
        <button data-d="1" ${this.i === CH.length - 1 ? "disabled" : ""}>next →</button>
      </div>`;
    this.querySelectorAll(".pager button").forEach((b) =>
      b.addEventListener("click", () => this.go(this.i + (+b.dataset.d))));
    window.scrollTo({ top: 0, behavior: "smooth" });
  }
});

// Drive the dial: fingerprint the baked Demo contract in-browser, render every
// function as a chip colored by its fingerprint, and let the reader turn the
// dial. The facet ladder doubles as the selector and shows the class count at
// each stop, so the visual collapse and the number move together.
const FACETS = ["full", "names-blind", "structure"];

customElements.define("live-dial", class extends HTMLElement {
  async connectedCallback() {
    this.mode = "shape";
    this.facet = "names-blind";
    this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
    try {
      this.bindings = await engineReady;
      this.fixture = this.bindings.fixture("demo");
      if (!this.fixture) throw new Error("demo fixture missing");
      this.render();
    } catch (e) {
      this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`;
    }
  }
  unitsFor(mode) {
    this._cache = this._cache || {};
    if (!this._cache[mode]) {
      const t0 = performance.now();
      this._cache[mode] = JSON.parse(this.bindings.fingerprint_yul(this.fixture, mode));
      this._ms = performance.now() - t0;
    }
    return this._cache[mode];
  }
  render() {
    const funcs = this.unitsFor(this.mode).slice(1);
    const count = (f) => new Set(funcs.map((u) => u.facets[f])).size;
    const ladder = FACETS.map((f) =>
      `<span class="stop ${f === this.facet ? "on" : ""}" data-facet="${f}"><b>${count(f)}</b> ${f}</span>`).join("");
    const grid = funcs.map((u) => chip(u, this.facet)).join("");
    const k = count(this.facet);
    const idle = `${funcs.length} functions · <b>${k}</b> classes at ${this.facet} · ${this.mode} ·`
      + ` ${this.mode === "shape" ? "loosen the dial and the colors merge" : "identity pins every artifact, so nothing merges"}`
      + ` · ${this._ms.toFixed(0)} ms in your browser`;
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
      b.addEventListener("click", () => { if (this.mode !== b.dataset.mode) { this.mode = b.dataset.mode; this.render(); } }));
    this.querySelectorAll("[data-facet]").forEach((b) =>
      b.addEventListener("click", () => { if (this.facet !== b.dataset.facet) { this.facet = b.dataset.facet; this.render(); } }));
    wireHighlight(this, (c, lit) =>
      `<span class="sw" style="background:hsl(${digestHue(c.dataset.eq.slice(3))} 70% 55%)"></span>`
      + `${c.dataset.name} · <span style="color:var(--warm)">${c.dataset.fp}</span>`
      + dimStrip(c, lit));
  }
});

// Library cross-reference: one wrapper per library, every emitted function as a
// chip at names-blind. Because color and class come from the fingerprint, the
// machinery the three libraries share lands on the same color in every row, and
// hovering it lights all three; each library's mul·div is its own island.
const XREF = [
  { id: "oz-muldiv", lib: "OpenZeppelin", short: "OZ" },
  { id: "solady-muldiv", lib: "Solady", short: "Solady" },
  { id: "solmate-muldiv", lib: "Solmate", short: "Solmate" },
];

customElements.define("live-xref", class extends HTMLElement {
  async connectedCallback() {
    this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
    try {
      const b = await engineReady;
      const F = "names-blind";
      const t0 = performance.now();
      const rows = XREF.map((x) => ({ x, funcs: JSON.parse(b.fingerprint_yul(b.fixture(x.id), "shape")).slice(1) }));
      const ms = (performance.now() - t0).toFixed(0);
      // how many distinct chunks are shared across all three libraries
      const spread = new Map();
      for (const { x, funcs } of rows)
        for (const u of funcs) {
          const d = u.facets[F];
          if (!spread.has(d)) spread.set(d, new Set());
          spread.get(d).add(x.short);
        }
      const all3 = [...spread.values()].filter((s) => s.size === 3).length;
      const gridRows = rows.map(({ x, funcs }) =>
        `<div class="eqrow"><div class="libname"><b>${x.short}</b>${funcs.length} fns</div>`
        + `<div class="eqgrid" data-lib="${x.short}">${funcs.map((u) => chip(u, F, x.short)).join("")}</div></div>`).join("");
      const idle = `${spread.size} distinct chunks · <b>${all3}</b> shared across all three libraries`
        + ` · hover one to trace it · ${ms} ms in your browser`;
      this.innerHTML = `${gridRows}<div class="eqread" data-idle="${idle}">${idle}</div>`;
      wireHighlight(this, (c, lit) => {
        const libs = new Set([...lit].map((x) => x.dataset.lib));
        const where = libs.size === 3 ? "shared across <b>all three</b> libraries"
          : libs.size === 2 ? `shared across <b>two</b> (${[...libs].join(", ")})`
          : `<b>only</b> in ${[...libs][0]}`;
        return `<span class="sw" style="background:hsl(${digestHue(c.dataset.eq.slice(3))} 70% 55%)"></span>`
          + `${c.dataset.name} · <span style="color:var(--warm)">${c.dataset.fp}</span> · ${where}`
          + dimStrip(c, lit);
      });
    } catch (e) {
      this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`;
    }
  }
});
