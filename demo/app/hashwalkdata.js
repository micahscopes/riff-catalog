// Real-world graphs for the interactive hashing walkthrough. The page supplies
// only this readable data. All digests, SCCs, colors, and addresses come back
// from the Rust riff-cat engine running in wasm.
window.RIFFCAT_HASH_WALKS = (() => {
  const order = {
    owner: "walk:coffee-shop",
    unit: "order-1042",
    unit_kind: "commerce.order",
    nodes: [
      {
        id: "order",
        kind: "commerce.order",
        label: "Order #1042",
        x: 50,
        y: 10,
        fields: [
          ["names", "number", "ORD-1042"],
          ["constants", "currency", "USD"],
          ["constants", "status", "paid"],
          ["types", "schema", "OrderV3"],
        ],
      },
      {
        id: "buyer",
        kind: "commerce.customer",
        label: "Customer",
        x: 14,
        y: 43,
        fields: [
          ["names", "display_name", "Mina"],
          ["types", "customer_id", "UUID"],
        ],
      },
      {
        id: "beans_line",
        kind: "commerce.line_item",
        label: "Beans x1",
        x: 50,
        y: 43,
        fields: [["constants", "quantity", "1"]],
      },
      {
        id: "filters_line",
        kind: "commerce.line_item",
        label: "Filters x1",
        x: 84,
        y: 43,
        fields: [["constants", "quantity", "1"]],
      },
      {
        id: "beans_product",
        kind: "commerce.product_snapshot",
        label: "Coffee beans",
        x: 50,
        y: 78,
        fields: [
          ["names", "title", "Coffee beans"],
          ["constants", "unit_price_cents", "1800"],
          ["types", "price", "u64"],
        ],
      },
      {
        id: "filters_product",
        kind: "commerce.product_snapshot",
        label: "Paper filters",
        x: 84,
        y: 78,
        fields: [
          ["names", "title", "Paper filters"],
          ["constants", "unit_price_cents", "650"],
          ["types", "price", "u64"],
        ],
      },
    ],
    children: [
      ["order", "buyer", 0, "buyer"],
      ["order", "item", 1, "beans_line"],
      ["order", "item", 2, "filters_line"],
      ["beans_line", "product", 0, "beans_product"],
      ["filters_line", "product", 0, "filters_product"],
    ],
    edges: [],
  };

  const editedOrder = JSON.parse(JSON.stringify(order));
  editedOrder.unit = "order-1042-quantity-edit";
  editedOrder.nodes.find((node) => node.id === "filters_line").label =
    "Filters x2";
  editedOrder.nodes.find((node) => node.id === "filters_line").fields = [
    ["constants", "quantity", "2"],
  ];

  const services = {
    owner: "walk:coffee-shop",
    unit: "production-services",
    unit_kind: "deployment.service_graph",
    nodes: [
      {
        id: "storefront",
        kind: "deployment.service",
        label: "Storefront",
        x: 50,
        y: 8,
        fields: [
          ["names", "name", "storefront"],
          ["types", "runtime", "Node.js"],
        ],
      },
      {
        id: "checkout",
        kind: "deployment.service",
        label: "Checkout",
        x: 50,
        y: 32,
        fields: [
          ["names", "name", "checkout"],
          ["types", "runtime", "Rust"],
        ],
      },
      {
        id: "inventory",
        kind: "deployment.service",
        label: "Inventory",
        x: 27,
        y: 63,
        fields: [
          ["names", "name", "inventory"],
          ["constants", "api_version", "v2"],
        ],
      },
      {
        id: "payments",
        kind: "deployment.service",
        label: "Payments",
        x: 74,
        y: 63,
        fields: [
          ["names", "name", "payments"],
          ["constants", "provider", "stripe"],
        ],
      },
      {
        id: "catalog",
        kind: "deployment.service",
        label: "Catalog",
        x: 27,
        y: 89,
        fields: [
          ["names", "name", "catalog"],
          ["constants", "api_version", "v4"],
        ],
      },
    ],
    children: [],
    edges: [
      ["storefront", "calls", "checkout", "dependency"],
      ["checkout", "reserves", "inventory", "dependency"],
      ["checkout", "charges", "payments", "dependency"],
      ["inventory", "reads", "catalog", "dependency"],
      ["catalog", "checks_stock", "inventory", "dependency"],
    ],
  };

  // A two-layer view of a lowered Yul loop. The four CFG blocks are mutually
  // reachable through the backedge. Their instruction subtrees are reached by
  // one-way child edges, so they remain downstream components whose digests
  // are folded into the block SCC without becoming members of it.
  const cfg = {
    owner: "walk:yul-cfg",
    unit: "sum-memory",
    unit_kind: "yulssa.fn",
    nodes: [
      {
        id: "entry",
        kind: "yulssa.block",
        label: "Entry",
        x: 13,
        y: 16,
        fields: [
          ["structure", "terminator", "jump"],
          ["constants", "initial_i", "0"],
        ],
      },
      {
        id: "loop_header",
        kind: "yulssa.block",
        label: "Header block",
        x: 40,
        y: 16,
        fields: [
          ["structure", "terminator", "conditional_jump"],
        ],
      },
      {
        id: "load_item",
        kind: "yulssa.block",
        label: "Body block A",
        x: 40,
        y: 39,
        fields: [["structure", "terminator", "jump"]],
      },
      {
        id: "add_total",
        kind: "yulssa.block",
        label: "Body block B",
        x: 40,
        y: 62,
        fields: [["structure", "terminator", "jump"]],
      },
      {
        id: "increment",
        kind: "yulssa.block",
        label: "Latch block",
        x: 40,
        y: 85,
        fields: [["structure", "terminator", "backedge"]],
      },
      {
        id: "test_insn",
        kind: "yulssa.insn",
        label: "lt(i, n)",
        x: 66,
        y: 16,
        fields: [
          ["structure", "op", "lt"],
          ["names", "operands", "i,n"],
        ],
      },
      {
        id: "load_insn",
        kind: "yulssa.insn",
        label: "mload(...)",
        x: 66,
        y: 39,
        fields: [
          ["structure", "op", "mload"],
          ["types", "result", "u256"],
        ],
      },
      {
        id: "add_insn",
        kind: "yulssa.insn",
        label: "total := add(...)",
        x: 66,
        y: 62,
        fields: [
          ["structure", "op", "add"],
          ["types", "result", "u256"],
        ],
      },
      {
        id: "inc_insn",
        kind: "yulssa.insn",
        label: "i := add(i, 1)",
        x: 66,
        y: 85,
        fields: [
          ["structure", "op", "add"],
          ["constants", "step", "1"],
        ],
      },
      {
        id: "exit",
        kind: "yulssa.block",
        label: "Exit loop",
        x: 89,
        y: 29,
        fields: [["structure", "terminator", "jump"]],
      },
      {
        id: "return",
        kind: "yulssa.block",
        label: "Return total",
        x: 89,
        y: 63,
        fields: [
          ["structure", "op", "mstore_return"],
          ["types", "value", "u256"],
        ],
      },
    ],
    children: [
      ["loop_header", "instruction", 0, "test_insn"],
      ["load_item", "instruction", 0, "load_insn"],
      ["add_total", "instruction", 0, "add_insn"],
      ["increment", "instruction", 0, "inc_insn"],
    ],
    edges: [
      ["entry", "jump", "loop_header", "dependency"],
      ["loop_header", "true", "load_item", "dependency"],
      ["loop_header", "false", "exit", "dependency"],
      ["load_item", "next", "add_total", "dependency"],
      ["add_total", "next", "increment", "dependency"],
      ["increment", "backedge", "loop_header", "dependency"],
      ["exit", "jump", "return", "dependency"],
    ],
  };

  return { order, editedOrder, services, cfg };
})();
