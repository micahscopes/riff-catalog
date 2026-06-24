//! Golden-vector dumper for the Lean lockstep spec (see `lean/README.md`).
//!
//! This is an ADDITIVE test: it changes no crate source and no Cargo.toml. When
//! run, it writes `lean/golden/vectors.json`, a versioned corpus of
//! `(input graph, policy) -> canonical digests` vectors that the Lean
//! executable spec (`lake exe check`) recomputes and compares byte for byte.
//!
//! Run explicitly to (re)generate the corpus:
//!   cargo test -p riff-catalog-core --test lean_vectors -- --ignored --nocapture
//!
//! The corpus is emitted in a deliberately regular JSON shape so the Lean side
//! can parse it with a tiny purpose-built reader (no JSON dependency in Lean).
//! Each graph is described as a list of build "ops" (the same calls a producer
//! makes), so both languages construct the identical `Graph`. We also emit a
//! BLAKE3 known-answer section: the Rust `blake3` crate hashes the standard
//! reference inputs, pinning that the pure-Lean BLAKE3 matches the same library
//! riffcat actually ships.

use riff_catalog_core::{
    CyclePolicy, Digest, DigestRequest, Dimension, EdgeRole, EntityKey, Facet, Graph, GraphKey,
    HashPolicy, NodeKey, ViewMode, digest_graph,
};

const VECTORS_PATH: &str = "../../lean/golden/vectors.json";

/// JSON-escape a string (only the characters our identifiers can contain plus
/// the unit separator, which appears in canonical keys; serde-compatible).
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// A build op, serialized as a JSON object the Lean side replays. Node keys are
/// always "entity" keys here (kind, owner, local); that covers the corpus.
enum Op {
    Node { kind: String, owner: String, local: String, node_kind: String },
    FieldU64 { owner: String, local: String, dim: Dimension, name: String, value: u64 },
    FieldI64 { owner: String, local: String, dim: Dimension, name: String, value: i64 },
    FieldText { owner: String, local: String, dim: Dimension, name: String, value: String },
    FieldBool { owner: String, local: String, dim: Dimension, name: String, value: bool },
    FieldBytes { owner: String, local: String, dim: Dimension, name: String, value: Vec<u8> },
    Child { p: (String, String), label: String, ordinal: u32, c: (String, String) },
    Edge { s: (String, String), label: String, t: (String, String), role: EdgeRole },
}

fn ek(kind: &str, owner: &str, local: &str) -> NodeKey {
    NodeKey::entity(EntityKey::new(kind, owner, local).unwrap())
}

impl Op {
    fn apply(&self, g: &mut Graph) {
        match self {
            Op::Node { kind, owner, local, node_kind } => {
                g.add_node(ek(kind, owner, local), node_kind.as_str()).unwrap();
            }
            Op::FieldU64 { owner, local, dim, name, value } => {
                g.add_field(&ek("n", owner, local), *dim, name.as_str(), *value).unwrap();
            }
            Op::FieldI64 { owner, local, dim, name, value } => {
                g.add_field(&ek("n", owner, local), *dim, name.as_str(), *value).unwrap();
            }
            Op::FieldText { owner, local, dim, name, value } => {
                g.add_field(&ek("n", owner, local), *dim, name.as_str(), value.as_str()).unwrap();
            }
            Op::FieldBool { owner, local, dim, name, value } => {
                g.add_field(&ek("n", owner, local), *dim, name.as_str(), *value).unwrap();
            }
            Op::FieldBytes { owner, local, dim, name, value } => {
                g.add_field(&ek("n", owner, local), *dim, name.as_str(), value.clone()).unwrap();
            }
            Op::Child { p, label, ordinal, c } => {
                g.add_child(&ek("n", &p.0, &p.1), label.as_str(), *ordinal, &ek("n", &c.0, &c.1))
                    .unwrap();
            }
            Op::Edge { s, label, t, role } => {
                g.add_edge(&ek("n", &s.0, &s.1), label.as_str(), &ek("n", &t.0, &t.1), *role)
                    .unwrap();
            }
        }
    }

    fn to_json(&self) -> String {
        let dim = |d: &Dimension| esc(d.as_str());
        match self {
            Op::Node { kind, owner, local, node_kind } => format!(
                r#"{{"op":"node","kind":"{}","owner":"{}","local":"{}","node_kind":"{}"}}"#,
                esc(kind), esc(owner), esc(local), esc(node_kind)
            ),
            Op::FieldU64 { owner, local, dim: d, name, value } => format!(
                r#"{{"op":"field","owner":"{}","local":"{}","dim":"{}","name":"{}","vkind":"u64","value":"{}"}}"#,
                esc(owner), esc(local), dim(d), esc(name), value
            ),
            Op::FieldI64 { owner, local, dim: d, name, value } => format!(
                r#"{{"op":"field","owner":"{}","local":"{}","dim":"{}","name":"{}","vkind":"i64","value":"{}"}}"#,
                esc(owner), esc(local), dim(d), esc(name), value
            ),
            Op::FieldText { owner, local, dim: d, name, value } => format!(
                r#"{{"op":"field","owner":"{}","local":"{}","dim":"{}","name":"{}","vkind":"text","value":"{}"}}"#,
                esc(owner), esc(local), dim(d), esc(name), esc(value)
            ),
            Op::FieldBool { owner, local, dim: d, name, value } => format!(
                r#"{{"op":"field","owner":"{}","local":"{}","dim":"{}","name":"{}","vkind":"bool","value":"{}"}}"#,
                esc(owner), esc(local), dim(d), esc(name), if *value { 1 } else { 0 }
            ),
            Op::FieldBytes { owner, local, dim: d, name, value } => {
                let hex: String = value.iter().map(|b| format!("{:02x}", b)).collect();
                format!(
                    r#"{{"op":"field","owner":"{}","local":"{}","dim":"{}","name":"{}","vkind":"bytes","value":"{}"}}"#,
                    esc(owner), esc(local), dim(d), esc(name), hex
                )
            }
            Op::Child { p, label, ordinal, c } => format!(
                r#"{{"op":"child","p_owner":"{}","p_local":"{}","label":"{}","ordinal":{},"c_owner":"{}","c_local":"{}"}}"#,
                esc(&p.0), esc(&p.1), esc(label), ordinal, esc(&c.0), esc(&c.1)
            ),
            Op::Edge { s, label, t, role } => format!(
                r#"{{"op":"edge","s_owner":"{}","s_local":"{}","label":"{}","t_owner":"{}","t_local":"{}","role":"{}"}}"#,
                esc(&s.0), esc(&s.1), esc(label), esc(&t.0), esc(&t.1), esc(role.as_str())
            ),
        }
    }
}

struct Vector {
    name: &'static str,
    graph_key: (&'static str, &'static str, &'static str, &'static str), // kind, owner, local, gk-local
    ops: Vec<Op>,
}

fn node(kind: &str, owner: &str, local: &str, nk: &str) -> Op {
    Op::Node { kind: kind.into(), owner: owner.into(), local: local.into(), node_kind: nk.into() }
}
fn fu64(owner: &str, local: &str, d: Dimension, name: &str, v: u64) -> Op {
    Op::FieldU64 { owner: owner.into(), local: local.into(), dim: d, name: name.into(), value: v }
}
fn fi64(owner: &str, local: &str, d: Dimension, name: &str, v: i64) -> Op {
    Op::FieldI64 { owner: owner.into(), local: local.into(), dim: d, name: name.into(), value: v }
}
fn ftext(owner: &str, local: &str, d: Dimension, name: &str, v: &str) -> Op {
    Op::FieldText { owner: owner.into(), local: local.into(), dim: d, name: name.into(), value: v.into() }
}
fn fbool(owner: &str, local: &str, d: Dimension, name: &str, v: bool) -> Op {
    Op::FieldBool { owner: owner.into(), local: local.into(), dim: d, name: name.into(), value: v }
}
fn fbytes(owner: &str, local: &str, d: Dimension, name: &str, v: Vec<u8>) -> Op {
    Op::FieldBytes { owner: owner.into(), local: local.into(), dim: d, name: name.into(), value: v }
}
fn child(po: &str, pl: &str, label: &str, ord: u32, co: &str, cl: &str) -> Op {
    Op::Child { p: (po.into(), pl.into()), label: label.into(), ordinal: ord, c: (co.into(), cl.into()) }
}
fn edge(so: &str, sl: &str, label: &str, to: &str, tl: &str, role: EdgeRole) -> Op {
    Op::Edge { s: (so.into(), sl.into()), label: label.into(), t: (to.into(), tl.into()), role }
}

/// The corpus. Deliberately includes the adversarial shapes the SCHEMA_VERSION
/// 2 bump fixed plus the standard small cases.
fn corpus() -> Vec<Vector> {
    use Dimension::*;
    let o = "demo"; // common owner for nodes; graph key owner is per-vector

    vec![
        // 1. empty graph (no nodes, no edges)
        Vector { name: "empty", graph_key: ("test.unit", "g", "unit:0", "unit"), ops: vec![] },

        // 2. single node, all-dimension fields and one self-describing kind
        Vector {
            name: "single_node",
            graph_key: ("test.unit", "g", "unit:0", "unit"),
            ops: vec![
                node("n", o, "a", "literal"),
                fu64(o, "a", Constants, "value", 13),
                ftext(o, "a", Names, "name", "x"),
                ftext(o, "a", Types, "type", "u256"),
                fbool(o, "a", TraceEvents, "emitted", true),
            ],
        },

        // 3. body -> literal with one reference edge (the golden.rs fixture
        //    shape; node-key `kind` is uniformly "n" here, see `ek`).
        Vector {
            name: "golden_fixture",
            graph_key: ("test.unit", "golden", "unit:0", "unit"),
            ops: vec![
                node("n", "golden", "body:0", "body"),
                node("n", "golden", "expr:0", "literal"),
                fu64("golden", "expr:0", Constants, "value", 13),
                ftext("golden", "expr:0", Names, "name", "x"),
                ftext("golden", "expr:0", Types, "type", "u256"),
                child("golden", "body:0", "expr", 0, "golden", "expr:0"),
                edge("golden", "expr:0", "uses", "golden", "body:0", EdgeRole::Reference),
            ],
        },

        // 4. duplicate (ordinal, label) children, distinct constants:
        //    f(1, 2) shape. Tests the SCHEMA_VERSION 2 fix at the Constants facet.
        Vector {
            name: "dup_children_1_2",
            graph_key: ("test.unit", "g", "unit:0", "unit"),
            ops: vec![
                node("n", o, "f", "call"),
                node("n", o, "a1", "literal"),
                node("n", o, "a2", "literal"),
                fu64(o, "a1", Constants, "value", 1),
                fu64(o, "a2", Constants, "value", 2),
                child(o, "f", "arg", 0, o, "a1"),
                child(o, "f", "arg", 0, o, "a2"),
            ],
        },
        // 4b. f(2, 1): same shape, swapped constants. Its Constants/full digests
        //     must differ from 4 (the bug the v2 bump fixed).
        Vector {
            name: "dup_children_2_1",
            graph_key: ("test.unit", "g", "unit:0", "unit"),
            ops: vec![
                node("n", o, "f", "call"),
                node("n", o, "a1", "literal"),
                node("n", o, "a2", "literal"),
                fu64(o, "a1", Constants, "value", 2),
                fu64(o, "a2", Constants, "value", 1),
                child(o, "f", "arg", 0, o, "a1"),
                child(o, "f", "arg", 0, o, "a2"),
            ],
        },

        // 5. dependency cycle (a -> b -> a), exercised under CondenseScc.
        Vector {
            name: "dep_cycle",
            graph_key: ("test.unit", "g", "unit:0", "unit"),
            ops: vec![
                node("n", o, "a", "fn"),
                node("n", o, "b", "fn"),
                ftext(o, "a", Names, "name", "alpha"),
                ftext(o, "b", Names, "name", "beta"),
                edge(o, "a", "calls", o, "b", EdgeRole::Dependency),
                edge(o, "b", "calls", o, "a", EdgeRole::Dependency),
            ],
        },

        // 6. symmetric SCC with an asymmetric tail outside: two members in a
        //    cycle, one of which also points out to an external node. Exercises
        //    the wl.init cross-edge fold (members must color apart).
        Vector {
            name: "symmetric_scc_with_tail",
            graph_key: ("test.unit", "g", "unit:0", "unit"),
            ops: vec![
                node("n", o, "x", "fn"),
                node("n", o, "y", "fn"),
                node("n", o, "leaf", "fn"),
                edge(o, "x", "e", o, "y", EdgeRole::Dependency),
                edge(o, "y", "e", o, "x", EdgeRole::Dependency),
                edge(o, "x", "e", o, "leaf", EdgeRole::Dependency),
            ],
        },

        // 7. mixed value kinds incl. negative i64 and raw bytes, plus an Origin
        //    edge (which graph.full must skip) and a Call edge (which it keeps).
        Vector {
            name: "mixed_values",
            graph_key: ("test.unit", "g", "unit:0", "unit"),
            ops: vec![
                node("n", o, "p", "node"),
                node("n", o, "q", "node"),
                fi64(o, "p", Constants, "delta", -42),
                fbytes(o, "p", Constants, "blob", vec![0xde, 0xad, 0xbe, 0xef]),
                ftext(o, "q", Names, "name", "q"),
                child(o, "p", "kid", 0, o, "q"),
                edge(o, "q", "origin", o, "p", EdgeRole::Origin),
                edge(o, "q", "calls", o, "p", EdgeRole::Call),
            ],
        },
    ]
}

/// The two policies the golden tests pin, plus a NonRecursiveGraphEdges variant.
fn policies() -> Vec<(&'static str, HashPolicy)> {
    vec![
        (
            "identity_reject",
            HashPolicy::new("test/1", ViewMode::IdentityBound, CyclePolicy::Reject).unwrap(),
        ),
        (
            "anon_condense",
            HashPolicy::new("test/1", ViewMode::AnonymousShape, CyclePolicy::CondenseScc).unwrap(),
        ),
        (
            "identity_condense",
            HashPolicy::new("test/1", ViewMode::IdentityBound, CyclePolicy::CondenseScc).unwrap(),
        ),
    ]
}

/// A policy is applicable to a vector only if the cycle policy admits the graph
/// (Reject rejects the cyclic vectors). Returns None if not applicable.
fn try_digests(policy: &HashPolicy, gk: &GraphKey, g: &Graph) -> Option<Vec<(Dimension, Digest)>> {
    let req = DigestRequest::all_dimensions(gk.clone(), policy.clone());
    match digest_graph(&req, g) {
        Ok(res) => Some(res.hashes.graph.iter().map(|(d, h)| (*d, *h)).collect()),
        Err(_) => None,
    }
}

/// BLAKE3 known-answer cross-check: hash the standard reference inputs with the
/// shipping `blake3` crate so the Lean pure-BLAKE3 is pinned to the same lib.
fn blake3_kats() -> Vec<(usize, String)> {
    [0usize, 1, 64, 1023, 1024, 1025, 2048, 2049]
        .into_iter()
        .map(|n| {
            let input: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
            (n, blake3::hash(&input).to_hex().to_string())
        })
        .collect()
}

fn build_graph(v: &Vector) -> (GraphKey, Graph) {
    let (kind, owner, local, gk_local) = v.graph_key;
    let gk = GraphKey::new(EntityKey::new(kind, owner, local).unwrap(), gk_local).unwrap();
    let mut g = Graph::new(gk.clone());
    for op in &v.ops {
        op.apply(&mut g);
    }
    (gk, g)
}

#[test]
#[ignore = "run explicitly to regenerate lean/golden/vectors.json"]
fn dump_lean_vectors() {
    let mut json = String::new();
    json.push_str("{\n");
    json.push_str(&format!("  \"schema_version\": {},\n", riff_catalog_core::SCHEMA_VERSION));
    json.push_str("  \"magic\": \"riffcat\",\n");

    // blake3 KATs
    json.push_str("  \"blake3_kats\": [\n");
    let kats = blake3_kats();
    for (i, (n, hex)) in kats.iter().enumerate() {
        let comma = if i + 1 < kats.len() { "," } else { "" };
        json.push_str(&format!("    {{\"len\": {}, \"hash\": \"{}\"}}{}\n", n, hex, comma));
    }
    json.push_str("  ],\n");

    // policy ids (meta records)
    json.push_str("  \"policies\": [\n");
    let pols = policies();
    for (i, (label, p)) in pols.iter().enumerate() {
        let comma = if i + 1 < pols.len() { "," } else { "" };
        let (view, cyc) = (p.view_mode.as_str(), p.cycle_policy.as_str());
        json.push_str(&format!(
            "    {{\"label\": \"{}\", \"level\": \"{}\", \"view_mode\": \"{}\", \"cycle_policy\": \"{}\", \"policy_id\": \"{}\"}}{}\n",
            label, "test/1", view, cyc, p.policy_id().to_hex(), comma
        ));
    }
    json.push_str("  ],\n");

    // vectors
    json.push_str("  \"vectors\": [\n");
    let vectors = corpus();
    let mut vector_jsons: Vec<String> = Vec::new();
    for v in &vectors {
        let (gk, g) = build_graph(v);
        // ops
        let ops_json: Vec<String> = v.ops.iter().map(|o| format!("        {}", o.to_json())).collect();
        // per applicable policy: graph digests + facet ids/addresses
        let mut results: Vec<String> = Vec::new();
        for (label, p) in &pols {
            if let Some(digests) = try_digests(p, &gk, &g) {
                let dim_json: Vec<String> = digests
                    .iter()
                    .map(|(d, h)| format!(r#"{{"dim":"{}","digest":"{}"}}"#, d.as_str(), h.to_hex()))
                    .collect();
                // facet addresses for full and names_blind
                let res = digest_graph(
                    &DigestRequest::all_dimensions(gk.clone(), p.clone()),
                    &g,
                )
                .unwrap();
                let facets = [
                    ("full", Facet::full(p.policy_id())),
                    ("names_blind", Facet::names_blind(p.policy_id())),
                    ("structure_only", Facet::structure_only(p.policy_id())),
                ];
                let mut facet_json: Vec<String> = Vec::new();
                for (fname, facet) in facets.iter() {
                    let addr = res.hashes.facet_address(facet).unwrap();
                    facet_json.push(format!(
                        r#"{{"name":"{}","facet_id":"{}","address_digest":"{}"}}"#,
                        fname,
                        facet.facet_id().to_hex(),
                        addr.address_digest().to_hex()
                    ));
                }
                results.push(format!(
                    "        {{\"policy\": \"{}\", \"graph\": [{}], \"facets\": [{}]}}",
                    label,
                    dim_json.join(", "),
                    facet_json.join(", ")
                ));
            }
        }
        let (kind, owner, local, gk_local) = v.graph_key;
        vector_jsons.push(format!(
            "    {{\n      \"name\": \"{}\",\n      \"graph_key\": {{\"kind\":\"{}\",\"owner\":\"{}\",\"local\":\"{}\",\"gk_local\":\"{}\"}},\n      \"ops\": [\n{}\n      ],\n      \"results\": [\n{}\n      ]\n    }}",
            v.name, kind, owner, local, gk_local,
            ops_json.join(",\n"),
            results.join(",\n"),
        ));
    }
    json.push_str(&vector_jsons.join(",\n"));
    json.push_str("\n  ]\n}\n");

    std::fs::write(VECTORS_PATH, json).expect("write vectors.json");
    eprintln!("wrote {} vectors to {}", vectors.len(), VECTORS_PATH);
}
