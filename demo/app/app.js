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
    title: "Turn the dial on a real contract.",
    lede: "Every function a small token compiled to. Fingerprinted right now, client-side, at three facets. Watch the classes collapse as you loosen the dial.",
    body: `
      <live-dial></live-dial>
      <p>No backend answered this. The contract's Yul went into riffcat-compiled-to-wasm and came back as facet
      fingerprints, in the time shown. Loosen from <em>full</em> to <em>names-blind</em> to <em>structure</em> and
      identical-shaped functions fall into the same class: that collapse is the dial doing its job.</p>`,
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
    title: "Which library did you actually vendor?",
    lede: "The same primitive in OpenZeppelin, Solady, and Solmate is a different fingerprint. Computed live, right here, from each library's own source.",
    body: `
      <live-xref></live-xref>
      <p>Within a library the chunk is one fingerprint across every contract that vendors it: the three digests
      above each held identical across three unrelated wrapper contracts (a vault, an airdrop, a lottery). Across
      libraries the same math has a different shape. The supply chain is in the fingerprint. The fuller sweep,
      five primitives across the three libraries, lands <b>zero collisions</b> the same way.</p>`,
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

// The live chapter: fingerprint the baked Demo contract in-browser and let the
// reader turn the dial (shape vs identity; full / names-blind / structure).
const FACETS = ["full", "names-blind", "structure"];
const FACET_LABEL = { full: "full (every detail)", "names-blind": "names-blind", structure: "structure only" };

customElements.define("live-dial", class extends HTMLElement {
  async connectedCallback() {
    this.mode = "shape";
    this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
    try {
      const b = await engineReady;
      const raw = b.fixture("demo");
      if (!raw) throw new Error("demo fixture missing");
      this.fixture = raw;
      this.bindings = b;
      this.render();
    } catch (e) {
      this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`;
    }
  }
  compute() {
    const t0 = performance.now();
    const units = JSON.parse(this.bindings.fingerprint_yul(this.fixture, this.mode));
    const ms = (performance.now() - t0);
    const object = units[0];
    const funcs = units.slice(1);
    const classesAt = (facet) => new Set(funcs.map((u) => u.facets[facet])).size;
    // largest twin group at the structure facet (shape-identical functions)
    const byStruct = new Map();
    for (const u of funcs) {
      const k = u.facets.structure;
      if (!byStruct.has(k)) byStruct.set(k, []);
      byStruct.get(k).push(u.name);
    }
    const biggest = [...byStruct.values()].sort((a, b) => b.length - a.length)[0] || [];
    return { ms, object, n: funcs.length, classesAt, biggest };
  }
  render() {
    const r = this.compute();
    const n = r.n;
    const rows = FACETS.map((f) => {
      const k = r.classesAt(f);
      const pct = n ? Math.round((1 - k / n) * 100) : 0;
      const w = Math.max(2, Math.round((k / Math.max(1, n)) * 220));
      return `<tr><td>${FACET_LABEL[f]}</td>
        <td class="n">${n} → ${k}</td>
        <td><span class="bar" style="width:${w}px"></span> ${pct}% twins</td></tr>`;
    }).join("");
    const members = r.biggest.slice(0, 6).map((s) => `<code>${s}</code>`).join(" ");
    const more = r.biggest.length > 6 ? ` +${r.biggest.length - 6} more` : "";
    const hint = this.mode === "shape"
      ? "shape: paths never enter the digest, so same-shaped code collapses as you loosen the dial."
      : "identity: each artifact is path-bound, so nothing collapses. That is the contrast that shows what shape buys you.";
    this.innerHTML = `
      <div class="dial">
        <div class="grp"><span>mode</span>
          <button data-mode="shape" aria-pressed="${this.mode === "shape"}">shape</button>
          <button data-mode="identity" aria-pressed="${this.mode === "identity"}">identity</button>
        </div>
      </div>
      <div class="figure"><div class="cap">demo · ${n} yul functions · fingerprinted live (${this.mode})</div>
        <table>
          <tr><th>facet</th><th>distinct classes</th><th>collapse</th></tr>
          ${rows}
        </table></div>
      <p class="live-note">${hint}</p>
      <p class="live-note">object fingerprint at structure: <b>${short(r.object.facets.structure)}</b> ·
        largest shape-identical group (${r.biggest.length}): ${members}${more}</p>
      <p class="live-note">computed in your browser in <b>${r.ms.toFixed(1)} ms</b>: riffcat, as wasm, no server.</p>`;
    this.querySelectorAll("[data-mode]").forEach((btn) =>
      btn.addEventListener("click", () => {
        if (this.mode === btn.dataset.mode) return;
        this.mode = btn.dataset.mode;
        this.render();
      }));
  }
});

// The library cross-reference, computed live: fingerprint one wrapper contract
// per library, locate that library's full-precision mul·div function, and show
// its names-blind shape. Same math, three libraries, three fingerprints.
const XREF = [
  { id: "oz-muldiv", lib: "OpenZeppelin", call: "Math.mulDiv", token: "mulDiv" },
  { id: "solady-muldiv", lib: "Solady", call: "FixedPointMathLib.fullMulDiv", token: "fullMulDiv" },
  { id: "solmate-muldiv", lib: "Solmate", call: "FixedPointMathLib.mulDivDown", token: "mulDivDown" },
];

customElements.define("live-xref", class extends HTMLElement {
  async connectedCallback() {
    this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
    try {
      const b = await engineReady;
      const t0 = performance.now();
      const found = XREF.map((x) => {
        const units = JSON.parse(b.fingerprint_yul(b.fixture(x.id), "shape")).slice(1);
        const fn = units.find((u) => u.name.startsWith("fun_") && u.name.includes(x.token));
        return { ...x, nb: fn ? fn.facets["names-blind"] : null, st: fn ? fn.facets.structure : null };
      });
      const ms = (performance.now() - t0).toFixed(1);
      const distinct = new Set(found.map((f) => f.nb)).size;
      const rows = found.map((f) =>
        `<tr><td>${f.lib} <code>${f.call}</code></td><td class="fp">${short(f.nb)}</td></tr>`).join("");
      this.innerHTML = `
        <div class="figure"><div class="cap">full-precision mul·div, names-blind · fingerprinted live</div>
          <table>${rows}</table></div>
        <p class="live-note"><b>${distinct} / ${found.length}</b> distinct
          ${distinct === found.length ? ", zero collisions" : ""} ·
          computed in your browser in <b>${ms} ms</b> from each library's own source.</p>`;
    } catch (e) {
      this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`;
    }
  }
});
