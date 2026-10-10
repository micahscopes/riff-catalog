//! Oracle for the meaning of a facet address.
//!
//! Meaning: two graphs are equal at facet F when the graphs that keep only F's
//! facts, on the skeleton every dimension shares, are the same up to renaming
//! of keys (free mode). The skeleton is the nodes, the child edges with their
//! labels and ordinals, and the flat edges with their roles and labels. Kinds
//! count only when F contains structure.
//!
//! The oracle compares facet addresses with the exact canonicaliser in
//! `riff_catalog_exact` on small random graphs, in both directions:
//!
//! - equal meaning must give equal addresses;
//! - equal addresses must mean equal meaning, except for pairs that colour
//!   refinement itself cannot tell apart (the documented 1-WL limit).

use std::collections::BTreeMap;

use riff_catalog_core::*;
use riff_catalog_exact as exact;

const DIMS: [Dimension; 3] = [Dimension::Structure, Dimension::Names, Dimension::Types];

/// A small deterministic generator (xorshift64*), so failures reproduce.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

/// A plain description of a graph, easy to mutate and to turn into either a
/// catalog graph or an exact-canonicaliser graph.
#[derive(Clone, Debug)]
struct Spec {
    kinds: Vec<&'static str>,
    names: Vec<&'static str>,
    types: Vec<&'static str>,
    /// (parent, label, ordinal, child)
    children: Vec<(usize, &'static str, u32, usize)>,
    /// (source, role, label, target)
    edges: Vec<(usize, EdgeRole, &'static str, usize)>,
    /// Key text per node; renaming keys must not change free-mode meaning.
    keys: Vec<String>,
}

fn random_spec(rng: &mut Rng) -> Spec {
    let n = 2 + rng.below(5);
    let pick = |rng: &mut Rng, xs: &[&'static str]| xs[rng.below(xs.len())];
    let mut spec = Spec {
        kinds: (0..n).map(|_| pick(rng, &["a", "b"])).collect(),
        names: (0..n).map(|_| pick(rng, &["x", "y"])).collect(),
        types: (0..n).map(|_| pick(rng, &["t0", "t1"])).collect(),
        children: Vec::new(),
        edges: Vec::new(),
        keys: (0..n).map(|i| format!("n{i}")).collect(),
    };
    // A forest: each node after the first either starts a new root (rarely) or
    // hangs under an earlier node. Ordinals 0 or 1 under label `c` give both
    // sequences and sets (tied children).
    for child in 1..n {
        if rng.chance(15) {
            continue;
        }
        let parent = rng.below(child);
        let ordinal = rng.below(2) as u32;
        spec.children.push((parent, "c", ordinal, child));
    }
    for _ in 0..rng.below(3) {
        let role = if rng.chance(20) { EdgeRole::Dependency } else { EdgeRole::Data };
        spec.edges.push((rng.below(n), role, "e", rng.below(n)));
    }
    spec
}

/// A second graph that is often close to the first: the interesting pairs.
fn mutate(rng: &mut Rng, spec: &Spec) -> Spec {
    let mut out = spec.clone();
    let n = out.kinds.len();
    match rng.below(7) {
        0 => {
            // Swap one dimension's facts between two nodes.
            let (i, j) = (rng.below(n), rng.below(n));
            match rng.below(3) {
                0 => out.kinds.swap(i, j),
                1 => out.names.swap(i, j),
                _ => out.types.swap(i, j),
            }
        }
        1 => {
            // Swap the targets of two flat edges.
            if out.edges.len() >= 2 {
                let t = out.edges[0].3;
                out.edges[0].3 = out.edges[1].3;
                out.edges[1].3 = t;
            }
        }
        2 => {
            // Retarget one flat edge.
            if let Some(edge) = out.edges.first_mut() {
                edge.3 = rng.below(n);
            }
        }
        3 => {
            // Change one fact.
            let i = rng.below(n);
            out.types[i] = if out.types[i] == "t0" { "t1" } else { "t0" };
        }
        4 => {
            // Rename every key: free-mode meaning must not move.
            out.keys = (0..n).map(|i| format!("m{}", n - i)).collect();
        }
        5 => {
            // Change one ordinal (sequence <-> set).
            if let Some(child) = out.children.first_mut() {
                child.2 = 1 - child.2;
            }
        }
        _ => return random_spec(rng),
    }
    out
}

fn node_key(spec: &Spec, i: usize) -> NodeKey {
    NodeKey::entity(EntityKey::new("t.node", "o", &spec.keys[i]).unwrap())
}

fn catalog_graph(spec: &Spec) -> Graph {
    let key = GraphKey::new(EntityKey::new("t.g", "o", "g").unwrap(), "g").unwrap();
    let mut g = Graph::new(key);
    for i in 0..spec.kinds.len() {
        let k = node_key(spec, i);
        g.add_node(k.clone(), spec.kinds[i]).unwrap();
        g.add_field(&k, Dimension::Names, "name", spec.names[i]).unwrap();
        g.add_field(&k, Dimension::Types, "type", spec.types[i]).unwrap();
    }
    for &(p, label, ordinal, c) in &spec.children {
        g.add_child(&node_key(spec, p), label, ordinal, &node_key(spec, c)).unwrap();
    }
    for &(s, role, label, t) in &spec.edges {
        g.add_edge(&node_key(spec, s), label, &node_key(spec, t), role).unwrap();
    }
    g
}

/// `keep_F(G)` as a vertex- and edge-labelled multigraph for the canonicaliser.
fn kept(spec: &Spec, facet: &[Dimension]) -> exact::Graph {
    let labels = (0..spec.kinds.len())
        .map(|i| {
            let mut parts = Vec::new();
            if facet.contains(&Dimension::Structure) {
                parts.push(format!("kind={}", spec.kinds[i]));
            }
            if facet.contains(&Dimension::Names) {
                parts.push(format!("names.name={}", spec.names[i]));
            }
            if facet.contains(&Dimension::Types) {
                parts.push(format!("types.type={}", spec.types[i]));
            }
            parts.join(";")
        })
        .collect();
    let mut edges: Vec<exact::Edge> = spec
        .children
        .iter()
        .map(|&(p, label, ordinal, c)| exact::Edge { from: p, to: c, role: format!("child:{label}:{ordinal}") })
        .collect();
    edges.extend(spec.edges.iter().map(|&(s, role, label, t)| exact::Edge {
        from: s,
        to: t,
        role: format!("edge:{}:{label}", role.as_str()),
    }));
    edges.sort();
    exact::Graph { labels, edges }
}

fn exact_equal(a: &exact::Graph, b: &exact::Graph) -> Option<bool> {
    let canon = |g: &exact::Graph| match exact::canonicalize(g, exact::Budget::default()) {
        exact::Outcome::Exact { canonical, .. } => Some(canonical),
        _ => None,
    };
    Some(canon(a)? == canon(b)?)
}

/// Whether 1-WL colour refinement (over labels and edges, both directions)
/// tells the two graphs apart. Equal hashes on a pair it cannot tell apart are
/// the documented limit, not a defect.
fn wl_distinguishes(a: &exact::Graph, b: &exact::Graph) -> bool {
    let offset = a.labels.len();
    let mut labels = a.labels.clone();
    labels.extend(b.labels.iter().cloned());
    let mut edges = a.edges.clone();
    edges.extend(b.edges.iter().map(|e| exact::Edge { from: e.from + offset, to: e.to + offset, role: e.role.clone() }));
    let rank = |keys: Vec<String>| -> Vec<usize> {
        let sorted: BTreeMap<String, usize> = keys.iter().cloned().map(|k| (k, 0)).collect();
        let index: BTreeMap<String, usize> = sorted.into_keys().enumerate().map(|(i, k)| (k, i)).collect();
        keys.iter().map(|k| index[k]).collect()
    };
    let mut colours = rank(labels.clone());
    for _ in 0..=labels.len() {
        let signatures: Vec<String> = (0..labels.len())
            .map(|v| {
                let mut out: Vec<_> = edges.iter().filter(|e| e.from == v).map(|e| (&e.role, colours[e.to])).collect();
                let mut inc: Vec<_> = edges.iter().filter(|e| e.to == v).map(|e| (&e.role, colours[e.from])).collect();
                out.sort();
                inc.sort();
                format!("{}|{:?}|{:?}", colours[v], out, inc)
            })
            .collect();
        colours = rank(signatures);
    }
    let mut left: Vec<_> = colours[..offset].to_vec();
    let mut right: Vec<_> = colours[offset..].to_vec();
    left.sort();
    right.sort();
    left != right
}

fn address(g: &Graph, facet: &[Dimension]) -> Digest {
    let policy = HashPolicy::new("oracle/1", Keys::Free).unwrap();
    let facet_ref = Facet::new(policy.policy_id(), facet.iter().copied()).unwrap();
    let request = DigestRequest::new(g.graph_key.clone(), policy.clone(), DIMS).unwrap();
    let hashes = digest_graph(&request, g).unwrap().hashes;
    facet_address(&policy, g, &hashes, &facet_ref).unwrap().address_digest()
}

#[derive(Default)]
struct Tally {
    pairs: usize,
    skipped: usize,
    /// Equal meaning, different addresses: the hash sees something it should forget.
    split: Vec<String>,
    /// Different meaning, equal addresses, and refinement can tell them apart.
    false_agreement: Vec<String>,
}

fn facets(sizes: &[usize]) -> Vec<Vec<Dimension>> {
    let mut out = Vec::new();
    for mask in 1u8..(1 << DIMS.len()) {
        let facet: Vec<Dimension> =
            DIMS.iter().enumerate().filter(|(i, _)| mask & (1 << i) != 0).map(|(_, d)| *d).collect();
        if sizes.contains(&facet.len()) {
            out.push(facet);
        }
    }
    out
}

fn run(seed: u64, pairs: usize, sizes: &[usize]) -> Tally {
    let mut rng = Rng(seed);
    let mut tally = Tally::default();
    for _ in 0..pairs {
        let a = random_spec(&mut rng);
        let b = mutate(&mut rng, &a);
        let (ga, gb) = (catalog_graph(&a), catalog_graph(&b));
        for facet in facets(sizes) {
            tally.pairs += 1;
            let (ka, kb) = (kept(&a, &facet), kept(&b, &facet));
            let Some(same) = exact_equal(&ka, &kb) else {
                tally.skipped += 1;
                continue;
            };
            let equal_address = address(&ga, &facet) == address(&gb, &facet);
            let case = || format!("facet {facet:?}\n  A {a:?}\n  B {b:?}");
            if same && !equal_address {
                tally.split.push(case());
            }
            if !same && equal_address && wl_distinguishes(&ka, &kb) {
                tally.false_agreement.push(case());
            }
        }
    }
    tally
}

fn report(tally: &Tally) -> String {
    let mut out = format!(
        "{} (pair, facet) checks, {} skipped; {} splits of equal meaning; {} false agreements\n",
        tally.pairs,
        tally.skipped,
        tally.split.len(),
        tally.false_agreement.len()
    );
    for case in tally.split.iter().take(3) {
        out.push_str(&format!("split: {case}\n"));
    }
    for case in tally.false_agreement.iter().take(3) {
        out.push_str(&format!("false agreement: {case}\n"));
    }
    out
}

/// One dimension: `digest_d` means `keep_d` (law L2).
#[test]
fn dimension_digests_mean_the_projection() {
    let tally = run(0x5eed_0001, 3000, &[1]);
    assert!(tally.split.is_empty() && tally.false_agreement.is_empty(), "{}", report(&tally));
}

/// Several dimensions: the facet means `keep_F`. Combining per-dimension
/// digests cannot be exact when sets vary in two dimensions of F (review
/// appendix B.1); those graphs take the joint pass (decision 1, option b).
#[test]
fn facet_addresses_mean_the_projection() {
    let tally = run(0x5eed_0002, 3000, &[2, 3]);
    assert!(tally.split.is_empty() && tally.false_agreement.is_empty(), "{}", report(&tally));
}
