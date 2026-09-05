// Interactive, real-world tours of riff-cat's graph hashing. JavaScript owns
// the explanation only. Every digest and SCC comes from the Rust core in wasm.

const engineReady = window.wasmBindings
  ? Promise.resolve(window.wasmBindings)
  : new Promise((resolve) =>
      addEventListener(
        "TrunkApplicationStarted",
        () => resolve(window.wasmBindings),
        { once: true },
      ),
    );

const DIMS = ["structure", "names", "constants", "types"];
const STEPS = {
  order: [
    ["The order", "Start with the data a shop already has."],
    ["Make a graph", "Nodes hold fields. Ordered child edges say what contains what."],
    ["Choose a contract", "The policy fixes how bytes become hashes."],
    ["Hash each node", "A local hash sees one node only."],
    ["Fold the leaves", "Leaves have no children, so they can finish first."],
    ["Fold the line items", "Each line item commits to its product child."],
    ["Fold the order", "The root commits to all three ordered branches."],
    ["Hash the graph", "The final graph record commits to every finished node."],
    ["Make an address", "A facet selects dimensions and binds them to this policy."],
  ],
  edit: [
    ["Change one field", "Paper filters changes from quantity 1 to quantity 2."],
    ["Local hash moves", "Only the edited node has different local content."],
    ["Its branch moves", "The line item folds in the edited local hash."],
    ["The root moves", "The order folds in that changed branch."],
    ["Other branches stay", "Unrelated subtrees keep their exact hashes."],
    ["Graph hash moves", "The graph record now contains a changed final node digest."],
    ["Facets decide", "Facets that omit constants stay stable. Facets that keep them move."],
  ],
  cycle: [
    ["The service map", "Most calls flow downward, but two services depend on each other."],
    ["The tree fold stops", "Neither member of the cycle can finish before the other."],
    ["Find the SCC", "Tarjan groups the mutually reachable services."],
    ["Hash each service", "Local hashes still need no neighbors."],
    ["Seed member colors", "Each member starts from its local hash and outgoing cross-component content."],
    ["Refine colors", "Internal neighbors are mixed in until the colors stabilize."],
    ["Hash the component", "The whole cycle now has one stable component digest."],
    ["Fold the component DAG", "Components can be folded because the quotient graph is acyclic."],
    ["Give nodes context", "Each node combines local content, final color, and component fold."],
    ["Hash the graph", "Finished node digests and flat edges enter graph.full."],
    ["Make an address", "Selected graph dimensions become a comparable facet address."],
  ],
  cfg: [
    ["The Yul loop", "Start with a loop that loads and sums several memory words."],
    ["The backedge stops the fold", "Increment jumps back to the loop test, so leaf-first recursion cannot finish."],
    ["Tarjan isolates the loop", "Only the four mutually reachable loop blocks enter the SCC."],
    ["Hash each node", "Blocks and instruction subtrees get context-free local hashes the same way."],
    ["Seed block colors", "Each block starts from local content and outgoing cross-component content."],
    ["Refine loop positions", "Internal successor and predecessor colors distinguish positions in the loop."],
    ["Condense the loop", "Outside it is one node; inside, the four blocks keep their CFG edges."],
    ["Fold the component DAG", "The LOOP SCC commits to separately addressed instruction subtrees."],
    ["Restore node context", "Each node combines local content, its final color, and its component fold."],
    ["Hash the whole function", "Every final node digest and graph edge enters graph.full."],
    ["Make an address", "The function gets a comparable facet address without losing the SCC boundary."],
  ],
};

let uid = 0;
const short = (hex) => (hex ? hex.slice(0, 10) : "not yet");
const esc = (value) =>
  String(value == null ? "" : value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
const fieldValue = (value) =>
  value && typeof value === "object" && "value" in value
    ? String(value.value)
    : String(value);

customElements.define(
  "hash-walkthrough",
  class extends HTMLElement {
    async connectedCallback() {
      this.uid = ++uid;
      this.scenario = this.getAttribute("scenario") || "order";
      this.step = 0;
      this.dimension = this.scenario === "edit" ? "constants" : "structure";
      this.selected =
        this.scenario === "cycle"
          ? "inventory"
          : this.scenario === "cfg"
            ? "loop_header"
            : "order";
      this.playTimer = null;
      this.innerHTML = `<p class="hw-loading">starting the Rust hashing engine...</p>`;

      try {
        const bindings = await engineReady;
        const specs = window.RIFFCAT_HASH_WALKS;
        if (!specs || typeof bindings.hash_walkthrough !== "function")
          throw new Error("the hashing walkthrough export is unavailable");

        if (this.scenario === "order") {
          this.spec = specs.order;
          this.result = this.hash(bindings, this.spec, "reject");
        } else if (this.scenario === "edit") {
          this.spec = specs.editedOrder;
          this.before = this.hash(bindings, specs.order, "reject");
          this.result = this.hash(bindings, specs.editedOrder, "reject");
        } else {
          this.spec = this.scenario === "cfg" ? specs.cfg : specs.services;
          try {
            this.hash(bindings, this.spec, "reject");
          } catch (error) {
            this.rejectError = this.errorText(error);
          }
          this.result = this.hash(bindings, this.spec, "condense_scc");
          if (this.scenario === "cfg") {
            const withoutBackedge = JSON.parse(JSON.stringify(this.spec));
            withoutBackedge.edges = withoutBackedge.edges.filter(
              (edge) => edge[1] !== "backedge",
            );
            this.withoutBackedge = this.hash(
              bindings,
              withoutBackedge,
              "condense_scc",
            );
          }
        }
        this.render();
      } catch (error) {
        this.innerHTML = `<p class="hw-error"><b>Could not run the walkthrough.</b><br>${esc(this.errorText(error))}</p>`;
      }
    }

    disconnectedCallback() {
      this.stopPlaying();
    }

    hash(bindings, spec, policy) {
      return JSON.parse(
        bindings.hash_walkthrough(JSON.stringify(spec), policy),
      );
    }

    errorText(error) {
      if (error && error.message) return error.message;
      return String(error).replace(/^RuntimeError:\s*/, "");
    }

    steps() {
      return STEPS[this.scenario];
    }

    setStep(step) {
      const next = Math.max(0, Math.min(step, this.steps().length - 1));
      if (next === this.step) return;
      this.step = next;
      const picks = {
        order: [
          "order",
          "order",
          "order",
          "order",
          "buyer",
          "filters_line",
          "order",
          "order",
          "order",
        ],
        edit: [
          "filters_line",
          "filters_line",
          "filters_line",
          "order",
          "beans_line",
          "order",
          "order",
        ],
        cycle: [
          "inventory",
          "inventory",
          "inventory",
          "inventory",
          "inventory",
          "catalog",
          "inventory",
          "inventory",
          "inventory",
          "checkout",
          "checkout",
        ],
        cfg: [
          "loop_header",
          "increment",
          "loop_header",
          "test_insn",
          "loop_header",
          "increment",
          "loop_header",
          "test_insn",
          "loop_header",
          "return",
          "loop_header",
        ],
      };
      this.selected = picks[this.scenario][next];
      this.render();
    }

    nextStep() {
      if (!this.result || this.step >= this.steps().length - 1) return false;
      this.setStep(this.step + 1);
      return true;
    }

    previousStep() {
      if (!this.result || this.step <= 0) return false;
      this.setStep(this.step - 1);
      return true;
    }

    stopPlaying() {
      if (this.playTimer) clearInterval(this.playTimer);
      this.playTimer = null;
    }

    togglePlay() {
      if (this.playTimer) {
        this.stopPlaying();
        this.render();
        return;
      }
      if (this.step === this.steps().length - 1) this.setStep(0);
      this.playTimer = setInterval(() => {
        if (!this.nextStep()) {
          this.stopPlaying();
          this.render();
        }
      }, 1400);
      this.render();
    }

    node(id, result = this.result) {
      return result.nodes.find((node) => node.id === id);
    }

    specNode(id) {
      return this.spec.nodes.find((node) => node.id === id);
    }

    componentFor(id) {
      return this.result.components.find((component) =>
        component.members.some((member) => member.id === id),
      );
    }

    cycleMembers() {
      const component = this.result.components.find(
        (candidate) => candidate.members.length > 1,
      );
      return new Set(
        (component ? component.members : []).map((member) => member.id),
      );
    }

    isCyclic() {
      return this.scenario === "cycle" || this.scenario === "cfg";
    }

    sccLabel() {
      if (this.scenario === "cfg")
        return `SCC: ${this.cycleMembers().size} CFG blocks`;
      return `SCC: ${[...this.cycleMembers()]
        .map((id) => this.specNode(id).label)
        .join(" + ")}`;
    }

    sccStyle() {
      const nodes = [...this.cycleMembers()].map((id) => this.specNode(id));
      const xs = nodes.map((node) => node.x);
      const ys = nodes.map((node) => node.y);
      const left = Math.max(1, Math.min(...xs) - 13);
      const top = Math.max(5, Math.min(...ys) - 7);
      const right = Math.min(99, Math.max(...xs) + 13);
      const bottom = Math.min(98, Math.max(...ys) + 7);
      return `left:${left}%;top:${top}%;width:${right - left}%;height:${bottom - top}%`;
    }

    orderHashKind(id) {
      if (this.step < 3) return null;
      if (this.step === 3) return "local";
      const leaf = ["buyer", "beans_product", "filters_product"].includes(id);
      const line = ["beans_line", "filters_line"].includes(id);
      if (this.step === 4 && leaf) return "tree";
      if (this.step === 5 && (leaf || line)) return "tree";
      return this.step >= 6 ? "tree" : "local";
    }

    cycleHashKind() {
      if (this.step < 3) return null;
      if (this.step < 5) return "local";
      if (this.step === 5) return "color";
      if (this.step < 8) return "component";
      return "context";
    }

    editHashKind(id) {
      if (this.step === 0) return null;
      if (this.step === 1) return "local";
      if (this.step === 2 && id === "filters_line") return "tree";
      if (this.step >= 3) return "tree";
      return "local";
    }

    hashKind(id) {
      if (this.scenario === "order") return this.orderHashKind(id);
      if (this.scenario === "edit") return this.editHashKind(id);
      return this.cycleHashKind(id);
    }

    nodeDigest(id, result = this.result) {
      const kind = this.hashKind(id);
      const node = this.node(id, result);
      if (!kind || !node) return null;
      if (kind === "color") {
        const component = this.componentFor(id);
        return component.members.find((member) => member.id === id).colors[
          this.dimension
        ];
      }
      if (kind === "component")
        return this.componentFor(id).digests[this.dimension];
      if (kind === "context") return node.tree[this.dimension];
      return node[kind][this.dimension];
    }

    nodeClass(id) {
      const classes = [];
      if (id === this.selected) classes.push("selected");
      if (this.nodeDigest(id)) classes.push("hashed");
      if (this.isCyclic() && this.cycleMembers().has(id))
        classes.push("scc");
      if (this.scenario === "cfg" && this.specNode(id).kind === "yulssa.insn")
        classes.push("payload");
      if (this.scenario === "edit") {
        const highlighted = {
          0: ["filters_line"],
          1: ["filters_line"],
          2: ["filters_line"],
          3: ["filters_line", "order"],
          4: ["beans_line", "beans_product", "buyer"],
          5: ["filters_line", "order"],
          6: [],
        }[this.step];
        if (highlighted.includes(id))
          classes.push(this.step === 4 ? "steady" : "changed");
      }
      return classes.join(" ");
    }

    edgePath(edge, edges) {
      const source = this.specNode(edge.source);
      const target = this.specNode(edge.target);
      if (edge.label === "backedge") {
        const cx = Math.min(source.x, target.x) - 38;
        const cy = (source.y + target.y) / 2;
        const qx = source.x * 0.25 + cx * 0.5 + target.x * 0.25;
        const qy = source.y * 0.25 + cy * 0.5 + target.y * 0.25;
        const c1x = (source.x + cx) / 2;
        const c1y = (source.y + cy) / 2;
        const c2x = (cx + target.x) / 2;
        const c2y = (cy + target.y) / 2;
        return `M ${source.x} ${source.y} Q ${c1x} ${c1y} ${qx} ${qy} Q ${c2x} ${c2y} ${target.x} ${target.y}`;
      }
      const reverse = edges.some(
        (other) => other.source === edge.target && other.target === edge.source,
      );
      if (!reverse) {
        const mx = (source.x + target.x) / 2;
        const my = (source.y + target.y) / 2;
        return `M ${source.x} ${source.y} L ${mx} ${my} L ${target.x} ${target.y}`;
      }
      const dx = target.x - source.x;
      const dy = target.y - source.y;
      // Reversing the edge reverses this perpendicular offset, so the two
      // directions land on opposite sides instead of painting over each other.
      const cx = (source.x + target.x) / 2 + dy * 0.5;
      const cy = (source.y + target.y) / 2 - dx * 0.5;
      const qx = source.x * 0.25 + cx * 0.5 + target.x * 0.25;
      const qy = source.y * 0.25 + cy * 0.5 + target.y * 0.25;
      const c1x = (source.x + cx) / 2;
      const c1y = (source.y + cy) / 2;
      const c2x = (cx + target.x) / 2;
      const c2y = (cy + target.y) / 2;
      return `M ${source.x} ${source.y} Q ${c1x} ${c1y} ${qx} ${qy} Q ${c2x} ${c2y} ${target.x} ${target.y}`;
    }

    condensedCfgHtml() {
      const loop = this.componentFor("loop_header");
      const nodeButton = (id, x, y, extra = "") => {
        const specNode = this.specNode(id);
        const digest = this.nodeDigest(id);
        const kind = this.hashKind(id);
        return `<button type="button" class="hw-node ${this.nodeClass(id)} ${extra}"
          style="left:${x}%;top:${y}%" data-node="${esc(id)}"
          aria-pressed="${id === this.selected}">
          <span>${esc(specNode.label)}</span>
          <code>${digest ? `${kind} ${short(digest)}` : this.nodeHint(id)}</code>
        </button>`;
      };
      const payloads = [
        ["test_insn", 18, 32, "Header"],
        ["load_insn", 39, 42, "A"],
        ["add_insn", 60, 52, "B"],
        ["inc_insn", 81, 62, "Latch"],
      ];
      const payloadEdges = payloads
        .map(
          ([, x, ownerX]) =>
            `<path d="M ${ownerX} 20 L ${x} 62 L ${x} 82" class="hw-edge subtree-edge" marker-mid="url(#hw-arrow-${this.uid})"/>`,
        )
        .join("");
      const payloadNodes = payloads
        .map(([id, x, , owner]) => {
          const specNode = this.specNode(id);
          const digest = this.nodeDigest(id);
          const kind = this.hashKind(id);
          return `<button type="button" class="hw-node ${this.nodeClass(id)} payload"
            style="left:${x}%;top:82%" data-node="${esc(id)}"
            aria-pressed="${id === this.selected}">
            <small>${esc(owner)} owns</small>
            <span>${esc(specNode.label)}</span>
            <code>${digest ? `${kind} ${short(digest)}` : this.nodeHint(id)}</code>
          </button>`;
        })
        .join("");
      const loopSelected = this.cycleMembers().has(this.selected);
      const graphShown = this.step >= 9;
      return `<div class="hw-canvas scenario-cfg condensed" aria-label="the loop SCC and its separately addressed payload subtrees">
        <svg class="hw-lines" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
          <defs><marker id="hw-arrow-${this.uid}" markerWidth="5" markerHeight="5" refX="4.5" refY="2.5" orient="auto"><path d="M0,0 L5,2.5 L0,5 z"/></marker></defs>
          <path d="M 15 20 L 31 20 L 47 20" class="hw-edge dependency" marker-mid="url(#hw-arrow-${this.uid})"/>
          <path d="M 47 20 L 62 20 L 78 20" class="hw-edge dependency" marker-mid="url(#hw-arrow-${this.uid})"/>
          <path d="M 78 20 L 78 32 L 78 44" class="hw-edge dependency" marker-mid="url(#hw-arrow-${this.uid})"/>
          ${payloadEdges}
        </svg>
        <div class="hw-layer-label condensed-flow">condensed control flow</div>
        <div class="hw-layer-label condensed-payload">owned payloads, separately addressed</div>
        ${nodeButton("entry", 15, 20)}
        <button type="button" class="hw-node hw-component-node hashed${loopSelected ? " selected" : ""}"
          style="left:47%;top:20%" data-node="loop_header" aria-pressed="${loopSelected}">
          <span>LOOP SCC</span>
          <small>Header → A → B → Latch ↺</small>
          <code>${short(loop.digests[this.dimension])}</code>
        </button>
        ${nodeButton("exit", 78, 20)}
        ${nodeButton("return", 78, 44)}
        ${payloadNodes}
        ${
          graphShown
            ? `<div class="hw-graph-digest"><span>graph.full</span><code>${short(this.result.graph[this.dimension])}</code></div>`
            : ""
        }
      </div>`;
    }

    graphHtml() {
      if (this.scenario === "cfg" && this.step >= 6)
        return this.condensedCfgHtml();
      const edges = [...this.result.children, ...this.result.edges];
      const paths = edges
        .map((edge) => {
          const source = this.specNode(edge.source);
          const target = this.specNode(edge.target);
          const reverse = edges.some(
            (other) =>
              other.source === edge.target && other.target === edge.source,
          );
          const dx = target.x - source.x;
          const dy = target.y - source.y;
          const backedge = edge.label === "backedge";
          const subtreeEdge = this.scenario === "cfg" && edge.role === "child";
          const cyclicEdge = reverse || backedge;
          const mx = backedge
            ? (source.x + target.x) / 2 - 19
            : (source.x + target.x) / 2 + (reverse ? dy * 0.27 : 0);
          const my = backedge
            ? (source.y + target.y) / 2
            : reverse
              ? (source.y + target.y) / 2 +
                (edge.source < edge.target ? -5 : 5)
              : (source.y + target.y) / 2 - 1.5;
          return (
            `<path d="${this.edgePath(edge, edges)}" class="hw-edge ${edge.role}${cyclicEdge ? " cycle-pair" : ""}${subtreeEdge ? " subtree-edge" : ""}"` +
            `${edge.role !== "child" || subtreeEdge ? ` marker-mid="url(#hw-${cyclicEdge ? "cycle-" : ""}arrow-${this.uid})"` : ""}/>` +
            (subtreeEdge
              ? ""
              : `<text x="${mx}" y="${my}" class="hw-edge-label${cyclicEdge ? " cycle-pair" : ""}">${esc(edge.label)}</text>`)
          );
        })
        .join("");
      const nodes = this.spec.nodes
        .map((specNode) => {
          const digest = this.nodeDigest(specNode.id);
          const kind = this.hashKind(specNode.id);
          return `<button type="button" class="hw-node ${this.nodeClass(specNode.id)}"
            style="left:${specNode.x}%;top:${specNode.y}%" data-node="${esc(specNode.id)}"
            aria-pressed="${specNode.id === this.selected}">
            <span>${esc(specNode.label)}</span>
            <code>${digest ? `${kind} ${short(digest)}` : this.nodeHint(specNode.id)}</code>
          </button>`;
        })
        .join("");
      const graphShown =
        (this.scenario === "order" && this.step >= 7) ||
        (this.scenario === "edit" && this.step >= 5) ||
        (this.isCyclic() && this.step >= 9);
      const scc =
        this.isCyclic() && this.step >= 2
          ? `<div class="hw-scc-ring" style="${this.sccStyle()}"><span>${esc(this.sccLabel())}</span></div>`
          : "";
      const layerLabel =
        this.scenario === "cfg"
          ? `<div class="hw-layer-label">one-way subtrees</div>`
          : "";
      return `<div class="hw-canvas scenario-${esc(this.scenario)}" aria-label="click a node to inspect its hash">
        <svg class="hw-lines" viewBox="0 0 100 100" preserveAspectRatio="none" aria-hidden="true">
          <defs><marker id="hw-arrow-${this.uid}" markerWidth="5" markerHeight="5" refX="4.5" refY="2.5" orient="auto"><path d="M0,0 L5,2.5 L0,5 z"/></marker></defs>
          <defs><marker id="hw-cycle-arrow-${this.uid}" class="hw-cycle-marker" markerWidth="5" markerHeight="5" refX="4.5" refY="2.5" orient="auto"><path d="M0,0 L5,2.5 L0,5 z"/></marker></defs>
          ${paths}
        </svg>
        ${scc}${layerLabel}${nodes}
        ${
          graphShown
            ? `<div class="hw-graph-digest"><span>graph.full</span><code>${short(this.result.graph[this.dimension])}</code></div>`
            : ""
        }
      </div>`;
    }

    nodeHint(id) {
      if (
        this.scenario === "edit" &&
        id === "filters_line" &&
        this.step === 0
      )
        return "quantity 1 -> 2";
      if (
        this.isCyclic() &&
        this.step === 1 &&
        this.cycleMembers().has(id)
      )
        return "waiting...";
      if (this.scenario === "cfg") {
        if (this.specNode(id).kind === "yulssa.insn")
          return "downstream subtree";
        if (this.cycleMembers().has(id)) return "CFG block";
        return "outside the SCC";
      }
      return "readable data";
    }

    localInputs(node) {
      if (this.dimension === "structure")
        return [`kind = ${node.kind}`, "0 structure fields"];
      const fields = node.fields.filter(
        (field) => field.dimension === this.dimension,
      );
      if (!fields.length) return [`0 ${this.dimension} fields`];
      return fields.map(
        (field) => `${field.name} = ${fieldValue(field.value)}`,
      );
    }

    treeInputs(node) {
      const parts = [`local ${short(node.local[this.dimension])}`];
      for (const child of node.children) {
        const childNode = this.node(child.target);
        const prefix =
          this.dimension === "structure"
            ? `${child.ordinal}:${child.label} `
            : "";
        parts.push(`${prefix}child ${short(childNode.tree[this.dimension])}`);
      }
      if (!node.children.length) parts.push("0 children");
      return parts;
    }

    digestBox(label, digest, tone = "") {
      return `<div class="hw-digest-box ${tone}"><span>${esc(label)}</span><code>${esc(digest || "not computed at this step")}</code></div>`;
    }

    detailOrder(node) {
      if (this.step === 0)
        return this.readableNode(
          node,
          "Nothing has been hashed yet. This is the source data.",
        );
      if (this.step === 1)
        return `<p>riff-cat receives <b>${this.result.nodes.length} nodes</b> and <b>${this.result.children.length} ordered child edges</b>. Display positions are not part of the graph.</p>${this.readableNode(node)}`;
      if (this.step === 2) return this.policyHtml();
      if (this.step === 7) return this.graphDetail();
      if (this.step === 8) return this.facetDetail();
      const kind = this.hashKind(node.id);
      const inputs =
        kind === "tree" ? this.treeInputs(node) : this.localInputs(node);
      const record = kind === "tree" ? "node.tree" : "node.local";
      return `<p><b>${record}</b> for ${esc(this.specNode(node.id).label)} in the <b>${this.dimension}</b> dimension.</p>
        ${this.inputList(inputs)}
        ${this.digestBox(`${record} digest`, node[kind][this.dimension])}`;
    }

    detailEdit(node) {
      const before = this.node(node.id, this.before);
      if (this.step === 0)
        return `<p>Only one input field changes.</p>
          <div class="hw-compare"><span>before <b>quantity = 1</b></span><span>after <b>quantity = 2</b></span></div>`;
      if (this.step === 5) return this.graphComparison();
      if (this.step === 6) return this.facetComparison();
      const kind = this.hashKind(node.id) || "local";
      const oldDigest = before[kind][this.dimension];
      const newDigest = node[kind][this.dimension];
      const same = oldDigest === newDigest;
      return `<p>${esc(this.specNode(node.id).label)} at <b>${kind}</b>: ${same ? "the input path does not include the edit" : "the edit reaches this hash"}.</p>
        ${this.digestBox("before", oldDigest)}
        ${this.digestBox("after", newDigest, same ? "same" : "different")}
        <div class="hw-verdict ${same ? "same" : "different"}">${same ? "exactly unchanged" : "changed"}</div>`;
    }

    detailCycle(node) {
      const component = this.componentFor(node.id);
      const member = component.members.find((item) => item.id === node.id);
      if (this.step === 0) {
        if (this.scenario === "cfg")
          return `<p>This graph separates four cyclic CFG blocks from their one-way instruction children. The children are normal downstream subtrees, not part of the loop SCC.</p>
            <pre class="hw-code">function sumMemory(n) -&gt; total {
  let i := 0
  for { } lt(i, n) { i := add(i, 1) } {
    let item := mload(add(0x80, mul(i, 0x20)))
    total := add(total, item)
  }
  mstore(0, total)
  return(0, 0x20)
}</pre>`;
        return `<p>Click any service. Arrows are dependency edges. Inventory and Catalog form the only non-trivial cycle.</p>${this.readableNode(node)}`;
      }
      if (this.step === 1)
        return `<p>The real <code>reject</code> policy returned:</p><pre class="hw-engine-error">${esc(this.rejectError || "cycle detected")}</pre><p>This is not a hash collision. The leaf-first algorithm has no valid next member inside the cycle.</p>`;
      if (this.step === 2) {
        const boundary =
          this.scenario === "cfg"
            ? "Entry only points in. Exit and Return only lead away. Instruction subtrees are only pointed to by their blocks and cannot reach back. All of them stay outside."
            : "Single services are components too.";
        return `<p>Tarjan returns one component with <b>${this.cycleMembers().size} mutually reachable members</b>. ${boundary}</p>${this.inputList(
          [...this.cycleMembers()].map((id) => this.specNode(id).label),
        )}`;
      }
      if (this.step === 3)
        if (this.scenario === "cfg" && node.kind === "yulssa.insn") {
          const outsideLoop = this.node(node.id, this.withoutBackedge);
          const before = outsideLoop.tree[this.dimension];
          const after = node.tree[this.dimension];
          return `<p>Remove only the backedge. This instruction's local and downstream subtree addresses are unchanged. The boxes show the complete subtree address.</p>
            ${this.digestBox("subtree without backedge", before)}
            ${this.digestBox("subtree inside loop", after, before === after ? "same" : "different")}
            <div class="hw-verdict ${before === after ? "same" : "different"}">${before === after ? "exactly unchanged" : "changed"}</div>`;
        }
      if (this.step === 3)
        return `<p>Cycles do not affect local content.</p>${this.inputList(this.localInputs(node))}${this.digestBox("node.local", node.local[this.dimension])}`;
      if (this.step === 4)
        return `<p><b>wl.init</b> starts from the local digest plus sorted outgoing cross-component edges. The engine keeps this intermediate private, then exposes the final color.</p>${this.digestBox("local input", node.local[this.dimension])}`;
      if (this.step === 5)
        return `<p>Each round mixes a member's previous color with its labeled internal neighbors. Stop when another round would not split any color class.</p>${this.digestBox("final member color", member.colors[this.dimension])}`;
      if (
        this.step === 7 &&
        this.scenario === "cfg" &&
        !this.cycleMembers().has(node.id)
      )
        return `<p>This instruction remains a singleton downstream component. Its normal tree address is computed independently, then the loop component commits to it.</p>${this.digestBox(`component ${component.index}`, component.digests[this.dimension])}`;
      if (
        this.step === 6 &&
        this.scenario === "cfg" &&
        this.cycleMembers().has(node.id)
      )
        return `<p>The outer component graph sees one LOOP SCC. Inside it, <b>Header → A → B → Latch → Header</b> remains the hashed control-flow graph. Condensation does not turn the blocks into siblings.</p>${this.digestBox(`component ${component.index}`, component.digests[this.dimension])}`;
      if (this.step === 6 || this.step === 7)
        return `<p><b>component.tree</b> commits to the stabilized component and anything it reaches outside itself.</p>${this.digestBox(`component ${component.index}`, component.digests[this.dimension])}`;
      if (this.step === 8)
        return `<p><b>node.component_context</b> restores a per-node result: local digest + final member color + component tree.</p>${this.inputList([
          `local ${short(node.local[this.dimension])}`,
          `color ${short(member.colors[this.dimension])}`,
          `component ${short(component.digests[this.dimension])}`,
        ])}${this.digestBox("node.component_context", node.tree[this.dimension])}`;
      if (this.step === 9) return this.graphDetail();
      return this.facetDetail();
    }

    readableNode(node, note = "") {
      const fields = node.fields.length
        ? node.fields.map(
            (field) =>
              `${field.dimension}: ${field.name} = ${fieldValue(field.value)}`,
          )
        : ["no fields"];
      return `${note ? `<p>${esc(note)}</p>` : ""}${this.inputList([
        `kind = ${node.kind}`,
        ...fields,
      ])}`;
    }

    policyHtml() {
      const policy = this.result.policy;
      return `<p>The policy is part of every digest record. Change it and the address changes deliberately.</p>
        ${this.inputList([
          `schema ${policy.schema_version}`,
          policy.algorithm,
          policy.level,
          policy.view_mode,
          policy.cycle_policy,
          "record tag + selected dimension",
        ])}
        ${this.digestBox("policy id", policy.policy_id)}
        <p class="hw-small">Exact encoding: ${esc(policy.encoding)}.</p>`;
    }

    graphDetail() {
      const edges = this.result.edges.length;
      return `<p><b>graph.full</b> hashes a sorted multiset of final node digests and ${edges} flat edge records.</p>
        ${this.inputList([
          `${this.result.nodes.length} final node digests`,
          `${edges} role + label + endpoint records`,
        ])}
        ${this.digestBox(
          `graph.full, ${this.dimension}`,
          this.result.graph[this.dimension],
        )}`;
    }

    graphComparison() {
      const oldDigest = this.before.graph[this.dimension];
      const newDigest = this.result.graph[this.dimension];
      const same = oldDigest === newDigest;
      return `<p>The whole graph in the <b>${this.dimension}</b> dimension.</p>
        ${this.digestBox("before", oldDigest)}
        ${this.digestBox("after", newDigest, same ? "same" : "different")}
        <div class="hw-verdict ${same ? "same" : "different"}">${same ? "exactly unchanged" : "changed"}</div>`;
    }

    facetDetail() {
      const facet =
        this.dimension === "structure"
          ? ["structure-only", "structure"]
          : this.dimension === "names"
            ? ["full", "full"]
            : ["names-blind", "names-blind"];
      return `<p>A facet address binds graph digests to the policy id. The <b>${facet[0]}</b> facet is the smallest built-in facet here that keeps ${this.dimension}.</p>
        ${this.digestBox(
          `${this.dimension} graph digest`,
          this.result.graph[this.dimension],
        )}
        ${this.digestBox(
          `${facet[0]} facet address`,
          this.result.facets[facet[1]],
        )}`;
    }

    facetComparison() {
      const rows = [
        ["structure-only", "structure"],
        ["names-blind", "names-blind"],
        ["full", "full"],
      ];
      return `<p>The same edit produces different answers depending on the question.</p>
        <div class="hw-facet-table">${rows
          .map(([label, key]) => {
            const same = this.before.facets[key] === this.result.facets[key];
            return `<div><span>${label}</span><code>${short(this.before.facets[key])} ${same ? "=" : "!="} ${short(this.result.facets[key])}</code><b class="${same ? "same" : "different"}">${same ? "stable" : "moves"}</b></div>`;
          })
          .join("")}</div>`;
    }

    inputList(items) {
      return `<ol class="hw-inputs">${items
        .map((item) => `<li><code>${esc(item)}</code></li>`)
        .join("")}</ol>`;
    }

    detailHtml() {
      const node = this.node(this.selected) || this.result.nodes[0];
      let body;
      if (this.scenario === "order") body = this.detailOrder(node);
      else if (this.scenario === "edit") body = this.detailEdit(node);
      else body = this.detailCycle(node);
      return `<aside class="hw-detail">
        <div class="hw-detail-head"><span>inspect</span><b>${esc(this.specNode(node.id).label)}</b></div>
        ${body}
      </aside>`;
    }

    render() {
      if (!this.result) return;
      const current = this.steps()[this.step];
      const rail = this.steps()
        .map(
          (step, index) =>
            `<button type="button" data-step="${index}" class="${index === this.step ? "on" : ""}" aria-current="${index === this.step ? "step" : "false"}" title="${esc(step[0])}">${index + 1}</button>`,
        )
        .join("");
      const dimensions = DIMS.map(
        (key) =>
          `<button type="button" data-dimension="${key}" class="${key === this.dimension ? "on" : ""}" aria-pressed="${key === this.dimension}">${key}</button>`,
      ).join("");
      this.innerHTML = `
        <div class="hw-topline">
          <div class="hw-controls">
            <button type="button" data-action="back" ${this.step === 0 ? "disabled" : ""}>← Back</button>
            <button type="button" data-action="next" ${this.step === this.steps().length - 1 ? "disabled" : ""}>Next →</button>
            <button type="button" data-action="reset">Reset</button>
            <button type="button" data-action="play">${this.playTimer ? "Pause" : "Play"}</button>
          </div>
          <span class="hw-keyhint">arrow keys work too</span>
        </div>
        <div class="hw-rail" style="--hw-steps:${this.steps().length}" aria-label="walkthrough steps">${rail}</div>
        <div class="hw-step-copy"><span>step ${this.step + 1} of ${this.steps().length}</span><b>${esc(current[0])}</b><p>${esc(current[1])}</p></div>
        <div class="hw-dimensions"><span>follow one dimension</span>${dimensions}</div>
        <div class="hw-main">${this.graphHtml()}${this.detailHtml()}</div>`;

      this.querySelector('[data-action="back"]').addEventListener("click", () =>
        this.previousStep(),
      );
      this.querySelector('[data-action="next"]').addEventListener("click", () =>
        this.nextStep(),
      );
      this.querySelector('[data-action="reset"]').addEventListener("click", () => {
        this.stopPlaying();
        this.step = -1;
        this.setStep(0);
      });
      this.querySelector('[data-action="play"]').addEventListener("click", () =>
        this.togglePlay(),
      );
      this.querySelectorAll("[data-step]").forEach((button) =>
        button.addEventListener("click", () => {
          this.stopPlaying();
          this.setStep(+button.dataset.step);
        }),
      );
      this.querySelectorAll("[data-dimension]").forEach((button) =>
        button.addEventListener("click", () => {
          this.dimension = button.dataset.dimension;
          this.render();
        }),
      );
      this.querySelectorAll("[data-node]").forEach((button) =>
        button.addEventListener("click", () => {
          this.selected = button.dataset.node;
          this.render();
        }),
      );
    }
  },
);
