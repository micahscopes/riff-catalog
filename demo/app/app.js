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
        return `<span class="sw" style="background:hsl(${digestHue(c.dataset.eq.slice(3))} 70% 55%)"></span>`
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
      const d = this.byName[f.name].facets[this.facet], hue = digestHue(d);
      return `<div class="fcard ${feq(d)}" data-eq="${feq(d)}" style="--hue:${hue}">`
        + `<div class="fcard-h"><span class="sw" style="background:hsl(${hue} 70% 55%)"></span>${f.name}</div>`
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
        const hue = f.lib && LIB[f.lib] ? `style="--hue:${LIB[f.lib].h}"` : "";
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
