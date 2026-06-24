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
  const jobs = [["demo", "shape"], ["demo", "identity"],
    ["oz-muldiv", "shape"], ["solady-muldiv", "shape"], ["solmate-muldiv", "shape"]];
  const ric = window.requestIdleCallback || ((f) => setTimeout(f, 16));
  engineReady.then((b) => {
    const step = () => {
      const j = jobs.shift();
      if (!j) return;
      try { yulUnits(b, j[0], j[1]); } catch { /* leave it for first visit */ }
      ric(step);
    };
    ric(step);
  }).catch(() => {});
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
  for (let i = 0; i < 10 && i < digest.length; i++) h = (h * 131 + digest.charCodeAt(i)) >>> 0;
  return h % 360;
}

// Chip color: a perceptually-uniform OKLCH color for a shape digest. Prefer the
// Rust harmonizer (palette, gamut-safe, golden-angle hue) the engine exposes;
// fall back to a JS OKLCH with the same golden-angle hue if a chip happens to
// render before the engine is live. The whole chip family (dark fill, bright
// accent, light text) is mixed from this one accent in CSS, in oklch.
function chipColor(digest) {
  const b = window.wasmBindings;
  if (b && b.chip_color) { try { return b.chip_color(digest); } catch (_) { /* fall through */ } }
  let acc = 0;
  for (let i = 0; i < 12 && i < digest.length; i++) acc = (acc * 131 + digest.charCodeAt(i)) >>> 0;
  return `oklch(72% 0.13 ${((acc * 137.508) % 360).toFixed(1)})`;
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
  return `<span class="chip ${eqKey(d)}" style="--chip:${chipColor(d)}" data-eq="${eqKey(d)}"`
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
  // mouseover lights the hovered chip's network (clearing the previous one
  // first, so moving chip-to-chip repaints cleanly). mouseleave clears, and
  // unlike mouseout it does not bubble and fires exactly once when the pointer
  // leaves the whole component, so the highlight can never get stuck.
  host.addEventListener("mouseover", (e) => {
    const c = e.target.closest(".chip");
    if (!c || !host.contains(c)) return;
    host.querySelectorAll(".chip.lit").forEach((x) => x.classList.remove("lit"));
    const lit = host.querySelectorAll("." + c.dataset.eq);
    host.querySelectorAll(".eqgrid").forEach((g) => g.classList.add("focused"));
    lit.forEach((x) => x.classList.add("lit"));
    const read = host.querySelector(".eqread");
    if (read) read.innerHTML = describe(c, lit);
  });
  host.addEventListener("mouseleave", () => {
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
    body: `<p>Same in every detail, or same once you ignore the names, the constants, the types? Each setting is a <em>facet</em>. You turn the dial yourself next.</p>`,
  },
  {
    nav: "facets",
    kicker: "the dial, on five functions",
    title: "What a facet actually does.",
    lede: "Five tiny functions, fingerprinted live in your browser. Slide the facet and watch which collapse to one shape, and which never do.",
    body: `<facet-primer></facet-primer>`,
  },
  {
    nav: "the riff",
    kicker: "the same dial, on music",
    title: "Why “riff”? Transpose it and it is still the riff.",
    lede: "A short motif and a few variants, fingerprinted live by the same engine. Turn the dial and see which count as the same: transpose-proof intervals, rhythm alone, or the set of notes. Press play to hear each one.",
    body: `<riff-dial></riff-dial><div class="kicker" style="margin-top:24px">now on chords, parsed from real notation</div><chord-fp></chord-fp>`,
  },
  {
    nav: "recognized",
    kicker: "the pitch, on real mainnet code",
    title: "We know this code: it is OpenZeppelin.",
    lede: "Real verified contracts off Sourcify, each function fingerprinted at the source level against a catalog of pinned OpenZeppelin, Solady, and Solmate. Colored by the library it matches; grey is novel app code. Hover for the source.",
    body: `<recog-scan></recog-scan>`,
  },
  {
    nav: "twins",
    kicker: "the same shape, across real contracts",
    title: "One function, contract after contract.",
    lede: "Library functions that turn up, the exact same shape, across these real contracts. Pick one to see it and everywhere it lands.",
    body: `<recog-twins></recog-twins>`,
  },
  {
    nav: "dedup",
    kicker: "how much is actually new",
    title: "Most of what is deployed is not new.",
    lede: "Across ten real mainnet contracts: how much is already a library shape, and how much is the novel surface an auditor actually has to read.",
    body: `<recog-dedup></recog-dedup>`,
  },
  {
    nav: "the compiler too",
    kicker: "one level down, post-compilation",
    title: "Below the source, the compiler repeats itself even harder.",
    lede: "The same dial, now on the Yul one contract compiles to: machinery you never wrote. Loosen it and the colors merge; hover a chip to light its twins.",
    body: `<live-dial></live-dial>`,
  },
  {
    nav: "sniff it out",
    kicker: "find a known bug by shape",
    title: "We know this bug. Find it everywhere.",
    lede: "A known ERC-4626 inflation shape, matched across Sourcify's verified vaults. The patch is a different shape, so you see who fixed it, and the shape reaches forks an exact-source match misses.",
    body: `<vuln-sniff></vuln-sniff>`,
  },
  {
    nav: "modified",
    kicker: "fuzzy: the forks that edited it",
    title: "And the ones that changed the code.",
    lede: "Exact match catches verbatim copies. Real forks edit the function: a dropped modifier, a hand-rolled forwarder. Sourcify, a text search, and exact match all miss them. Matching the shape's subtrees does not.",
    body: `<fuzzy-scan></fuzzy-scan>`,
  },
  {
    nav: "structure vs meaning",
    kicker: "framing the verifier",
    title: "Syntactic to semantic is a lattice, not a line.",
    lede: "Where structure ends and meaning begins, and why riffcat hands that boundary to a verifier.",
    body: `<facet-lattice></facet-lattice>`,
  },
  { nav: "prove it", kicker: "the seat we leave open", title: "riffcat localizes. The verdict seat stays empty.", lede: "A shape match is a candidate, not a verdict. riffcat localizes the matching subtree and writes a proof obligation, then leaves the verdict and attested-by columns empty, a seat that only a named verifier can fill. Pick a tool to see what it would have to discharge.", body: `<verdict-seat></verdict-seat>` },
  {
    nav: "anchors",
    kicker: "the heart of the framing: a fact rides an address",
    title: "An address is an anchor. A fact rides it, but only as far as it forgets.",
    lede: "Attach a fact to a facet address and it transports, for free, to every shape that shares the address. The catch is a soundness condition, and you can watch it fail: a fact rides an anchor only when the anchor forgets at least as much as the fact does. Pin a fact below, then slide the anchor coarser and see exactly where it would wrongly travel.",
    body: `<anchor-transport></anchor-transport>`,
  },
  { nav: "prior art", kicker: "the primitive is older than us", title: "Content-addressed structure keeps getting reinvented.", lede: "Three communities arrived at the same move on their own: normalize a structure, then address it by its content. Music theory in 1973, a content-addressed Lean kernel, a verified Lean compiler for the EVM. The hash is not what is new here. The faceted, explainable query is.", body: `<prior-art></prior-art>` },
  {
    nav: "the proofs check",
    kicker: "one rung up: the map itself is proved",
    title: "The normal-form proofs check.",
    lede: "Riffcat lands a chord on its prime form. A Lean 4 development proves that prime form really is the canonical representative of the transpose-and-invert class, and the kernel checks it. We content-address that proof and register it as a claim, anchored on the one facet the proof stays sound at. Hover the parts of the badge to read each one honestly.",
    body: `<verified-attest></verified-attest>`,
  },
  {
    nav: "forte catalog",
    kicker: "the dial lands on a published catalog",
    title: "The same dial finds Forte's catalog on its own.",
    lede: "Seven named chords, each parsed from notation and fingerprinted live in your browser. Turn the dial to the set-class facet and watch major and minor fall into one class while augmented and diminished stay apart. The class riffcat computes is Allen Forte's, arrived at from structure alone.",
    body: `<forte-catalog></forte-catalog>`,
  },
  { nav: "interval vector", kicker: "the harmonic signature underneath a chord", title: "Six numbers that survive transposing and flipping.", lede: "Every chord projects to six interval-class counts: how many minor seconds it contains, how many major seconds, and so on up to the tritone. Move the chord to any key, turn it upside down, and these six numbers do not change. Pick a chord and watch its signature; major and minor land on the same one.", body: "<ivf-fingerprint></ivf-fingerprint>" },
  { nav: "three rungs", kicker: "one chord, three rungs of forgetting", title: "A pitch-class set has a ladder, not a switch.", lede: "The set-class facet is the top of a short climb. Take one chord up the ladder a rung at a time: the literal notes, then the same set slid to its tightest packing (transposition forgotten), then the prime form (inversion forgotten too). Each rung forgets one more thing, and you can watch exactly what.", body: `<three-rungs></three-rungs>` },
  { nav: "two fingerprints", kicker: "two axes, not two rivals", title: "The compiler already emits a fingerprint. This is its complement.", lede: "Sourcify's metadata hash is the exact-identity fingerprint of a compilation: it answers \"is this the identical build,\" and a single whitespace flips it. A structural fingerprint answers the other question, \"is this the same code wearing different clothes,\" and is built to hold when the metadata hash moves. Edit the source below and watch the two axes part.", body: `<metadata-axes></metadata-axes>` },
  {
  nav: "main vs meta",
  kicker: "the wall riffcat steps around",
  title: "In the bytecode, you cannot tell Main from Meta.",
  lede: "Raw onchain bytecode is one run-on strip: the code (Main) and the metadata, immutables, libraries, and constructor arguments (Meta) sit interleaved, with no clean line between them. So a byte match has to guess. riffcat works one level up, on the source, where structure, names, constants, and types are already separate by construction.",
  body: `<byte-wall></byte-wall>`,
},
  {
    nav: "a shared block",
    kicker: "offered for co-design, not handed over",
    title: "A working core, brought here to react to.",
    lede: "riffcat is a small library with a thin CLI, the same engine that ran live in the chapters before this one. It could plausibly help several of the projects in this room. We are not asking you to adopt it; we are asking what it should become for the work you do.",
    body: `<shared-block></shared-block>`,
  },
  {
    nav: "what we need",
    kicker: "offered for co-design, not a handoff",
    title: "One problem, three contributors, none of us alone.",
    lede: "riffcat does one leg: it localizes, it points at the matching or changed subtree. The compiler can supply the provenance, source down to bytecode. A verifier adjudicates, with a proof or a counterexample. Hover a leg for the honest version of what we would need from that team, and the question we cannot answer ourselves.",
    body: `<collab-triangle></collab-triangle>`,
  },
];

// The URL hash deep-links the storybook: "#<chapter>" selects a chapter, and a
// chapter may own a sub-anchor after a slash ("#recognized/Contract.fn"). The
// chapter part is handled here; sub-anchors are left to the chapter's component.
const slugify = (s) => s.replace(/\s+/g, "-");

customElements.define("tour-app", class extends HTMLElement {
  connectedCallback() {
    const nav = document.getElementById("nav");
    nav.innerHTML = CH.map((c, k) => `<button data-k="${k}">${k === 0 ? "·" : k}. ${c.nav}</button>`).join("");
    nav.querySelectorAll("button").forEach((b) =>
      b.addEventListener("click", () => this.go(+b.dataset.k)));
    document.addEventListener("keydown", (e) => {
      if (e.target.closest("live-dial") || e.target.closest("recog-scan")) return; // let those keep focus
      if (e.key === "ArrowRight") this.go(this.i + 1);
      if (e.key === "ArrowLeft") this.go(this.i - 1);
    });
    this.i = this.chapterFromHash();
    addEventListener("hashchange", () => {
      const k = this.chapterFromHash();
      if (k !== this.i) this.go(k, true); // chapter changed via hash; do not rewrite it
    });
    this.render();
  }
  chapterFromHash() {
    const seg = decodeURIComponent((location.hash || "").replace(/^#/, "")).split("/")[0];
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
  render() {
    const c = CH[this.i];
    document.querySelectorAll("#nav button").forEach((b, k) =>
      b.setAttribute("aria-current", k === this.i ? "true" : "false"));
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
        <div class="kicker">${c.kicker}</div>
        <h1>${c.title}</h1>
        ${c.lede ? `<p class="lede">${c.lede}</p>` : ""}
        ${c.body}
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
      if (!this.bindings.fixture("demo")) throw new Error("demo fixture missing");
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
      `<span class="sw" style="background:${chipColor(c.dataset.eq.slice(3))}"></span>`
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
      // If the three libraries are not warm yet, say so before the (one-time,
      // synchronous) engine work and let that note paint first, so a cold visit
      // reads as "computing" rather than a frozen tab. A warm visit skips this
      // and renders straight from cache.
      if (!XREF.every((x) => yulCache.has(x.id + ":shape"))) {
        this.innerHTML = `<p class="live-note">fingerprinting three libraries in your browser…</p>`;
        await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      }
      let ms = 0;
      const rows = XREF.map((x) => { const r = yulUnits(b, x.id, "shape"); ms += r.ms; return { x, funcs: r.units.slice(1) }; });
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
        return `<span class="sw" style="background:${chipColor(c.dataset.eq.slice(3))}"></span>`
          + `${c.dataset.name} · <span style="color:var(--warm)">${c.dataset.fp}</span> · ${where}`
          + dimStrip(c, lit);
      });
    } catch (e) {
      this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`;
    }
  }
});

// Recognition: real verified contracts (baked in realdata.js) scanned against
// the committed std-lib catalog. Each function is colored by the library it was
// recognized as (or grey if novel app code); hovering lights the same shape
// across every contract and shows the actual Solidity in a code panel. The
// recognition is precomputed (fingerprint + catalog lookup), so this view needs
// no engine at runtime.
// keyed by the catalog's lowercase library id; n = display name, h = hue.
const LIB = {
  openzeppelin: { n: "OpenZeppelin", h: 210 },
  solady: { n: "Solady", h: 145 },
  solmate: { n: "Solmate", h: 32 },
};

// Minimal, safe Solidity highlighter: tokenize comments/strings out first, then
// color keywords and value types only inside code segments (no nested mangling).
function solHi(src) {
  const esc = (t) => t.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const KW = /\b(function|returns?|memory|storage|calldata|public|private|internal|external|view|pure|payable|require|revert|assert|if|else|for|while|do|mapping|struct|enum|event|emit|new|delete|unchecked|assembly|modifier|constructor|using|override|virtual|import|pragma|contract|library|interface|is|abstract|immutable|constant|try|catch)\b/g;
  const TY = /\b(address|uint\d*|int\d*|bool|bytes\d*|string)\b/g;
  const code = (t) => esc(t).replace(KW, '<span class="c-kw">$1</span>').replace(TY, '<span class="c-ty">$1</span>');
  const re = /(\/\/[^\n]*|\/\*[\s\S]*?\*\/)|("(?:[^"\\]|\\.)*"|'(?:[^'\\]|\\.)*')/g;
  let out = "", last = 0, m;
  while ((m = re.exec(src))) {
    out += code(src.slice(last, m.index));
    out += m[1] ? `<span class="c-com">${esc(m[1])}</span>` : `<span class="c-str">${esc(m[2])}</span>`;
    last = re.lastIndex;
  }
  return out + code(src.slice(last));
}

// Facet primer: five tiny, fully-readable functions, fingerprinted live at the
// source level (sol-fn). Slide the facet and watch which functions collapse to
// the same shape and which never do. This is the vocabulary every later chapter
// leans on, taught on code you can read in one glance. Data+AST in facetdemo.js.
const FACET_GLOSS = {
  "full": "every dimension counts",
  "names-blind": "names dropped; constants and types still count",
  "structure": "shape only; names, constants, and types all dropped",
};
const feq = (d) => "fe-" + d.slice(0, 12);
customElements.define("facet-primer", class extends HTMLElement {
  async connectedCallback() {
    this.facet = "full";
    const data = window.RIFFCAT_FACET;
    if (!data) { this.innerHTML = `<p class="live-note bad">primer data not loaded</p>`; return; }
    this.fns = data.fns;
    this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
    try {
      const b = await engineReady;
      const t0 = performance.now();
      const units = JSON.parse(b.fingerprint_source(data.ast, "shape")).filter((u) => u.unit === "sol-fn");
      this._ms = performance.now() - t0;
      this.byName = {};
      for (const u of units) this.byName[u.name.split(".").pop()] = u;
      this.render();
    } catch (e) { this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`; }
  }
  facetCount(f) { return new Set(this.fns.map((x) => this.byName[x.name].facets[f])).size; }
  render() {
    const FACETS = ["full", "names-blind", "structure"];
    const seen = new Map(), order = []; // digest -> [names], in display order
    for (const f of this.fns) {
      const d = this.byName[f.name].facets[this.facet];
      if (!seen.has(d)) { seen.set(d, []); order.push(d); }
      seen.get(d).push(f.name);
    }
    const cards = this.fns.map((f) => {
      const d = this.byName[f.name].facets[this.facet], col = chipColor(d);
      return `<div class="fcard ${feq(d)}" data-eq="${feq(d)}" style="--chip:${col}">`
        + `<div class="fcard-h"><span class="sw" style="background:${col}"></span>${f.name}</div>`
        + `<pre class="fcode">${solHi(f.src)}</pre></div>`;
    }).join("");
    const ladder = FACETS.map((f) =>
      `<span class="stop ${f === this.facet ? "on" : ""}" data-facet="${f}"><b>${this.facetCount(f)}</b> ${f}</span>`).join("");
    const groupTxt = order.map((d) => { const n = seen.get(d); return n.length > 1 ? `<b>${n.join(" = ")}</b>` : n[0]; }).join(" · ");
    const idle = `<b>${seen.size}</b> shape${seen.size === 1 ? "" : "s"} at ${this.facet} · ${FACET_GLOSS[this.facet]}`
      + ` · ${groupTxt} · ${this._ms.toFixed(0)} ms in your browser`;
    this.innerHTML = `
      <div class="dialbar"><div class="grp"><span>facet</span><div class="ladder">${ladder}</div></div></div>
      <div class="fgrid">${cards}</div>
      <div class="eqread" data-idle="${idle}">${idle}</div>`;
    this.querySelectorAll("[data-facet]").forEach((s) =>
      s.addEventListener("click", () => { if (this.facet !== s.dataset.facet) { this.facet = s.dataset.facet; this.render(); } }));
    this.querySelectorAll(".fcard").forEach((card) => {
      card.addEventListener("mouseenter", () => {
        const twins = this.querySelectorAll("." + card.dataset.eq);
        this.querySelectorAll(".fcard").forEach((x) => x.classList.add("dim"));
        twins.forEach((x) => { x.classList.remove("dim"); x.classList.add("twin"); });
        const read = this.querySelector(".eqread");
        if (read) read.innerHTML = twins.length > 1
          ? `<b>${twins.length}</b> share this shape at ${this.facet}: <b>${[...twins].map((x) => x.querySelector(".fcard-h").textContent.trim()).join(" = ")}</b>`
          : `<b>${card.querySelector(".fcard-h").textContent.trim()}</b> is unique at ${this.facet}: no other function has this shape`;
      });
      card.addEventListener("mouseleave", () => {
        this.querySelectorAll(".fcard").forEach((x) => x.classList.remove("dim", "twin"));
        const read = this.querySelector(".eqread");
        if (read) read.innerHTML = read.dataset.idle || "";
      });
    });
  }
});

const RECOG_HINT = "hover a function for its source; click to pin it (the URL updates, so the view is shareable)";
customElements.define("recog-scan", class extends HTMLElement {
  connectedCallback() {
    const data = window.RIFFCAT_REAL;
    if (!data || !data.contracts) { this.innerHTML = `<p class="live-note bad">recognition data not loaded</p>`; return; }
    this.contracts = data.contracts;
    this.spread = new Map(); // nb12 -> Set(contract index)
    this.contracts.forEach((c, ci) => c.fns.forEach((f) => {
      if (!this.spread.has(f.nb)) this.spread.set(f.nb, new Set());
      this.spread.get(f.nb).add(ci);
    }));
    this.onHash = () => this.selectByHash(true);
    addEventListener("hashchange", this.onHash);
    this.render();
    this.selectByHash(false); // honor a deep-link anchor on load
  }
  disconnectedCallback() { removeEventListener("hashchange", this.onHash); }
  render() {
    const rows = this.contracts.map((c, ci) => {
      const rec = c.fns.filter((f) => f.lib).length;
      const chips = c.fns.map((f, fi) => {
        const cls = f.lib ? "chip rec" : "chip novel";
        const hue = f.lib && LIB[f.lib] ? `style="--chip:oklch(72% 0.15 ${LIB[f.lib].h})"` : "";
        const label = f.fn === "constructor" ? "constructor" : f.fn;
        return `<span class="${cls} eq-${f.nb}" data-eq="eq-${f.nb}" data-ci="${ci}" data-fi="${fi}"`
          + ` data-anchor="${encodeURIComponent(c.name + "." + f.fn)}" ${hue} title="${c.name}.${f.fn}">${label}</span>`;
      }).join("");
      return `<div class="eqrow recogrow">
        <div class="libname"><b>${c.name}</b><span class="ver">${c.version}</span>
          <a class="srcfy" href="${c.url}" target="_blank" rel="noopener">sourcify ↗</a>
          <span class="rate"><b>${rec}</b>/${c.fns.length} std-lib</span></div>
        <div class="eqgrid" data-ci="${ci}">${chips}</div></div>`;
    }).join("");
    const legend = `<div class="legend">`
      + Object.values(LIB).map(({ n, h }) => `<span><i style="background:hsl(${h} 70% 55%)"></i>${n}</span>`).join("")
      + `<span><i class="novel"></i>novel / app code</span></div>`;
    // codedock reserves a constant height; the panel inside sizes to content.
    // Keeping the dock height fixed means hovering never changes the page
    // height, so scrollTop never clamps and the chips never bump (the flicker),
    // while the visible panel still hugs each function's source.
    this.innerHTML = `${legend}${rows}<div class="codedock"><div class="codepanel"><div class="cphint">${RECOG_HINT}</div></div></div>`;
    this.panel = this.querySelector(".codepanel");
    this.addEventListener("mouseover", (e) => { const c = e.target.closest(".chip"); if (c && this.contains(c)) this.show(c); });
    this.addEventListener("mouseleave", () => this.rest()); // fires once on leaving; settles to the pinned anchor or clears, never sticks
    this.addEventListener("click", (e) => { const c = e.target.closest(".chip"); if (c && this.contains(c)) this.toggleAnchor(c); });
  }
  show(chip) { // transient view (hover): light the shape's network + fill the panel
    this.querySelectorAll(".chip.lit").forEach((x) => x.classList.remove("lit"));
    this.querySelectorAll(".eqgrid").forEach((g) => g.classList.add("focused"));
    this.querySelectorAll("." + chip.dataset.eq).forEach((x) => x.classList.add("lit"));
    this.panel.innerHTML = this.panelHTML(chip);
  }
  rest() { // settle back to the pinned anchor, or clear if nothing is pinned
    if (this.anchorChip) this.show(this.anchorChip);
    else {
      this.querySelectorAll(".chip.lit").forEach((x) => x.classList.remove("lit"));
      this.querySelectorAll(".eqgrid").forEach((g) => g.classList.remove("focused"));
      this.panel.innerHTML = `<div class="cphint">${RECOG_HINT}</div>`;
    }
  }
  toggleAnchor(chip) {
    if (chip === this.anchorChip) { // click the pinned one again to unpin
      chip.classList.remove("anchored"); this.anchorChip = null; this.rest();
      if (location.hash !== "#recognized") location.hash = "#recognized";
      return;
    }
    this.pin(chip, false);
  }
  pin(chip, fromHash) {
    this.querySelectorAll(".chip.anchored").forEach((x) => x.classList.remove("anchored"));
    chip.classList.add("anchored");
    this.anchorChip = chip;
    this.show(chip);
    if (!fromHash) { const h = "#recognized/" + chip.dataset.anchor; if (location.hash !== h) location.hash = h; }
  }
  selectByHash(fromHash) {
    const parts = (location.hash || "").replace(/^#/, "").split("/");
    if (parts[0] !== "recognized" || !parts[1]) return;
    const chip = this.querySelector(`.chip[data-anchor="${parts.slice(1).join("/")}"]`);
    if (chip && chip !== this.anchorChip) { this.pin(chip, true); chip.scrollIntoView({ block: "center", behavior: "smooth" }); }
  }
  panelHTML(chip) {
    const c = this.contracts[+chip.dataset.ci], f = c.fns[+chip.dataset.fi];
    const inN = this.spread.get(f.nb)?.size || 1;
    const verdict = f.lib && LIB[f.lib]
      ? `recognized as <span class="reclib" style="color:hsl(${LIB[f.lib].h} 70% 62%)">${LIB[f.lib].n} ${f.canon}</span>`
      : `<span class="recnovel">novel / app-specific</span> (no std-lib match)`;
    const also = inN > 1 ? ` · same shape in <b>${inN}</b> of these contracts` : "";
    return `<div class="cphead"><b>${c.name}.${f.fn}</b> · ${verdict} · <span class="fp">${f.nb.slice(0, 8)}</span>${also}`
      + `<a href="${c.url}" target="_blank" rel="noopener">on sourcify ↗</a></div>`
      + `<pre class="code">${solHi(f.src)}</pre>`;
  }
});

// Twins across real contracts: the same recognized library function, the exact
// same shape, in more than one of the verified contracts. Pick one from the list
// to see its source and every contract that carries it. Precomputed (realdata).
customElements.define("recog-twins", class extends HTMLElement {
  connectedCallback() {
    const data = window.RIFFCAT_REAL;
    if (!data || !data.contracts) { this.innerHTML = `<p class="live-note bad">recognition data not loaded</p>`; return; }
    this.contracts = data.contracts;
    const g = new Map(); // lib+canon -> { lib, canon, inst:[{ci,fi}], cset:Set(ci) }
    this.contracts.forEach((c, ci) => c.fns.forEach((f, fi) => {
      if (!f.lib || !LIB[f.lib]) return;
      const k = f.lib + "" + f.canon;
      if (!g.has(k)) g.set(k, { lib: f.lib, canon: f.canon, inst: [], cset: new Set() });
      const e = g.get(k); e.inst.push({ ci, fi }); e.cset.add(ci);
    }));
    this.shared = [...g.values()].filter((e) => e.cset.size >= 2)
      .sort((a, b) => b.cset.size - a.cset.size || a.canon.localeCompare(b.canon));
    this.sel = 0;
    this.render();
  }
  render() {
    const n = this.contracts.length;
    const list = this.shared.map((e, i) =>
      `<button class="twrow ${i === this.sel ? "on" : ""}" data-i="${i}">`
      + `<span class="tw-c"><b>${e.cset.size}</b>/${n}</span>`
      + `<span class="tw-n">${e.canon}</span>`
      + `<span class="tw-lib" style="color:hsl(${LIB[e.lib].h} 70% 62%)">${LIB[e.lib].n}</span></button>`).join("");
    this.innerHTML = `<div class="twwrap"><div class="twlist">${list}</div><div class="twview codepanel"></div></div>`;
    this.querySelectorAll(".twrow").forEach((b) =>
      b.addEventListener("click", () => {
        this.sel = +b.dataset.i;
        this.querySelectorAll(".twrow").forEach((x, i) => x.classList.toggle("on", i === this.sel));
        this.renderView();
      }));
    this.renderView();
  }
  renderView() {
    const e = this.shared[this.sel], rep = e.inst[0], c = this.contracts[rep.ci], f = c.fns[rep.fi];
    const chips = [...e.cset].sort((a, b) => a - b).map((ci) => `<span class="cchip">${this.contracts[ci].name}</span>`).join("");
    this.querySelector(".twview").innerHTML =
      `<div class="cphead">the same shape in <b>${e.cset.size}</b> of ${this.contracts.length}: `
      + `<b style="color:hsl(${LIB[e.lib].h} 70% 62%)">${LIB[e.lib].n} ${e.canon}</b></div>`
      + `<div class="twcontracts">${chips}</div>`
      + `<pre class="code">${solHi(f.src)}</pre>`
      + `<p class="twnote">Audit it once. Every contract above carries this shape, identifiers aside.</p>`;
  }
});

// Dedup at the source level: of every function across the real contracts, how
// much is a shape already in the std-lib catalog (the part you do not read) vs
// novel app code (the part you do). One bar per contract. Precomputed (realdata).
customElements.define("recog-dedup", class extends HTMLElement {
  connectedCallback() {
    const data = window.RIFFCAT_REAL;
    if (!data || !data.contracts) { this.innerHTML = `<p class="live-note bad">recognition data not loaded</p>`; return; }
    this.contracts = data.contracts;
    let known = 0, total = 0; const byLib = {};
    for (const c of this.contracts) for (const f of c.fns) { total++; if (f.lib && LIB[f.lib]) { known++; byLib[f.lib] = (byLib[f.lib] || 0) + 1; } }
    this.stats = { known, total, novel: total - known, byLib };
    this.render();
  }
  render() {
    const s = this.stats, pct = Math.round(100 * s.known / s.total), n = this.contracts.length;
    const libOrder = Object.entries(s.byLib).sort((a, b) => b[1] - a[1]);
    const segs = libOrder.map(([lib, k]) => `<span style="flex:${k};background:hsl(${LIB[lib].h} 58% 46%)" title="${LIB[lib].n} ${k}"></span>`).join("")
      + `<span class="seg-novel" style="flex:${s.novel}" title="novel ${s.novel}"></span>`;
    const legend = libOrder.map(([lib, k]) => `<span><i style="background:hsl(${LIB[lib].h} 58% 46%)"></i>${LIB[lib].n} ${k}</span>`).join("")
      + `<span><i class="novel"></i>novel ${s.novel}</span>`;
    const rows = this.contracts.map((c) => {
      const seg = {}; for (const f of c.fns) { const key = (f.lib && LIB[f.lib]) ? f.lib : "novel"; seg[key] = (seg[key] || 0) + 1; }
      const bar = Object.entries(seg).sort((a, b) => (a[0] === "novel" ? 1 : 0) - (b[0] === "novel" ? 1 : 0))
        .map(([key, k]) => key === "novel"
          ? `<span class="seg-novel" style="flex:${k}"></span>`
          : `<span style="flex:${k};background:hsl(${LIB[key].h} 58% 46%)"></span>`).join("");
      const known = c.fns.filter((f) => f.lib && LIB[f.lib]).length;
      return `<div class="dduprow"><span class="ddup-n">${c.name}</span><div class="ddupbar">${bar}</div><span class="ddup-r">${known}/${c.fns.length}</span></div>`;
    }).join("");
    this.innerHTML = `
      <div class="dduphead"><b>${s.known}</b> of <b>${s.total}</b> functions across these ${n} contracts are shapes already in OpenZeppelin, Solady, or Solmate <span class="ddup-pct">${pct}%</span></div>
      <div class="ddupbar big">${segs}</div>
      <div class="legend">${legend}</div>
      <div class="dduprows">${rows}</div>
      <p class="twnote">Each bar is one contract; the grey is its novel surface, the only part an auditor reads closely. TimelockController is entirely standard, Airdrop almost entirely its own.</p>`;
  }
});

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
  vulnerable: "Vulnerable: when the vault is empty this mints shares 1:1, so an attacker can steal the first real deposit.",
  patched: "The fix: same function name, structurally different. The fingerprint separates it from the vulnerable shape, so you can see who patched.",
  mitigated: "Safe look-alike: keeps the _initialConvertToShares name a text search would flag, but routes through virtual shares, so its shape is not the vulnerable one.",
};
customElements.define("vuln-sniff", class extends HTMLElement {
  connectedCallback() {
    const data = window.RIFFCAT_VULN;
    if (!data || !data.shapes) { this.innerHTML = `<p class="live-note bad">vuln data not loaded</p>`; return; }
    this.data = data;
    this.shapes = data.shapes;
    this.witnesses = data.witnesses;
    this.sel = 0;
    this.render();
  }
  render() {
    const r = this.data.reach;
    const rows = this.shapes.map((s, i) => {
      const st = VULN_STATUS[s.status];
      const verd = s.status === "vulnerable" ? "vulnerable" : s.status === "patched" ? "safe: patched" : "safe: look-alike";
      const exact = s.exact == null ? "-" : s.exact.toLocaleString();
      const reach = s.reach == null ? "-" : s.reach.toLocaleString();
      return `<tr class="lxrow ${i === this.sel ? "on" : ""}" data-i="${i}" style="--hue:${st.hue}">`
        + `<td class="lx-shape">${s.label.replace("OpenZeppelin", "OZ")} <span class="vsub">${s.sub}</span></td>`
        + `<td class="lx-fp">${s.structure.slice(0, 8)}</td>`
        + `<td class="lx-verd ${s.status === "vulnerable" ? "bad" : "ok"}">${verd}</td>`
        + `<td class="lx-n">${exact}</td>`
        + `<td class="lx-n lx-reach">${reach}</td></tr>`;
    }).join("");
    this.innerHTML = `
      <p class="vbug">${this.data.bug}</p>
      <p class="vlead">All four rows are functions named <code>convertToShares</code>. Across Sourcify's verified ERC-4626 vaults, an exact-source match flags <b>${r.exactVuln.toLocaleString()}</b> as the vulnerable file; matching the <b>shape</b> flags <b>${r.shapeVuln.toLocaleString()}</b>, the same bug in <b>${r.extra}</b> more vaults (${r.variants} source variants) an exact match treats as unrelated.</p>
      <table class="vledger"><thead><tr><th>shape</th><th>structure</th><th>riffcat</th><th>Sourcify exact</th><th>riffcat shape</th></tr></thead><tbody>${rows}</tbody></table>
      <p class="vledger-cap">These counts classify the distinct source files (one exact file is one shape), so they are exact within Sourcify's corpus and a floor, not a per-contract crawl. Click a row for its source and real deployed examples. Solady (last row) keeps the function name a text search would flag, but its shape is the safe one.</p>
      <div class="vbody codepanel"></div>`;
    this.querySelectorAll(".lxrow").forEach((row) =>
      row.addEventListener("click", () => { this.sel = +row.dataset.i; this.querySelectorAll(".lxrow").forEach((x, i) => x.classList.toggle("on", i === this.sel)); this.renderBody(); }));
    this.renderBody();
  }
  renderBody() {
    const s = this.shapes[this.sel], st = VULN_STATUS[s.status];
    const wits = this.witnesses.filter((w) => w.shape === s.key);
    const chips = wits.map((w) =>
      `<a class="wchip" href="${w.url}" target="_blank" rel="noopener">${w.name}<span class="wch">${w.chainName}</span></a>`).join("");
    const match = s.reach
      ? `riffcat matched this shape in <b>${s.reach.toLocaleString()}</b> verified vaults (an exact-source match finds ${s.exact.toLocaleString()}), for example:`
      : `kept the name, but its shape is not the vulnerable one; these are safe:`;
    this.querySelector(".vbody").innerHTML =
      `<div class="vhead" style="--hue:${st.hue}"><span class="vbadge">${st.label}</span> <b>${s.label}</b> <span class="vsub">${s.sub}</span>`
      + `<span class="vfp">structure ${s.structure.slice(0, 10)}</span></div>`
      + `<p class="vrole">${VULN_ROLE[s.status]}</p>`
      + `<pre class="code">${solHi(s.src)}</pre>`
      + `<div class="vwits"><span class="vwlabel">${match}</span>${chips}</div>`;
  }
});

// Modified-variant detection (fuzzy). Real deployed forks that EDITED the
// vulnerable Multicall body, so exact whole-function matching, Sourcify's
// byte-identical match, and a text search all miss them, but weighted
// containment over the per-node Merkle digests still recognizes the shape.
// Precomputed in fuzzydata.js (the proposed `riffcat similar`); Sourcify floor.
customElements.define("fuzzy-scan", class extends HTMLElement {
  connectedCallback() {
    const d = window.RIFFCAT_FUZZY;
    if (!d || !d.variants) { this.innerHTML = `<p class="live-note bad">fuzzy data not loaded</p>`; return; }
    this.data = d;
    this.variants = d.variants;
    this.sel = 0;
    this.render();
  }
  render() {
    const d = this.data;
    const rows = this.variants.map((v, i) =>
      `<tr class="lxrow ${i === this.sel ? "on" : ""}" data-i="${i}">`
      + `<td class="lx-shape">${v.name} <span class="vsub">${v.chainName}</span></td>`
      + `<td class="lx-verd bad">no class</td>`
      + `<td class="lx-n lx-reach">${v.cwVuln.toFixed(2)}</td></tr>`).join("");
    this.innerHTML = `
      <p class="vbug">${d.bug}</p>
      <p class="vlead">Each fork below <b>edited</b> the <code>multicall</code> body, so its shape is neither the vulnerable class nor the patch: exact match, Sourcify's byte-identical match, and a text search all return <b>nothing</b>. Weighted containment of the vulnerable shape still finds them. Click a row.</p>
      <div class="codepanel fref"><div class="cphead">the vulnerable shape being matched <span class="vfp">structure ${d.vuln.fp}</span> <span class="vsub">${d.vuln.nodes} nodes</span></div><pre class="code">${solHi(d.vuln.src)}</pre></div>
      <table class="vledger"><thead><tr><th>fork (edited the body)</th><th>exact match</th><th>fuzzy</th></tr></thead><tbody>${rows}</tbody></table>
      <div class="vbody codepanel"></div>
      <p class="vledger-cap">The coincidence floor for this shape is ~<b>${d.nullCeiling}</b> and the OZ patch scores ~<b>${d.patchedScore}</b>; both catches clear it. Sourcify-verified, a floor. Two further customized vulnerable bodies (${d.marginal.map((m) => m.name).join(", ")}) land near the floor and the score alone cannot certify them.</p>`;
    this.querySelectorAll(".lxrow").forEach((row) =>
      row.addEventListener("click", () => { this.sel = +row.dataset.i; this.querySelectorAll(".lxrow").forEach((x, i) => x.classList.toggle("on", i === this.sel)); this.renderBody(); }));
    this.renderBody();
  }
  renderBody() {
    const v = this.variants[this.sel];
    this.querySelector(".vbody").innerHTML =
      `<div class="vhead" style="--hue:2"><span class="vbadge">caught, modified</span> <b>${v.name}</b> <span class="vsub">${v.chainName}, ${v.nodes} nodes</span>`
      + `<span class="vfp">fuzzy ${v.cwVuln.toFixed(2)} vs vuln, ${v.cwPatch.toFixed(2)} vs patch</span>`
      + `<a class="vsrcfy" href="https://sourcify.dev/#/lookup/${v.address}" target="_blank" rel="noopener">on sourcify ↗</a></div>`
      + `<p class="vrole">${v.edit}</p>`
      + `<pre class="code">${solHi(v.src)}</pre>`;
  }
});

// The riff chapter: the same dial, on music. A short motif and a few variants,
// fingerprinted by the very same engine (fingerprint_riff) at musical facets.
// Pick a facet and the variants that count as "the same" share a color, exactly
// like the code chapters. Playback is raw Web Audio (one triangle osc per note);
// the click is the user gesture the browser needs to start audio.
const NOTE_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const noteName = (p) => NOTE_NAMES[((p % 12) + 12) % 12] + (Math.floor(p / 12) - 1);
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
  { name: "An die Musik", notes: [[69, 2], [69, 1], [71, 1], [69, 2], [66, 1], [64, 1], [66, 2], [62, 2]] },
  { name: "up a fifth", notes: [[76, 2], [76, 1], [78, 1], [76, 2], [73, 1], [71, 1], [73, 2], [69, 2]] },
  { name: "same notes, re-voiced", notes: [[62, 1], [78, 1], [66, 1], [81, 1], [64, 1], [71, 2]] },
  { name: "same rhythm, new notes", notes: [[72, 2], [67, 1], [71, 1], [67, 2], [65, 1], [69, 1], [67, 2], [72, 2]] },
  { name: "a different riff", notes: [[60, 1], [60, 1], [67, 1], [67, 1], [69, 2]] },
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
  const len = Math.floor(_ac.sampleRate * 1.5), ir = _ac.createBuffer(2, len, _ac.sampleRate);
  for (let ch = 0; ch < 2; ch++) {
    const data = ir.getChannelData(ch);
    for (let i = 0; i < len; i++) data[i] = (Math.random() * 2 - 1) * Math.pow(1 - i / len, 2.8);
  }
  const verb = _ac.createConvolver(); verb.buffer = ir;
  const wet = _ac.createGain(); wet.gain.value = 0.25;
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
    const o1 = ac.createOscillator(); o1.type = "sine"; o1.frequency.value = freq;
    const o2 = ac.createOscillator(); o2.type = "sine"; o2.frequency.value = freq * 2;
    const o2g = ac.createGain(); o2g.gain.value = 0.1;
    // vibrato: gentle, eased in over the note's first moments
    const lfo = ac.createOscillator(); lfo.type = "sine"; lfo.frequency.value = 5.5;
    const lfoG = ac.createGain();
    lfoG.gain.setValueAtTime(0, t);
    lfoG.gain.linearRampToValueAtTime(freq * 0.007, t + Math.min(0.22, d * 0.6));
    lfo.connect(lfoG); lfoG.connect(o1.frequency); lfoG.connect(o2.frequency);
    // breath: a whisper of band-passed noise, gated with the note
    const nlen = Math.ceil(d * ac.sampleRate) + 1, nb = ac.createBuffer(1, nlen, ac.sampleRate), nd = nb.getChannelData(0);
    for (let i = 0; i < nlen; i++) nd[i] = Math.random() * 2 - 1;
    const noise = ac.createBufferSource(); noise.buffer = nb;
    const nbp = ac.createBiquadFilter(); nbp.type = "bandpass"; nbp.frequency.value = freq * 2.5; nbp.Q.value = 0.6;
    const ng = ac.createGain();
    const lp = ac.createBiquadFilter(); lp.type = "lowpass"; lp.frequency.value = 2400;
    const env = ac.createGain();
    const A = 0.06, R = 0.16, peak = 0.16;
    env.gain.setValueAtTime(0.0001, t);
    env.gain.linearRampToValueAtTime(peak, t + A);
    env.gain.setValueAtTime(peak, t + Math.max(A + 0.01, d - R));
    env.gain.exponentialRampToValueAtTime(0.0006, t + d);
    ng.gain.setValueAtTime(0, t);
    ng.gain.linearRampToValueAtTime(0.02, t + A);
    ng.gain.linearRampToValueAtTime(0.0001, t + d);
    o1.connect(lp); o2.connect(o2g).connect(lp);
    noise.connect(nbp).connect(ng).connect(lp);
    lp.connect(env);
    env.connect(ac.destination); env.connect(ac._verb);
    o1.start(t); o2.start(t); lfo.start(t); noise.start(t);
    const end = t + d + 0.06;
    o1.stop(end); o2.stop(end); lfo.stop(end); noise.stop(end);
    t += d;
  }
}
customElements.define("riff-dial", class extends HTMLElement {
  async connectedCallback() {
    this.facet = "harmonic_relationships";
    this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
    try {
      this.b = await engineReady;
      this.fp = RIFFS.map((r) => ({
        ...r,
        addr: JSON.parse(this.b.fingerprint_riff(
          JSON.stringify({ notes: r.notes.map(([pitch, dur]) => ({ pitch, dur })) }))),
      }));
      this.render();
    } catch (e) { this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`; }
  }
  render() {
    const ladder = RIFF_FACETS.map(([k, label]) =>
      `<span class="stop ${k === this.facet ? "on" : ""}" data-facet="${k}">${label}</span>`).join("");
    const groups = new Map();
    for (const r of this.fp) {
      const a = r.addr[this.facet];
      if (!groups.has(a)) groups.set(a, []);
      groups.get(a).push(r.name);
    }
    const rows = this.fp.map((r, i) => {
      const a = r.addr[this.facet];
      const chips = r.notes.map(([p]) => `<span class="nchip">${noteName(p)}</span>`).join("");
      return `<div class="riffrow"><button class="playbtn" data-i="${i}">▶ play</button>`
        + `<span class="riffname">${r.name}</span>`
        + `<span class="nchips">${chips}</span>`
        + `<span class="shapedot" style="--chip:${chipColor(a)}" title="shape ${a.slice(0, 10)}"></span></div>`;
    }).join("");
    const n = groups.size;
    const groupTxt = [...groups.values()]
      .map((names) => names.length > 1 ? `<b>${names.join(" = ")}</b>` : names[0]).join(" · ");
    const idle = `<b>${n}</b> shape${n === 1 ? "" : "s"} at this facet · ${groupTxt}`;
    this.innerHTML = `
      <div class="dialbar"><div class="grp"><span>facet</span><div class="ladder">${ladder}</div></div></div>
      <div class="riffs">${rows}</div>
      <div class="eqread">${idle}</div>
      <p class="twnote">The same dial as the code chapters, on music. Transpose the riff and the <b>intervals</b> survive; keep the durations and the <b>rhythm</b> survives; reorder and re-octave the notes and the <b>note set</b> survives. The engine computes each with the very same facet machinery (<code>fingerprint_riff</code>).</p>`;
    this.querySelectorAll("[data-facet]").forEach((s) =>
      s.addEventListener("click", () => { if (this.facet !== s.dataset.facet) { this.facet = s.dataset.facet; this.render(); } }));
    this.querySelectorAll(".playbtn").forEach((btn) =>
      btn.addEventListener("click", () => playRiff(this.fp[+btn.dataset.i].notes)));
  }
});

// Play a chord: its pitch classes sounded together, soft flute-ish (shares the
// reverb with the riff voice). A simpler voice than playRiff (no breath layer).
function playChord(pcs) {
  const ac = fluteCtx();
  if (!ac) return;
  if (ac.state === "suspended") ac.resume();
  const t = ac.currentTime + 0.05, d = 1.7, gain = 0.13 / Math.max(2, pcs.length);
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
const CHORD_FACETS = [["note_set", "note set"], ["set_class", "set class"]];
const CHORDS = ["C", "Cdo", "Am", "F", "Caug", "Bdim"];
customElements.define("chord-fp", class extends HTMLElement {
  async connectedCallback() {
    this.facet = "set_class";
    this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
    try {
      const b = await engineReady;
      this.data = CHORDS
        .map((c) => { try { return { c, fp: JSON.parse(b.fingerprint_chord(c)) }; } catch { return null; } })
        .filter(Boolean);
      this.render();
    } catch (e) { this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`; }
  }
  render() {
    const ladder = CHORD_FACETS.map(([k, label]) =>
      `<span class="stop ${k === this.facet ? "on" : ""}" data-facet="${k}">${label}</span>`).join("");
    const groups = new Map();
    for (const d of this.data) {
      const a = d.fp[this.facet];
      if (!groups.has(a)) groups.set(a, []);
      groups.get(a).push(d.c);
    }
    const rows = this.data.map((d, i) => {
      const a = d.fp[this.facet];
      const notes = d.fp.pitch_classes.map((pc) => `<span class="nchip">${NOTE_NAMES[pc]}</span>`).join("");
      const pf = this.facet === "set_class" ? ` <span class="vsub">prime [${d.fp.prime_form.join(" ")}]</span>` : "";
      return `<div class="riffrow"><button class="playbtn" data-i="${i}">▶ play</button>`
        + `<span class="riffname">${d.c}</span><span class="nchips">${notes}</span>${pf}`
        + `<span class="shapedot" style="--chip:${chipColor(a)}" title="${this.facet} ${a.slice(0, 10)}"></span></div>`;
    }).join("");
    const n = groups.size;
    const gtxt = [...groups.values()]
      .map((cs) => cs.length > 1 ? `<b>${cs.join(" = ")}</b>` : cs[0]).join(" · ");
    const note = this.facet === "set_class"
      ? `At the <b>set-class</b> facet, riffcat lands on Allen Forte's catalog: C, Cdo, Am and F collapse to one class (major and minor triads are the same set class, 3-11), while the augmented and diminished triads are their own. The engine rediscovers set theory from the structure alone.`
      : `At the <b>note-set</b> facet, two spellings of the same notes share an anchor (C and its solfege spelling Cdo); every other chord is its own set of notes.`;
    this.innerHTML = `
      <div class="dialbar"><div class="grp"><span>facet</span><div class="ladder">${ladder}</div></div></div>
      <div class="riffs">${rows}</div>
      <div class="eqread"><b>${n}</b> shape${n === 1 ? "" : "s"} at this facet · ${gtxt}</div>
      <p class="twnote">${note}</p>`;
    this.querySelectorAll("[data-facet]").forEach((s) =>
      s.addEventListener("click", () => { if (this.facet !== s.dataset.facet) { this.facet = s.dataset.facet; this.render(); } }));
    this.querySelectorAll(".playbtn").forEach((btn) =>
      btn.addEventListener("click", () => playChord(this.data[+btn.dataset.i].fp.pitch_classes)));
  }
});

// Framing the verifier: the syntactic-to-semantic picture, corrected. The
// structural side is a LATTICE of facets (forget names, or constants, or types,
// independently; finer agreement implies coarser), not a point on a line.
// Semantic equivalence is an ORTHOGONAL axis (undecidable in general), the
// verifier's job. riffcat localizes on the lattice and hands FV a scoped
// obligation. Hand-laid SVG; hover a node for what it forgets.
const LATTICE_NODES = {
  full: { x: 250, y: 58, t: "full", s: "keep every dimension: exact identity" },
  namesblind: { x: 118, y: 168, t: "names-blind", s: "forget names: same code modulo identifiers" },
  constblind: { x: 382, y: 168, t: "constants-blind", s: "forget constants: same shape modulo literals" },
  structure: { x: 250, y: 278, t: "structure", s: "forget names, constants, and types: pure shape" },
  semantic: { x: 600, y: 168, t: "semantic", s: "same behavior on all inputs: undecidable in general, and the verifier's axis, not riffcat's", sem: true },
};
const LATTICE_EDGES = [["full", "namesblind"], ["full", "constblind"], ["namesblind", "structure"], ["constblind", "structure"]];
customElements.define("facet-lattice", class extends HTMLElement {
  connectedCallback() {
    const N = LATTICE_NODES;
    const edges = LATTICE_EDGES.map(([a, b]) =>
      `<line x1="${N[a].x}" y1="${N[a].y + 15}" x2="${N[b].x}" y2="${N[b].y - 15}" class="latedge"/>`).join("");
    const node = (k) => {
      const n = N[k];
      return `<g class="latnode ${n.sem ? "sem" : ""}" data-k="${k}" transform="translate(${n.x},${n.y})">`
        + `<rect x="-68" y="-15" width="136" height="30" rx="5"/>`
        + `<text x="0" y="4" text-anchor="middle">${n.t}</text></g>`;
    };
    const idle = "Hover a facet. Downward edges forget one more dimension; equal at a finer facet implies equal at every coarser one.";
    this.innerHTML = `
      <svg viewBox="0 0 720 320" class="lattice" role="img" aria-label="facet lattice and the orthogonal semantic axis">
        <defs><marker id="latarr" markerWidth="9" markerHeight="9" refX="6" refY="3" orient="auto">
          <path d="M0,0 L6,3 L0,6 z" class="latarrhead"/></marker></defs>
        <text x="250" y="24" text-anchor="middle" class="latcap">structural facets (a lattice)</text>
        <text x="600" y="24" text-anchor="middle" class="latcap sem">meaning (the verifier's axis)</text>
        <line x1="478" y1="42" x2="478" y2="292" class="latdivide"/>
        ${edges}
        <path d="M 322 232 C 452 256, 506 200, 548 176" class="lathandoff" marker-end="url(#latarr)"/>
        <text x="430" y="270" text-anchor="middle" class="latflow">localize, then hand off a scoped obligation</text>
        ${Object.keys(N).map(node).join("")}
        <text x="600" y="202" text-anchor="middle" class="latfv">hevm · SMTChecker · Lean · Certora</text>
      </svg>
      <div class="eqread" data-idle="${idle}">${idle}</div>
      <p class="twnote">The usual picture is a line from syntactic to semantic. With riffcat the structural side is a <b>lattice</b>: you forget names, or constants, or types, independently, and a finer agreement always implies a coarser one. <b>Semantic equivalence is a different axis</b> (two structurally different programs can compute the same thing, which is what an optimizer does daily), undecidable in general. riffcat lives entirely on the structural lattice; it <b>localizes</b> a candidate at a chosen facet and hands a verifier a tight, scoped proof obligation. The facet it anchors at is exactly the scope that obligation stays sound in.</p>`;
    const read = this.querySelector(".eqread");
    this.querySelectorAll(".latnode").forEach((g) =>
      g.addEventListener("mouseenter", () => {
        this.querySelectorAll(".latnode").forEach((x) => x.classList.remove("on"));
        g.classList.add("on");
        const n = LATTICE_NODES[g.dataset.k];
        read.innerHTML = `<b>${n.t}</b> · ${n.s}`;
      }));
    this.addEventListener("mouseleave", () => {
      this.querySelectorAll(".latnode").forEach((x) => x.classList.remove("on"));
      read.innerHTML = read.dataset.idle;
    });
  }
});

// The empty verdict seat. riffcat localizes a candidate (a shape match at a
// chosen facet) and emits a proof obligation; the claim-ledger row's verdict
// and attested-by columns are an EMPTY SEAT, awaiting a verifier. This view is
// pure narrative + a static ledger (no engine call): it renders synchronously.
// Selecting a candidate row shows the obligation riffcat hands off and the seat
// that stays open; selecting an adjudicator names what that tool would discharge
// and how (a proof, or a counterexample). riffcat never fills the seat itself.
const SEAT_DISCLAIMER =
  "riffcat does not prove equivalence, verify safety, or decide exploitability. "
  + "It localizes a candidate and states the obligation. The seat stays empty until a named tool fills it.";

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
    obligation: "for each pair sharing this anchor, prove behavioural equivalence, or return an input where they differ",
  },
  {
    key: "vuln",
    subj: "ERC-4626 convertToShares (inflation shape)",
    from: "the bug-shape sniff",
    facet: "structure",
    asserts: "the control- and data-flow shape of the first-depositor inflation pattern",
    open: "is this instance actually exploitable",
    footprint: "structure, constants (the guard threshold)",
    obligation: "decide whether an attacker input reaches the unguarded mint, given this contract's constants",
  },
  {
    key: "fuzzy",
    subj: "Multicall (edited fork)",
    from: "the modified forks",
    facet: "structure (weighted containment)",
    asserts: "enough of the known vulnerable subtree is still present to be a candidate",
    open: "is the surviving shape still the vulnerable one",
    footprint: "structure, resolved callees",
    obligation: "confirm the edit preserved the vulnerable path, or exhibit the input the edit now guards",
  },
];

// The seat's candidate fillers. Each is a real verifier; the note states, in the
// phrasebook voice, the verb it does (prove / refute / model-check) and the layer
// it works at. SMTChecker is the warmest door (it ships inside solc); the others
// are named honestly as the tools that COULD fill the seat, not endorsements.
const SEAT_ADJUDICATORS = [
  { id: "hevm",       name: "hevm",       does: "symbolic execution and equivalence checking over EVM bytecode; returns a proof or a concrete counterexample." },
  { id: "smtchecker", name: "SMTChecker", does: "ships inside solc; discharges assertions and reachability over the source. The closest door, because the obligation can travel with the compile." },
  { id: "kontrol",    name: "Kontrol",    does: "K-framework proofs over EVM semantics; the obligation becomes a claim it discharges or leaves open." },
  { id: "certora",    name: "Certora",    does: "specification-driven verification; the localized candidate becomes a rule to prove or a violation to surface." },
];

const SEAT_EMPTY = '<span class="seat-empty" title="awaiting a verifier">awaiting adjudication</span>';

customElements.define("verdict-seat", class extends HTMLElement {
  connectedCallback() {
    this.sel = 0;        // selected candidate row
    this.tool = null;    // selected adjudicator (null = seat still empty)
    this.render();
  }
  render() {
    const rows = SEAT_CANDIDATES.map((c, i) =>
      `<tr class="lxrow seatrow ${i === this.sel ? "on" : ""}" data-i="${i}">`
      + `<td class="lx-shape">${c.subj} <span class="vsub">${c.from}</span></td>`
      + `<td class="seat-asserts">${c.asserts}</td>`
      + `<td class="lx-fp">${c.facet}</td>`
      + `<td class="lx-verd seat-cell">${this.tool ? this.tool.name : SEAT_EMPTY}</td>`
      + `<td class="lx-verd seat-cell">${this.tool ? "<span class=\"seat-pending\">a proof or a counterexample</span>" : SEAT_EMPTY}</td></tr>`).join("");
    const chips = SEAT_ADJUDICATORS.map((a) =>
      `<button class="seat-tool ${this.tool && this.tool.id === a.id ? "on" : ""}" data-tool="${a.id}">${a.name}</button>`).join("");
    this.innerHTML = `
      <p class="vbug">One problem, three contributors: riffcat <b>localizes</b> the matching subtree, the compiler can supply the <b>provenance</b> down to bytecode, and a verifier <b>adjudicates</b>. The two columns on the right are the verifier's. They are empty by design.</p>
      <table class="vledger seat-ledger">
        <thead><tr><th>candidate (localized by riffcat)</th><th>what riffcat asserts</th><th>anchor facet</th><th>verdict</th><th>attested by</th></tr></thead>
        <tbody>${rows}</tbody>
      </table>
      <p class="vledger-cap">Click a candidate for the obligation riffcat hands off. The verdict is structural-not-semantic: riffcat says <em>same shape, and here are the parts that match</em>, and the sentence stops there.</p>
      <div class="seat-handoff codepanel"></div>
      <div class="seat-fill">
        <span class="seat-fill-label">who could fill the seat</span>
        ${chips}
        <button class="seat-tool seat-clear ${this.tool ? "" : "on"}" data-tool="">leave it open</button>
      </div>
      <div class="seat-disclaimer"><span class="seat-disc-mark">!</span><span>${SEAT_DISCLAIMER}</span></div>
      <p class="twnote">A facet address is an anchor. A verdict attaches to that anchor and rides every artifact sharing it, but only when the anchor is at least as fine as everything the verdict depends on (its <b>footprint</b>). That is why the obligation carries its facet: it is the exact scope a future proof stays sound in. riffcat measures what an anchor forgets; the verifier decides what is true within it.</p>`;
    this.querySelectorAll(".seatrow").forEach((row) =>
      row.addEventListener("click", () => {
        this.sel = +row.dataset.i;
        this.querySelectorAll(".seatrow").forEach((x, i) => x.classList.toggle("on", i === this.sel));
        this.renderHandoff();
      }));
    this.querySelectorAll("[data-tool]").forEach((btn) =>
      btn.addEventListener("click", () => {
        const id = btn.dataset.tool;
        this.tool = id ? SEAT_ADJUDICATORS.find((a) => a.id === id) : null;
        this.render(); // re-render so the ledger's verdict/attested columns reflect the seat
      }));
    this.renderHandoff();
  }
  renderHandoff() {
    const c = SEAT_CANDIDATES[this.sel];
    const seatLine = this.tool
      ? `<span class="seat-named"><b>${this.tool.name}</b> takes the seat</span>: ${this.tool.does}`
      : `<span class="seat-open">the seat is empty</span>: riffcat states the obligation and stops. No tool above has taken it.`;
    this.querySelector(".seat-handoff").innerHTML =
      `<div class="cphead seat-head"><b>${c.subj}</b> <span class="vsub">localized at ${c.facet}</span></div>`
      + `<div class="seat-ob">`
      + `<div class="seat-ob-row"><span class="seat-tag">riffcat asserts</span><span class="seat-val">${c.asserts}</span></div>`
      + `<div class="seat-ob-row"><span class="seat-tag">open question</span><span class="seat-val open">${c.open}</span></div>`
      + `<div class="seat-ob-row"><span class="seat-tag">footprint</span><span class="seat-val">${c.footprint} <span class="vsub">(the dimensions the answer depends on; the anchor must cover them)</span></span></div>`
      + `<div class="seat-ob-row"><span class="seat-tag">proof obligation</span><span class="seat-val ob">${c.obligation}</span></div>`
      + `</div>`
      + `<div class="seat-resolve">${seatLine}</div>`;
  }
});

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
  { key: "note_set", label: "note set", drops: "keeps the exact pitch classes" },
  { key: "set_class", label: "set class", drops: "forgets transposition and inversion" },
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
    key: "is3-11", foot: 1,
    label: "set class is Forte 3-11",
    say: "interval vector [0 0 1 1 1 0], the major/minor triad family",
    // true exactly of the chords whose prime form is the 3-11 class [0,3,7]
    holds: (d) => JSON.stringify(d.prime_form) === JSON.stringify([0, 3, 7]),
  },
  {
    key: "evenness", foot: 1,
    label: "no interval class is empty",
    say: "a structural fact about the set class: every interval class appears at least once",
    holds: (d) => d.interval_vector.every((x) => x > 0),
  },
  {
    key: "rootC", foot: 0,
    label: "contains the pitch class C",
    say: "depends on the literal notes, not just the transposition class",
    holds: (d) => d.pitch_classes.includes(0),
  },
  {
    key: "hasE", foot: 0,
    label: "contains the pitch class E",
    say: "again a literal-notes fact: which pitch classes are present",
    holds: (d) => d.pitch_classes.includes(4),
  },
];

// A short static catalog that carries the same shape into the code/FV domain, so
// the formal point lands even though the live engine here speaks chords. Each
// entry names a fact, the dimensions its truth depends on (its footprint), and
// the home facet that footprint defines. This is prose-as-data, not a live
// computation; it mirrors section 3 of the proof note.
const ANC_FV = [
  { fact: "matches the ERC-4626 inflation vuln shape", foot: "structure", home: "structure", note: "a control-and-data-flow shape, so it rides the structural anchor and reaches every structural twin: triage, not a verdict." },
  { fact: "is OpenZeppelin mulDiv, identifiers aside", foot: "structure, names", home: "names-blind", note: "an identification modulo names; it rides names-blind, not bare structure." },
  { fact: "the overflow guard holds at this threshold", foot: "structure, constants", home: "keeps constants", note: "depends on a literal, so transporting it along a constants-blind anchor would be unsound." },
];

const ancShort = (h) => (h || "").slice(0, 8);

customElements.define("anchor-transport", class extends HTMLElement {
  async connectedCallback() {
    this.anchor = 1;   // index into ANC_FACETS; start coarse (set_class) to show the family
    this.factKey = "is3-11";
    this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
    try {
      const b = await engineReady;
      // Fingerprint each chord once; we read its facet addresses + the raw
      // pitch data the facts inspect straight off the binding's full result.
      this.data = ANC_CHORDS
        .map((c) => { try { return { c, fp: JSON.parse(b.fingerprint_chord(c)) }; } catch { return null; } })
        .filter(Boolean);
      this.render();
    } catch (e) { this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`; }
  }
  render() {
    const facet = ANC_FACETS[this.anchor];
    const fact = ANC_FACTS.find((f) => f.key === this.factKey);

    // The anchor address each chord computes at the chosen facet. Chords that
    // share an address share an anchor; a pinned fact rides exactly those.
    const addrOf = (fp) => fp[facet.key];

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

    const ladder = ANC_FACETS.map((f, i) =>
      `<span class="stop ${i === this.anchor ? "on" : ""}" data-anchor="${i}" title="${f.drops}">${f.label}</span>`).join("");

    const factPills = ANC_FACTS.map((f) =>
      `<button class="ancfact ${f.key === this.factKey ? "on" : ""}" data-fact="${f.key}">`
      + `<span class="ancfoot foot-${f.foot}">needs ${ANC_FACETS[f.foot].label}</span>${f.label}</button>`).join("");

    // One row per chord: its notes, its address at the anchor, and a transport
    // state. A rider whose fact holds is a SOUND landing; a rider whose fact
    // fails is a WRONG landing (only possible when cover fails); a non-rider is
    // simply out of scope.
    const rows = this.data.map((d) => {
      const a = addrOf(d.fp);
      const isRider = a === anchorAddr;
      const ok = fact.holds(d.fp);
      const state = !isRider ? "out" : ok ? "sound" : "wrong";
      const notes = d.fp.pitch_classes.map((pc) => `<span class="nchip">${NOTE_NAMES[pc]}</span>`).join("");
      const tag = state === "sound" ? "fact rides here"
        : state === "wrong" ? "fact would ride here, but is false"
        : "different anchor";
      const subjMark = d === subject ? `<span class="ancpin" title="the fact is pinned here">pinned</span>` : "";
      return `<div class="ancrow anc-${state}">`
        + `<span class="shapedot" style="--chip:${chipColor(a)}" title="${facet.label} ${ancShort(a)}"></span>`
        + `<span class="riffname">${d.c}${subjMark}</span>`
        + `<span class="nchips">${notes}</span>`
        + `<span class="ancaddr">${ancShort(a)}</span>`
        + `<span class="anctag">${tag}</span></div>`;
    }).join("");

    const soundLine = covers
      ? `<span class="anc-ok">cover holds</span>: the anchor (<b>${facet.label}</b>) is at least as fine as the fact's footprint (<b>${ANC_FACETS[fact.foot].label}</b>), so every rider is a sound landing.`
      : `<span class="anc-bad">cover fails</span>: the anchor (<b>${facet.label}</b>) forgot a dimension the fact depends on (its footprint is <b>${ANC_FACETS[fact.foot].label}</b>), so the fact would ride to <b>${wrong.length}</b> shape${wrong.length === 1 ? "" : "s"} where it is not true: ${wrong.map((d) => d.c).join(", ") || "none in this set"}.`;

    const idle = `fact <b>${fact.label}</b> pinned on <b>${subject.c}</b> at the <b>${facet.label}</b> anchor`
      + ` · rides to <b>${riders.length}</b> shape${riders.length === 1 ? "" : "s"} sharing that address`
      + (covers ? "" : ` · <b>${wrong.length}</b> of them wrongly`);

    this.innerHTML = `
      <div class="ancfacts">
        <div class="anclabel">pin a fact</div>
        ${factPills}
        <p class="ancsay">${fact.say}. Its footprint is <b>${ANC_FACETS[fact.foot].label}</b>: the coarsest anchor it can ride soundly.</p>
      </div>
      <div class="dialbar">
        <div class="grp"><span>anchor facet</span><div class="ladder">${ladder}</div></div>
        <span class="ancslidehint">slide right to forget more</span>
      </div>
      <div class="ancrows">${rows}</div>
      <div class="anccover ${covers ? "ok" : "bad"}">${soundLine}</div>
      <div class="eqread" data-idle="${idle}">${idle}</div>
      <p class="twnote">The mechanism is the claims layer: a fact keys on a facet address, and sharing that address is decidable by lookup, so transport is free and instant. Soundness is one condition, and it is provable: <b>a fact rides an anchor only if the anchor forgets at least as much as the fact does</b>. The facet's own invariance theorem measures what it forgets; the transport theorem lets a fact ride exactly when its footprint avoids that forgotten set. Slide the anchor below its home facet and the fact does not get more general, it gets wrong, which is also how you read off what the fact truly depends on.</p>
      <div class="ancfv">
        <div class="anclabel">the same condition, on code (stated, not computed here)</div>
        ${ANC_FV.map((e) => `<div class="ancfvrow"><span class="ancfvfact">${e.fact}</span>`
          + `<span class="ancfvhome">home facet: <b>${e.home}</b></span>`
          + `<span class="ancfvnote">${e.note}</span></div>`).join("")}
        <p class="twnote">We can prove the transport theorem and check the cover condition. We cannot prove a human's footprint is declared correctly; that residual trust sits with the auditor. riffcat is the anchor a proof attaches to, and it can state precisely the scope a transported proof stays sound in. It narrows what to read; it does not decide what is true.</p>
      </div>`;

    this.querySelectorAll("[data-anchor]").forEach((s) =>
      s.addEventListener("click", () => { const i = +s.dataset.anchor; if (this.anchor !== i) { this.anchor = i; this.render(); } }));
    this.querySelectorAll("[data-fact]").forEach((b) =>
      b.addEventListener("click", () => { if (this.factKey !== b.dataset.fact) { this.factKey = b.dataset.fact; this.render(); } }));
  }
});

// "prior art" chapter: content-addressed structural identity, independently
// reinvented across three communities. Pure copy, no engine: renders
// synchronously. Three cards on a lineage timeline (Forte 1973 -> Yatima/Ix ->
// Verity), each with one honest sentence; hovering a card writes its precise
// same/different-vs-riffcat line into a readout that restores an idle line on
// leave (the facet-lattice hover idiom). A closing note concedes the primitive
// and draws the line at our actual slice: the faceted, explainable query.
const PRIORART_CARDS = [
  {
    k: "forte",
    era: "1973 · music theory",
    who: "Allen Forte",
    what: "pitch-class set theory",
    line: "Prime form is a canonical address for a chord, invariant under transposition and inversion; the Forte number is that address into a catalog of shapes.",
    accent: "var(--warm)",
    read: "Same move: normalize a structure (a set of notes), then address it. A facet by another name, settled in the 1970s. You saw the engine land on this catalog two chapters back, on real chords.",
  },
  {
    k: "lurk",
    era: "ongoing · FV / Lean",
    who: "Lurk · Yatima · Ix",
    what: "content-addressed Lean",
    line: "A nameless, De-Bruijn kernel IR, Merkle-hashed to a content address independent of computationally-irrelevant naming, so a proof of typechecking can travel with the artifact.",
    accent: "var(--cool)",
    read: "Same primitive, our names-blind facet frozen as the only setting. Theirs is an exact-identity oracle: two terms share an address or they are unrelated. No near-match, no distance, by design.",
  },
  {
    k: "verity",
    era: "ongoing · FV / EVM",
    who: "Verity (LFG Labs)",
    what: "a verified Lean compiler for the EVM",
    line: "A formally verified compiler from an embedded DSL to EVM bytecode, proven to preserve semantics across its supported fragment, with the trust boundary named in the open.",
    accent: "var(--a)",
    read: "The other axis: meaning. Verity proves one contract correct against its spec. riffcat points at which of the millions of deployed contracts share its shape, so a proof like that knows where to aim. Complement, not competitor.",
  },
];
const PRIORART_IDLE = "Three independent arrivals at one idea: normalize, then content-address. Hover a card for how it sits beside riffcat. The shared primitive is theirs; what follows is ours.";
customElements.define("prior-art", class extends HTMLElement {
  connectedCallback() {
    const cards = PRIORART_CARDS.map((c) =>
      `<div class="pacard" data-k="${c.k}" style="--pa:${c.accent}">`
      + `<div class="pa-era">${c.era}</div>`
      + `<div class="pa-who">${c.who}</div>`
      + `<div class="pa-what">${c.what}</div>`
      + `<p class="pa-line">${c.line}</p></div>`).join("");
    this.innerHTML = `
      <div class="paline" aria-hidden="true"></div>
      <div class="pagrid">${cards}</div>
      <div class="eqread" data-idle="${PRIORART_IDLE}">${PRIORART_IDLE}</div>
      <p class="twnote">The point is not that we are first. It is that the primitive (content-addressed, names-blind structural hashing) is established, well, by people whose judgement we trust, and that this is reassuring rather than awkward. So we do not claim the hash. riffcat's slice is turning a structural digest from an exact-identity oracle into a <b>faceted, graded, explainable query</b> over a large, adversarial corpus: a ladder of normalizations, per-dimension digests, twins and near-match, for recognition and triage. Same primitive; opposite question. They ask whether this is the exact same object. We ask how much, and in which dimensions, this resembles that, across millions of independently compiled contracts. We can show you the why-they-match. We do not claim the why-they-mean-the-same.</p>`;
    const read = this.querySelector(".eqread");
    this.querySelectorAll(".pacard").forEach((card) => {
      const c = PRIORART_CARDS.find((x) => x.k === card.dataset.k);
      card.addEventListener("mouseenter", () => {
        this.querySelectorAll(".pacard").forEach((x) => x.classList.toggle("dim", x !== card));
        card.classList.add("on");
        read.innerHTML = `<span class="pa-sw" style="background:${c.accent}"></span><b>${c.who}</b> · ${c.read}`;
      });
    });
    this.addEventListener("mouseleave", () => {
      this.querySelectorAll(".pacard").forEach((x) => x.classList.remove("dim", "on"));
      read.innerHTML = read.dataset.idle;
    });
  }
});

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
    { name: "primeForm_correct", says: "prime form is the unique representative of the transpose-and-invert class" },
    { name: "primeForm_idempotent", says: "normalizing a normal form changes nothing" },
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
  { k: "checked", t: "kernel-checked", s: "the Lean 4 kernel verified these theorems; lake build passing means checked, not merely tested. Assumption-free finite combinatorics over the twelve pitch classes." },
  { k: "assumption", t: "named assumption", s: "no two distinct shapes collide in the content address: a stated cryptographic assumption on the digest, not a theorem. Named, not hidden." },
  { k: "open", t: "open gap", s: "every theorem is about the Lean model of the encoder; the running Rust engine agrees by golden vectors and reading, not yet by verified extraction. We state this as integrity, up front." },
];

const ATTEST_SHORT = (h) => (h || "").slice(0, 12);

customElements.define("verified-attest", class extends HTMLElement {
  async connectedCallback() {
    this.rec = ATTEST_RECORD;
    // Future path: if the engine ever exposes a content-addressed claim register
    // and a lake status, fill the badge from it. Both are await-safe to miss.
    try {
      const b = await engineReady;
      if (b && typeof b.attestation === "function") {
        const live = JSON.parse(b.attestation(
          ATTEST_RECORD.property, ATTEST_RECORD.subjectFacet, ATTEST_RECORD.footprintFacet));
        // expected shape: { claim, subjectAddr, build, checked: bool }
        if (live && live.claim) {
          this.rec = Object.assign({}, ATTEST_RECORD, live);
        }
      }
    } catch (_) { /* baked placeholder stands; this is a static-attested view */ }
    this.live = !!(window.wasmBindings && typeof window.wasmBindings.attestation === "function");
    this.render();
  }
  render() {
    const r = this.rec;
    const checkClass = this.live ? "ok" : "static";
    const checkText = this.live ? "registered live" : "attested (static placeholder)";
    const theorems = r.theorems.map((th) =>
      `<div class="vattest-thm" data-read="thm" data-name="${th.name}" data-says="${th.says}">`
      + `<span class="vattest-tick">checks</span><code>${th.name}</code></div>`).join("");
    const tiers = ATTEST_TIERS.map((tier) =>
      `<span class="vattest-tier vattest-${tier.k}" data-read="tier" data-key="${tier.k}">${tier.t}</span>`).join("");
    const idle = `<b>${r.theorems.length}</b> theorems kernel-checked · property <code>${r.property}</code>`
      + ` anchored at the <b>${r.footprintFacet}</b> facet · the claim rides every voicing and transposition sharing that anchor, and no finer`;
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
      if (kind === "thm") return `<b>${el.dataset.name}</b> · ${el.dataset.says} · kernel-checked, assumption-free`;
      if (kind === "seal") return `<b>content address</b> · the digest of the checked proof; sharing this address is what lets the claim transport without re-proving`;
      if (kind === "claim") return `<b>the claim shape</b> · a property keyed onto a subject facet address, with a recorded footprint facet; transport is sound only when the anchor facet covers the footprint`;
      if (kind === "tier") {
        const tier = ATTEST_TIERS.find((x) => x.k === el.dataset.key);
        return `<b>${tier.t}</b> · ${tier.s}`;
      }
      if (kind === "tiers") return `three tiers, kept apart on purpose: <b>checked</b> by the kernel, an honestly <b>named assumption</b>, and the <b>open gap</b> between the Lean model and the Rust engine`;
      return idleHTML;
    };
    this.querySelectorAll("[data-read]").forEach((el) =>
      el.addEventListener("mouseenter", () => { read.innerHTML = describe(el); }));
    this.addEventListener("mouseleave", () => { read.innerHTML = idleHTML; });
  }
});

// The Forte-catalog chapter: a small catalog of named chords, each parsed from
// real notation and fingerprinted live (fingerprint_chord), shown with its
// prime form, interval vector, and Forte-style set-class label. At the
// set_class facet the chords group exactly the way Allen Forte's catalog does:
// major and minor triads collapse to 3-11, the dominant and minor sevenths
// collapse to 4-27, while augmented (3-12) and diminished (3-10) stand alone.
// The point of the page: riffcat lands on the published catalog from structure
// alone, the same dial the code chapters use.
//
// Forte numbers are a fixed, published fact about each prime form (Forte 1973),
// not something the engine emits, so they live here as a tiny lookup keyed by
// the engine's own prime_form output, the same way the lattice chapter carries
// its node copy in JS. Everything that moves (pitch classes, prime form,
// interval vector, the set-class address that drives grouping and color) comes
// straight from fingerprint_chord.

// The catalog, in notation the vibe-grammars parser already reads. Each entry is
// (notation, the name a musician would say). The engine does the rest.
const FORTE_CHORDS = [
  ["C", "major triad"],
  ["Am", "minor triad"],
  ["Caug", "augmented triad"],
  ["Bdim", "diminished triad"],
  ["Cmaj7", "major seventh"],
  ["G7", "dominant seventh"],
  ["Am7", "minor seventh"],
];

// Two facets, loosest last: the literal note set, then Forte's set class (the
// prime form, invariant under transposition and inversion). The page opens on
// set_class because that is where the catalog appears.
const FORTE_FACETS = [["note_set", "note set"], ["set_class", "set class"]];

// Published Forte numbers, keyed by prime form (the engine's prime_form joined
// by spaces). Only the classes this catalog can produce; an unknown prime form
// just shows no label rather than a wrong one. Each carries Forte's own
// interval-vector spelling for a quiet cross-check against the live one.
const FORTE_NUMBERS = {
  "0 3 7": { id: "3-11", iv: "<001110>", gloss: "major and minor triads, one class" },
  "0 4 8": { id: "3-12", iv: "<000300>", gloss: "augmented triad, all major thirds" },
  "0 3 6": { id: "3-10", iv: "<002001>", gloss: "diminished triad, the symmetric one" },
  "0 1 5 8": { id: "4-20", iv: "<101220>", gloss: "major seventh" },
  "0 2 5 8": { id: "4-27", iv: "<012111>", gloss: "dominant and minor sevenths, one class" },
};

// Forte's angle-bracket spelling of an interval vector (the six interval-class
// counts), so the live count and the catalog count sit side by side.
const forteIvText = (iv) => "<" + iv.map((n) => (n > 9 ? "X" : String(n))).join("") + ">";

customElements.define("forte-catalog", class extends HTMLElement {
  async connectedCallback() {
    this.facet = "set_class";
    this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
    try {
      const b = await engineReady;
      this.data = FORTE_CHORDS
        .map(([notation, name]) => {
          try { return { notation, name, fp: JSON.parse(b.fingerprint_chord(notation)) }; }
          catch { return null; }
        })
        .filter(Boolean);
      this.render();
    } catch (e) {
      this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`;
    }
  }
  // The Forte label for a chord, looked up by the engine's prime form.
  forteFor(d) { return FORTE_NUMBERS[d.fp.prime_form.join(" ")] || null; }
  render() {
    const ladder = FORTE_FACETS.map(([k, label]) =>
      `<span class="stop ${k === this.facet ? "on" : ""}" data-facet="${k}">${label}</span>`).join("");

    // Group by the live set-class (or note-set) address: this is the same
    // grouping the engine computes, not a re-derivation in JS.
    const groups = new Map(); // address -> [names]
    for (const d of this.data) {
      const a = d.fp[this.facet];
      if (!groups.has(a)) groups.set(a, []);
      groups.get(a).push(d.name);
    }
    const onClass = this.facet === "set_class";

    const cards = this.data.map((d, i) => {
      const a = d.fp[this.facet];
      const col = chipColor(a);
      const notes = d.fp.pitch_classes.map((pc) => `<span class="nchip">${NOTE_NAMES[pc]}</span>`).join("");
      const f = this.forteFor(d);
      const forteTag = onClass && f
        ? `<span class="fc-num">${f.id}</span>` : "";
      const prime = onClass
        ? `<span class="fc-pf">prime [${d.fp.prime_form.join(" ")}]</span>` : "";
      // interval vector: the live count, with Forte's published spelling
      // alongside it when on the set-class facet, so the two visibly agree.
      const ivLive = forteIvText(d.fp.interval_vector);
      const iv = onClass
        ? `<span class="fc-iv">iv ${ivLive}${f ? ` <em>= Forte ${f.iv}</em>` : ""}</span>`
        : `<span class="fc-iv">iv ${ivLive}</span>`;
      return `<div class="fcatcard fcat-${a.slice(0, 12)}" data-eq="fcat-${a.slice(0, 12)}" style="--chip:${col}">`
        + `<div class="fcat-h">`
        + `<button class="playbtn" data-i="${i}">▶</button>`
        + `<span class="shapedot" style="--chip:${col}" title="${this.facet} ${a.slice(0, 10)}"></span>`
        + `<span class="fcat-name"><b>${d.notation}</b> <span class="vsub">${d.name}</span></span>`
        + `${forteTag}</div>`
        + `<div class="fcat-notes"><span class="nchips">${notes}</span></div>`
        + `<div class="fcat-meta">${prime}${iv}</div></div>`;
    }).join("");

    const n = groups.size;
    const gtxt = [...groups.values()]
      .map((names) => names.length > 1 ? `<b>${names.join(" = ")}</b>` : names[0]).join(" · ");
    const idle = onClass
      ? `<b>${n}</b> set classes across ${this.data.length} chords · ${gtxt} · live, in your browser`
      : `<b>${n}</b> note sets across ${this.data.length} chords · each chord its own notes`;

    const note = onClass
      ? `At the <b>set-class</b> facet the seven chords fall into the classes Allen Forte catalogued in 1973. Major and minor triads share one address (<b>3-11</b>), the dominant and minor sevenths share another (<b>4-27</b>, they are inversions of one set), while the augmented (<b>3-12</b>) and diminished (<b>3-10</b>) triads stand alone. riffcat is not given the catalog: the prime form is computed from the structure, then content-addressed by the same facet machinery the code chapters use (<code>fingerprint_chord</code>). The interval vectors it counts match Forte's published ones, line for line.`
      : `At the <b>note-set</b> facet each chord is just its set of pitches, so every one of these seven is its own address. Loosen the dial to <b>set class</b> and structure starts to fold the catalog together.`;

    this.innerHTML = `
      <div class="dialbar"><div class="grp"><span>facet</span><div class="ladder">${ladder}</div></div></div>
      <div class="fcatgrid">${cards}</div>
      <div class="eqread" data-idle="${idle}">${idle}</div>
      <p class="twnote">${note}</p>`;

    this.querySelectorAll("[data-facet]").forEach((s) =>
      s.addEventListener("click", () => { if (this.facet !== s.dataset.facet) { this.facet = s.dataset.facet; this.render(); } }));
    this.querySelectorAll(".playbtn").forEach((btn) =>
      btn.addEventListener("click", () => playChord(this.data[+btn.dataset.i].fp.pitch_classes)));

    // Hover a card to light every chord in the same class and name them in the
    // readout: the collapse made legible, exactly the facet-primer gesture.
    this.querySelectorAll(".fcatcard").forEach((card) => {
      card.addEventListener("mouseenter", () => {
        const kin = this.querySelectorAll("." + card.dataset.eq);
        this.querySelectorAll(".fcatcard").forEach((x) => x.classList.add("dim"));
        kin.forEach((x) => { x.classList.remove("dim"); x.classList.add("lit"); });
        const read = this.querySelector(".eqread");
        if (!read) return;
        const names = [...kin].map((x) => x.querySelector(".fcat-name b").textContent.trim());
        read.innerHTML = kin.length > 1
          ? `<b>${kin.length}</b> share this ${onClass ? "set class" : "note set"}: <b>${names.join(" = ")}</b>`
          : `<b>${names[0]}</b> is alone at the ${onClass ? "set-class" : "note-set"} facet`;
      });
      card.addEventListener("mouseleave", () => {
        this.querySelectorAll(".fcatcard").forEach((x) => x.classList.remove("dim", "lit"));
        const read = this.querySelector(".eqread");
        if (read) read.innerHTML = read.dataset.idle || "";
      });
    });
  }
});

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

customElements.define("ivf-fingerprint", class extends HTMLElement {
  async connectedCallback() {
    this.sel = 0;
    this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
    try {
      const b = await engineReady;
      this.data = IVF_CHORDS
        .map((c) => { try { return { c, fp: JSON.parse(b.fingerprint_chord(c)) }; } catch { return null; } })
        .filter(Boolean);
      if (!this.data.length) throw new Error("no chord parsed");
      this.render();
    } catch (e) { this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`; }
  }
  // Tiny bar strip for one interval vector. The widest count present sets the
  // full-height bar, so a vector reads as a shape at a glance; an empty class is
  // a faint floor, never a gap. Bars are colored from the vector's own class
  // color, so two chords with the same signature also wear the same color.
  strip(vec, color, big) {
    const peak = Math.max(1, ...vec);
    const cls = big ? "ivf-strip ivf-big" : "ivf-strip";
    const bars = vec.map((n, i) => {
      const h = Math.round(12 + (big ? 40 : 22) * (n / peak));
      const on = n > 0 ? "on" : "off";
      return `<span class="ivf-col" title="${IVF_IC[i][2]}: ${n}">`
        + `<span class="ivf-bar ${on}" style="height:${h}px;--ivc:${color}"></span>`
        + `<span class="ivf-n">${n}</span>`
        + (big ? `<span class="ivf-lab">${IVF_IC[i][1]}</span>` : "")
        + `</span>`;
    }).join("");
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
    const rows = this.data.map((d, i) => {
      const vec = d.fp.interval_vector;
      const color = chipColor(d.fp.set_class);
      const notes = d.fp.pitch_classes.map((pc) => `<span class="nchip">${NOTE_NAMES[pc]}</span>`).join("");
      return `<div class="riffrow ivf-row ${i === this.sel ? "on" : ""}" data-i="${i}">`
        + `<button class="playbtn" data-i="${i}">▶ play</button>`
        + `<span class="riffname">${d.c}</span>`
        + `<span class="nchips">${notes}</span>`
        + this.strip(vec, color, false)
        + `<span class="ivf-vec"><${"" }${vec.join("")}></span>`
        + `<span class="shapedot" style="--chip:${color}" title="interval vector &lt;${vec.join("")}&gt;"></span></div>`;
    }).join("");
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
    const detail = `<div class="ivf-detail">`
      + `<div class="ivf-dh"><b>${sel.c}</b> · interval vector <span class="ivf-bra">&lt;${svec.join(" ")}&gt;</span>`
      + ` <span class="vsub">${total} interval${total === 1 ? "" : "s"} in all</span></div>`
      + this.strip(svec, selColor, true)
      + `<div class="ivf-share">${shareLine}</div></div>`;
    const n = groups.size;
    const gtxt = [...groups.values()]
      .map((cs) => cs.length > 1 ? `<b>${cs.join(" = ")}</b>` : cs[0]).join(" · ");
    this.innerHTML = `
      <div class="dialbar"><div class="grp"><span>chord</span><div class="ladder">`
      + this.data.map((d, i) => `<span class="stop ${i === this.sel ? "on" : ""}" data-i="${i}">${d.c}</span>`).join("")
      + `</div></div></div>
      <div class="riffs">${rows}</div>
      ${detail}
      <div class="eqread"><b>${n}</b> distinct signature${n === 1 ? "" : "s"} among ${this.data.length} chords · ${gtxt}</div>
      <p class="twnote">The interval vector counts the unordered intervals inside a chord, one bin per interval class from the minor second up to the tritone. It keeps no order, no octave, no root, so it is a <b>transposition- and inversion-invariant</b> harmonic signature: shift the chord to any key or turn it upside down and the six numbers hold. That is why <b>C major and A minor</b> share one vector here (both are set class 3-11). It is a projection, honestly partial: it can tell you two chords have the same interval content, not that they are the same chord. The engine reads it straight off the structure (<code>fingerprint_chord</code>).</p>`;
    this.querySelectorAll(".stop[data-i]").forEach((s) =>
      s.addEventListener("click", () => this.pick(+s.dataset.i)));
    this.querySelectorAll(".ivf-row").forEach((r) =>
      r.addEventListener("click", (e) => { if (!e.target.closest(".playbtn")) this.pick(+r.dataset.i); }));
    this.querySelectorAll(".playbtn").forEach((btn) =>
      btn.addEventListener("click", (e) => { e.stopPropagation(); playChord(this.data[+btn.dataset.i].fp.pitch_classes); }));
  }
  pick(i) { if (i !== this.sel && this.data[i]) { this.sel = i; this.render(); } }
});

// The three rungs of the structural ladder on a pitch-class set, on one chord.
// Each rung is a stricter facet: note_set (the literal pitch classes) -> the
// transposition normal form (the same set rotated to its minimal, tightest-packed
// reading, transposition forgotten) -> set_class / prime form (inversion folded in
// too). We climb one chord up the ladder and name what each rung drops.
//
// The middle rung reads a field the engine does not expose yet: a transposition-
// normal-form address from fingerprint_chord, matching polyphonotopes-math's
// normalFormBits (the minimal-rotation, transposition-invariant canonical form
// that sits between the literal set and the inversion-folded prime form). The
// component is written against that future field, with a graceful fallback note
// when it is absent. See engine_needs.
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
    forgets: "where the set sits: every transposition reads as one tightest-packed rotation",
    keeps: "the inside spacing of the chord, still distinguishing a shape from its mirror",
  },
  {
    key: "set_class",
    rung: "inversion forgotten too",
    forgets: "the mirror as well: a shape and its inversion share one prime form",
    keeps: "only the interval content: Allen Forte's catalog address",
  },
];
// A chord whose three rungs are all visibly distinct, so the ladder reads as a
// climb and not a collapse. G major works: written as {D,G,B} the literal set is
// not its own tightest rotation (rung 1 != rung 2), and a major triad is not
// inversion-symmetric, so the prime form folds it further (rung 2 != rung 3). Its
// prime form is [0,3,7], the 3-11 class the set-class chapters land on.
const TR_CHORD = "G";
// Render the chosen rung's address as a small set of pitch-class chips, using the
// engine's own field for that rung. The literal set always shows; the normal-form
// rung shows the rotated reading; the set-class rung shows the prime form.
const trReading = (fp, key) => {
  if (key === "set_class") return fp.prime_form;
  if (key === "transposition_normal" && fp.transposition_normal_form)
    return fp.transposition_normal_form;
  return fp.pitch_classes;
};
const trPcLabel = (pc) => ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"][((pc % 12) + 12) % 12];
customElements.define("three-rungs", class extends HTMLElement {
  async connectedCallback() {
    this.rung = "note_set";
    this.innerHTML = `<p class="live-note">booting the wasm engine…</p>`;
    try {
      const b = await engineReady;
      this.fp = JSON.parse(b.fingerprint_chord(TR_CHORD));
      this.hasNormal = this.fp.transposition_normal != null
        && this.fp.transposition_normal_form != null;
      this.render();
    } catch (e) { this.innerHTML = `<p class="live-note bad">engine error: ${e}</p>`; }
  }
  addrFor(key) {
    if (key === "set_class") return this.fp.set_class;
    if (key === "transposition_normal") return this.fp.transposition_normal || this.fp.set_class;
    return this.fp.note_set;
  }
  render() {
    const fp = this.fp;
    const ladder = TR_RUNGS.map((r) =>
      `<span class="stop ${r.key === this.rung ? "on" : ""}" data-rung="${r.key}">${r.rung}</span>`).join("");
    // The chord climbs the same ladder; each rung is a row, lit when selected,
    // showing its address-color dot and the set reading the engine returns there.
    const rows = TR_RUNGS.map((r, i) => {
      const addr = this.addrFor(r.key);
      const reading = trReading(fp, r.key);
      const chips = reading.map((pc) => `<span class="nchip">${trPcLabel(pc)}</span>`).join("");
      const on = r.key === this.rung;
      const missing = r.key === "transposition_normal" && !this.hasNormal;
      return `<div class="rungrow ${on ? "on" : ""} ${missing ? "pending" : ""}" data-rung="${r.key}">`
        + `<span class="rung-ix">${i + 1}</span>`
        + `<span class="rung-name">${r.rung}</span>`
        + `<span class="nchips">${chips}</span>`
        + `<button class="playbtn" data-i="${i}">▶ hear it</button>`
        + `<span class="shapedot" style="--chip:${chipColor(addr)}" title="${r.key} ${addr.slice(0, 10)}"></span></div>`;
    }).join("");
    const sel = TR_RUNGS.find((r) => r.key === this.rung);
    const pendNote = (this.rung === "transposition_normal" && !this.hasNormal)
      ? ` <span class="rung-pending">(this rung is awaiting an engine field; showing the literal set as a placeholder)</span>` : "";
    const idle = `rung <b>${TR_RUNGS.indexOf(sel) + 1}</b> of 3 · forgets ${sel.forgets} · keeps ${sel.keeps}${pendNote}`;
    this.innerHTML = `
      <div class="dialbar"><div class="grp"><span>climb</span><div class="ladder">${ladder}</div></div></div>
      <div class="riffs rungs">${rows}</div>
      <div class="eqread" data-idle="${idle}">${idle}</div>
      <p class="twnote">One chord (<b>${TR_CHORD}</b>), three readings of "the same." The first rung is the literal set of pitch classes. The second slides the set to its tightest-packed rotation, so every transposition reads alike: the same move a listener makes hearing a riff moved up a fifth as still the riff. The third folds in inversion as well and lands on the prime form, Forte's catalog address. Each step up forgets exactly one more thing, and never adds anything back: equal at a lower rung is always equal at every rung above it.</p>`;
    this.querySelectorAll("[data-rung]").forEach((el) =>
      el.addEventListener("click", () => {
        const k = el.dataset.rung;
        if (k && this.rung !== k) { this.rung = k; this.render(); }
      }));
    this.querySelectorAll(".playbtn").forEach((btn) =>
      btn.addEventListener("click", (e) => {
        e.stopPropagation();
        const r = TR_RUNGS[+btn.dataset.i];
        playChord(trReading(this.fp, r.key));
      }));
  }
});

// "two fingerprints" chapter: the compiler's metadata hash (byte-exact, flips on
// any character change) set beside riffcat's structural fingerprint (built to
// survive the edits that move the metadata hash but keep the shape). This is an
// illustrative compare, not a live recompile: the metadata-hash stand-in is a
// real digest over the exact source text (so it genuinely flips on a whitespace),
// and the structural fingerprint is shown as the shape it would resolve to and
// holds across the same edits. The live structural compute lives in the
// "drive the dial" chapter; here the point is the contrast of the two axes.
//
// Source quote (Sourcify, "Finding Auxdatas in the Bytecode", 2024-02-12): the
// metadata hash "acts as a fingerprint of the compilation ... the slightest
// change in the compiler settings or even a whitespace in any of the source
// files will cause a change in the metadata hash." Kaan (#1659): "the metadata
// hash is somehow the fingerprint of the compilation."

// One baseline function and three edits that each move the metadata hash. The
// structural fingerprint is deliberately insensitive to all three: whitespace is
// not part of the shape, a rename drops out at names-blind, a constant churn
// drops out once constants are set aside. Each edit returns the edited source so
// the metadata-hash stand-in can be recomputed over the literal text.
const MHA_BASE =
`function previewMint(uint256 shares) public view returns (uint256) {
    return _convertToAssets(shares, Math.Rounding.Up);
}`;
const MHA_EDITS = [
  { key: "none", label: "original", note: "the verified source, as compiled", apply: (s) => s },
  {
    key: "ws", label: "+ a whitespace", note: "one blank line added; nothing else touched",
    apply: (s) => s.replace("public view", "public  view"),
  },
  {
    key: "rename", label: "rename a parameter", note: "shares becomes amount throughout",
    apply: (s) => s.replace(/shares/g, "amount"),
  },
  {
    key: "const", label: "change a constant", note: "rounding direction flipped Up to Down",
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
  return (h.toString(16).padStart(8, "0") + g.toString(16).padStart(8, "0")).slice(0, 12);
}

// The structural fingerprint is the SAME across every edit here, because none of
// the three edits change the shape at names-and-constants-blind. A fixed digest
// over the structure stands in for it; it is what the live engine resolves this
// function to at the structure facet, and it does not move below.
const MHA_STRUCT = "9b2f04c7d1ae";

customElements.define("metadata-axes", class extends HTMLElement {
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

    const tabs = MHA_EDITS.map((e, i) =>
      `<button class="mha-tab ${i === this.sel ? "on" : ""}" data-i="${i}">${e.label}</button>`).join("");

    // two digest cards, side by side: the metadata hash (flips) and the
    // structural fingerprint (holds). The verdict word under each is the honest
    // claim, no more.
    const metaCard =
      `<div class="mha-card ${metaMoved ? "moved" : ""}">
        <div class="mha-card-h">metadata hash<span class="mha-axis">Sourcify · byte-exact</span></div>
        <div class="mha-fp mha-meta">${meta}</div>
        <div class="mha-verd ${metaMoved ? "flip" : "hold"}">${metaMoved ? "flipped" : "unchanged"}</div>
        <div class="mha-q">answers: is this the identical compilation</div>
      </div>`;
    const structCard =
      `<div class="mha-card ${structMoved ? "moved" : ""}">
        <div class="mha-card-h">structural fingerprint<span class="mha-axis">riffcat · shape-exact</span></div>
        <div class="mha-fp mha-struct">${MHA_STRUCT}</div>
        <div class="mha-verd ${structMoved ? "flip" : "hold"}">${structMoved ? "flipped" : "held"}</div>
        <div class="mha-q">answers: is this the same code, names and constants aside</div>
      </div>`;

    const read = this.sel === 0
      ? `the unedited source · both fingerprints agree here, because nothing has changed yet`
      : metaMoved && !structMoved
        ? `<b>${cur.note}</b> · the metadata hash <span class="mha-w-flip">flipped</span>, the structural fingerprint <span class="mha-w-hold">held</span>`
        : `${cur.note}`;

    this.innerHTML = `
      <div class="mha-quote">
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
      <div class="eqread" data-idle="${read}">${read}</div>
      <p class="twnote">Two axes, not two rivals. The metadata hash is the exact-identity fingerprint, and its sensitivity is the point: it gives a cryptographic guarantee that the whole compilation, whitespace included, is the original. The structural fingerprint is the deliberately insensitive counterpart: it dials out the names, constants, and formatting, so it survives the edits that move the metadata hash but keep the shape. One answers &ldquo;is this the identical build,&rdquo; the other &ldquo;is this the same construction in different clothes.&rdquo; riffcat rides the recompilation Sourcify already does; it does not replace the hash, it sits beside it.</p>
      <p class="twnote mha-honest">This panel is an illustration: the right-hand digest is a stand-in computed over the literal text, so it flips on any character the way the real metadata hash does, and the structural fingerprint is shown as the shape the engine resolves this function to. The structural fingerprint is computed live, on real code, in the &ldquo;drive the dial&rdquo; chapter.</p>`;

    this.querySelectorAll(".mha-tab").forEach((b) =>
      b.addEventListener("click", () => {
        const i = +b.dataset.i;
        if (i === this.sel) return;
        this.sel = i;
        this.render();
      }));
  }
});

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
  main: { t: "Main", s: "the code itself: the bytes a similarity search actually wants to compare." },
  immutable: { t: "Meta · immutable", s: "immutable variable values, patched into the runtime bytecode at deploy. A transformation, not the code's shape." },
  library: { t: "Meta · library", s: "library addresses linked at deploy. Same shape, different bytes, depending on where the library landed." },
  auxdata: { t: "Meta · auxdata", s: "CBOR metadata hash appended by the compiler. It moves on a whitespace change, and it can sit more than once in the strip." },
  constructor: { t: "Meta · constructor", s: "constructor arguments trailing the deployed bytes. Deploy-specific, not part of the code's shape." },
};

// The source side: the four dimensions riffcat reads off the AST, already
// separate by construction. These are exactly the dimensions the dial turns
// (the earlier chapters: full, names-blind, structure), shown here as columns so
// the contrast with the run-on strip is literal.
const BW_DIMS = [
  { k: "structure", t: "structure", s: "the shape of the syntax tree: control flow, the calls, how it is built. This is what survives the dial all the way down." },
  { k: "names", t: "names", s: "identifiers and labels. Read off the AST as their own dimension, so dropping them is one setting of the dial, not a guess." },
  { k: "constants", t: "constants", s: "literal values. Their own dimension too, which is why a constant change can move identity without touching structure." },
  { k: "types", t: "types", s: "declared types. Separated by construction at the source level, where a byte strip has long since flattened them away." },
];

customElements.define("byte-wall", class extends HTMLElement {
  connectedCallback() {
    // Geometry. Left column: the opaque onchain strip (Main/Meta interleaved).
    // Right column: the source dimensions, each its own clean band.
    const stripX = 36, stripY = 92, stripW = 300, stripH = 30;
    const total = BW_STRIP.reduce((a, c) => a + c.w, 0);
    let cx = stripX;
    const cells = BW_STRIP.map((c, i) => {
      const w = (c.w / total) * stripW;
      const x = cx; cx += w;
      const cls = c.kind === "main" ? "bw-main" : "bw-meta bw-" + c.kind;
      return `<rect class="bw-cell ${cls}" data-k="${c.kind}" x="${x.toFixed(1)}" y="${stripY}"`
        + ` width="${(w - 1).toFixed(1)}" height="${stripH}" rx="2"/>`;
    }).join("");

    const dimX = 432, dimW = 252, dimH = 30, dimGap = 12, dimY0 = 56;
    const dims = BW_DIMS.map((d, i) => {
      const y = dimY0 + i * (dimH + dimGap);
      return `<g class="bw-dim" data-k="${d.k}" transform="translate(${dimX},${y})">`
        + `<rect class="bw-dimbox" x="0" y="0" width="${dimW}" height="${dimH}" rx="4"/>`
        + `<text class="bw-dimt" x="12" y="${dimH / 2 + 4}">${d.t}</text></g>`;
    }).join("");

    const idle = "Hover the strip or a source dimension. On the left, Main (code) and Meta (the transformations) interleave, with no clean line between them. On the right, the source already separates them.";

    this.innerHTML = `
      <blockquote class="say bw-say">we do not know which parts of the onchain bytecode is Main vs Meta &middot; find potential similar bytecodes, ignore the Meta parts
        <span class="bw-cite">Kaan Uzdogan, Sourcify, on the similarity work (argotorg/sourcify #1643, 2024)</span>
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
      <p class="twnote">Sourcify's similarity search already strips the trailing auxdata before comparing bytecode, which is the right instinct. The wall is that the other Meta, immutables, libraries, and constructor arguments, does not sit in one clean trailing block: it is patched <b>through</b> the code as deploy-time transformations, so a byte match has to guess where Main ends and Meta begins. riffcat steps one level up. On the source AST the dimensions are separate by construction: <b>structure</b> is its own thing, and <b>names</b>, <b>constants</b>, and <b>types</b> are each their own dimension you can keep or drop. That is the same dial from the earlier chapters, and it is why dropping names is a setting here rather than a guess. It rides the recompilation Sourcify already does; it does not redo the byte split.</p>`;

    const read = this.querySelector(".eqread");
    const say = (t, s) => { read.innerHTML = `<b>${t}</b> &middot; ${s}`; };
    this.querySelectorAll(".bw-cell").forEach((cell) =>
      cell.addEventListener("mouseenter", () => {
        const k = cell.dataset.k;
        this.querySelectorAll(".bw-cell").forEach((x) => x.classList.toggle("bw-lit", x.dataset.k === k));
        const L = BW_LEGEND[k];
        say(L.t, L.s);
      }));
    this.querySelectorAll(".bw-dim").forEach((g) =>
      g.addEventListener("mouseenter", () => {
        this.querySelectorAll(".bw-dim").forEach((x) => x.classList.remove("on"));
        g.classList.add("on");
        const d = BW_DIMS.find((x) => x.k === g.dataset.k);
        say(d.t, d.s);
      }));
    this.addEventListener("mouseleave", () => {
      this.querySelectorAll(".bw-cell").forEach((x) => x.classList.remove("bw-lit"));
      this.querySelectorAll(".bw-dim").forEach((x) => x.classList.remove("on"));
      read.innerHTML = read.dataset.idle;
    });
  }
});

// "A shared building block" chapter. Static, synchronous, no engine: this is
// mostly copy. It places riffcat as a library-plus-thin-CLI offered for
// co-design, with the fe lineage as a background existence-proof ("we wanted X,
// so Y mattered"), and the collaboration triangle (localize / provenance /
// adjudicate) as a hand-laid SVG in the spirit of facet-lattice: hover a leg for
// who owns it. Solidity-facing throughout; fe stays brief and demystifying.
// Unique tag + SB_-prefixed module names so nothing collides with app.js.

// The triangle: one problem, three contributors, none can solve it alone. Each
// leg names what it does, who owns it, and (honestly) whether riffcat fills it
// today. Coordinates are hand-laid to sit in the same 720x320 frame the lattice
// uses, so the two diagrams read as siblings.
const SB_LEGS = {
  localize: {
    x: 360, y: 70, role: "riffcat",
    t: "localize",
    s: "Point at the exact subtree that matches or changed. Explainable, content-addressed. This is the leg riffcat does, and it stops there.",
    owns: "riffcat (this)", state: "built",
  },
  provenance: {
    x: 150, y: 250, role: "the compiler",
    t: "provenance",
    s: "Which source, and which Yul, became which bytecode. Only the compiler team can emit it. riffcat has a slot wired for it (trace_events) and today nothing fills it.",
    owns: "solc / a compiler", state: "empty slot",
  },
  adjudicate: {
    x: 570, y: 250, role: "a verifier",
    t: "adjudicate",
    s: "Prove the two really compute the same thing, or return a counterexample. A verifier consumes a localized candidate as a scoped proof obligation. The verdict is theirs, not riffcat's.",
    owns: "hevm / SMTChecker / Certora", state: "builds on top",
  },
};
const SB_EDGES = [["localize", "provenance"], ["provenance", "adjudicate"], ["adjudicate", "localize"]];

// The library surface, plainly: what is a stable library boundary today, and
// what is the thin CLI shell over it. Honest about which calls are real.
const SB_SURFACE = [
  { k: "fingerprint", d: "digest a graph at a chosen facet (full, names-blind, structure). The same call the live chapters made in your browser.", lib: true },
  { k: "recognize", d: "look a shape up against a catalog and get back what it is, with the subtrees that matched.", lib: true },
  { k: "similar", d: "weighted containment over a shape's subtrees, for the forks that edited the function. Direction stated, leaf floor reported.", lib: true },
  { k: "the CLI", d: "a thin shell over the library: ingest, query by shape, diff. The library is the building block; the CLI is one way to hold it.", lib: false },
];

customElements.define("shared-block", class extends HTMLElement {
  connectedCallback() {
    this.render();
  }
  render() {
    const N = SB_LEGS;
    const edges = SB_EDGES.map(([a, b]) =>
      `<line x1="${N[a].x}" y1="${N[a].y}" x2="${N[b].x}" y2="${N[b].y}" class="sbedge"/>`).join("");
    const node = (k) => {
      const n = N[k];
      return `<g class="sbnode sb-${k}" data-k="${k}" transform="translate(${n.x},${n.y})">`
        + `<rect x="-78" y="-20" width="156" height="40" rx="6"/>`
        + `<text class="sb-t" x="0" y="-1" text-anchor="middle">${n.t}</text>`
        + `<text class="sb-r" x="0" y="13" text-anchor="middle">${n.role}</text></g>`;
    };
    const surface = SB_SURFACE.map((s) =>
      `<div class="sbrow"><span class="sb-k">riffcat ${s.k}</span>`
      + `<span class="sb-tag ${s.lib ? "lib" : "cli"}">${s.lib ? "library" : "thin CLI"}</span>`
      + `<span class="sb-d">${s.d}</span></div>`).join("");
    const idle = "Hover a leg. One problem, three contributors, none of them closes the loop alone.";
    this.innerHTML = `
      <svg viewBox="0 0 720 320" class="sbtri" role="img" aria-label="the collaboration triangle: localize, provenance, adjudicate">
        <text x="360" y="26" text-anchor="middle" class="sbcap">one problem, three legs</text>
        ${edges}
        ${Object.keys(N).map(node).join("")}
      </svg>
      <div class="eqread" data-idle="${idle}">${idle}</div>
      <div class="sbsurface">${surface}</div>
      <p class="twnote">On the fe team we wanted to know exactly where each piece of compiled output came from, so the provenance tracking mattered, and riffcat grew out of that. fe is young, which helped: we could build that producer side from the start, the slot a mature compiler would have to retrofit. So fe is not the thing we are asking you to adopt. It is the proof that the producer side can be built, which is why the schema here is real and not a sketch.</p>
      <p class="twnote sbclose">It rides the corpus, it does not rebuild it. This is a draft passed around the table, not a contract handed across it. The honest question is the close: it could plausibly help several of these projects, so what fields and shapes does <b>your</b> data model actually need.</p>`;
    const read = this.querySelector(".eqread");
    this.querySelectorAll(".sbnode").forEach((g) =>
      g.addEventListener("mouseenter", () => {
        this.querySelectorAll(".sbnode").forEach((x) => x.classList.remove("on"));
        g.classList.add("on");
        const n = SB_LEGS[g.dataset.k];
        read.innerHTML = `<b>${n.t}</b> · owned by ${n.owns} · <span class="sb-state">${n.state}</span><br>${n.s}`;
      }));
    this.addEventListener("mouseleave", () => {
      this.querySelectorAll(".sbnode").forEach((x) => x.classList.remove("on"));
      read.innerHTML = read.dataset.idle;
    });
  }
});

// "What we would need help with": the localize + provenance + adjudicate
// triangle. riffcat does one leg (localize); the compiler supplies the missing
// provenance; a verifier adjudicates. Sourcify holds the pain in the middle.
// Mostly copy with a small hand-laid SVG triangle: hover a leg to surface the
// honest per-team ask, each ending on a question that team owns. No engine
// needed, so this renders synchronously. Models its SVG + hover + .eqread
// readout on facet-lattice; reuses .twnote, .legend, and the theme vars.
// Unique tag + CT_-prefixed module names to avoid colliding with app.js.
const CT_LEGS = {
  localize: {
    x: 250, y: 60, t: "riffcat · localize", who: "the leg we do",
    s: "Points at the matching or changed subtree, on source and on Yul, content-addressed so the match is a receipt you can read, not a score to trust.",
    ask: "This is the one leg we run today. The honest question is whether the thing we hand off, a localized candidate at a chosen facet, is the right shape for you to receive.",
    q: "What would make a localized candidate worth picking up?",
  },
  provenance: {
    x: 70, y: 300, t: "the compiler · provenance", who: "the empty slot",
    s: "Which source, and which Yul, became which bytecode. riffcat has a slot built for exactly this (the trace_events dimension and origin edges) and today nothing fills it.",
    ask: "Reaching a contract that was never verified needs source and Yul traced down to bytecode. That is genuinely hard, and you are the only ones who can emit it. fe could build the slot from the start; solc would retrofit it.",
    q: "What would it take to fill that slot, and is Yul the right layer?",
  },
  adjudicate: {
    x: 430, y: 300, t: "a verifier · adjudicate", who: "the leg above us",
    s: "Proves the two really are equivalent, or returns a counterexample. riffcat shows the why-they-match; it does not adjudicate the why-they-mean-the-same.",
    ask: "We localize a candidate and scope it to a facet. A verifier turns that into a proof obligation and answers it with a why. The facet we anchor at is exactly the scope the obligation stays sound in.",
    q: "Where is your line between worth-proving and noise?",
  },
};
const CT_PAIN = "Sourcify holds the pain: match onchain bytecode to verified source. None of the three legs closes that loop alone.";
// The honest list from the posture doc, each beat ending on a question the named
// team owns. Offer, never prescribe.
const CT_NEEDS = [
  { who: "Sourcify, and all the Argot projects",
    body: "If this is a shared building block, the fingerprint, facet, and origin schema has to fit more than riffcat. We have a candidate already near-isomorphic across fe and riffcat.",
    q: "What fields does your data model actually need?" },
  { who: "The solc team (fe as the existence proof)",
    body: "Recognizing a known shape in an unverified contract needs source and Yul to bytecode provenance. That is the empty trace_events slot, wired and waiting, with no plug yet made for it.",
    q: "What would that provenance cost, and is Yul the right place to fill it?" },
  { who: "The verification teams",
    body: "riffcat narrows the haystack; a person or a prover still checks each needle. A localized, facet-scoped candidate is meant to read as a tight proof obligation, not a verdict.",
    q: "What would you need from a candidate to treat its output as one?" },
  { who: "Sourcify and r0qs, for ground truth",
    body: "Precision and recall need labelled data. You hold the corpus and the provenance tags.",
    q: "Could we co-build the eval slice?" },
  { who: "Roadmap owners and auditors",
    body: "The roadmap names vulnerability patterns. Which ones first is your domain knowledge, not ours.",
    q: "Which shapes are worth catching first?" },
];
// Open-ended starters, the kind you end a beat on, not a feature pitch.
const CT_STARTERS = [
  "Query the corpus by shape instead of by address. What is the first question you would ask it?",
  "We are not asking you to adopt a finished thing. We are offering a working core and asking what it should become for you.",
];
customElements.define("collab-triangle", class extends HTMLElement {
  connectedCallback() {
    const L = CT_LEGS;
    const edge = (a, b) =>
      `<line x1="${L[a].x}" y1="${L[a].y}" x2="${L[b].x}" y2="${L[b].y}" class="ctedge"/>`;
    const node = (k) => {
      const n = L[k];
      return `<g class="ctleg ${k}" data-k="${k}" transform="translate(${n.x},${n.y})">`
        + `<rect x="-92" y="-17" width="184" height="34" rx="6"/>`
        + `<text x="0" y="4" text-anchor="middle">${n.t}</text></g>`;
    };
    const idle = "Hover a leg. Each one is a different team's piece; none of them closes the loop alone.";
    const needs = CT_NEEDS.map((n) =>
      `<div class="ctneed"><div class="ctneed-who">${n.who}</div>`
      + `<div class="ctneed-body">${n.body}</div>`
      + `<div class="ctneed-q">${n.q}</div></div>`).join("");
    const starters = CT_STARTERS.map((s) => `<li>${s}</li>`).join("");
    this.innerHTML = `
      <svg viewBox="-30 0 560 360" class="cttri" role="img" aria-label="the localize, provenance, adjudicate triangle">
        ${edge("localize", "provenance")}${edge("provenance", "adjudicate")}${edge("adjudicate", "localize")}
        <text x="250" y="200" text-anchor="middle" class="ctpain">Sourcify</text>
        <text x="250" y="220" text-anchor="middle" class="ctpain sub">holds the pain</text>
        ${Object.keys(L).map(node).join("")}
      </svg>
      <div class="eqread" data-idle="${idle}">${idle}</div>
      <p class="twnote">${CT_PAIN} riffcat rides the corpus, it does not rebuild it. It narrows what to read; it does not decide what is true.</p>
      <div class="kicker" style="margin-top:24px">what we would actually need, and who owns the answer</div>
      <div class="ctneeds">${needs}</div>
      <div class="kicker" style="margin-top:22px">two questions to start on</div>
      <ul class="ctstart">${starters}</ul>`;
    const read = this.querySelector(".eqread");
    this.querySelectorAll(".ctleg").forEach((g) =>
      g.addEventListener("mouseenter", () => {
        this.querySelectorAll(".ctleg").forEach((x) => x.classList.remove("on"));
        g.classList.add("on");
        const n = CT_LEGS[g.dataset.k];
        read.innerHTML = `<span class="ctwho">${n.who}</span> <b>${n.t}</b> · ${n.s}`
          + `<div class="ctask">${n.ask}</div>`
          + `<div class="ctq">${n.q}</div>`;
      }));
    this.addEventListener("mouseleave", () => {
      this.querySelectorAll(".ctleg").forEach((x) => x.classList.remove("on"));
      read.innerHTML = read.dataset.idle;
    });
  }
});
