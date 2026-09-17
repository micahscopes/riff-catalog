//! Finite examples separating SCC membership, WL cells, and exact structure.

use anyhow::Result;
use riff_catalog_core::{
    CyclePolicy, DigestRequest, Dimension, EdgeRole, EntityKey, Facet, Graph, GraphKey, HashPolicy,
    NodeKey, ViewMode, digest_graph,
};
use serde_json::{Value, json};

pub fn graph(edges: &[(usize, usize)], nodes: usize, owner: &str) -> Graph {
    let entity = EntityKey::new("wl.pilot", owner, "graph").unwrap();
    let mut graph = Graph::new(GraphKey::new(entity.clone(), "graph").unwrap());
    let keys: Vec<_> = (0..nodes)
        .map(|i| NodeKey::derived(entity.clone(), format!("v:{i}")).unwrap())
        .collect();
    for (i, key) in keys.iter().enumerate() {
        graph.add_node(key.clone(), "vertex").unwrap();
        // A label-preserving facet can distinguish these semantic roles.
        // The structure facet explicitly erases them.
        graph
            .add_field(key, Dimension::Names, "role", format!("role:{i}"))
            .unwrap();
    }
    for &(a, b) in edges {
        graph
            .add_edge(&keys[a], "adjacent", &keys[b], EdgeRole::Dependency)
            .unwrap();
        graph
            .add_edge(&keys[b], "adjacent", &keys[a], EdgeRole::Dependency)
            .unwrap();
    }
    graph
}

pub fn triangles_per_vertex(edges: &[(usize, usize)], n: usize) -> Vec<usize> {
    let adjacent = |a, b| edges.contains(&(a, b)) || edges.contains(&(b, a));
    let mut counts = vec![0; n];
    for a in 0..n {
        for b in a + 1..n {
            for c in b + 1..n {
                if adjacent(a, b) && adjacent(b, c) && adjacent(a, c) {
                    counts[a] += 1;
                    counts[b] += 1;
                    counts[c] += 1;
                }
            }
        }
    }
    counts
}

/// Independent bounded graph-isomorphism oracle: least adjacency matrix over
/// every vertex permutation, with no hashing or WL involved. Only n <= 8.
pub fn exact_form(edges: &[(usize, usize)], n: usize) -> u64 {
    assert!(n <= 8);
    let mut adjacency = vec![vec![false; n]; n];
    for &(a, b) in edges {
        adjacency[a][b] = true;
        adjacency[b][a] = true;
    }
    fn visit(order: &mut [usize], at: usize, adjacency: &[Vec<bool>], best: &mut u64) {
        if at == order.len() {
            let mut bits = 0u64;
            for &a in order.iter() {
                for &b in order.iter() {
                    bits = (bits << 1) | u64::from(adjacency[a][b]);
                }
            }
            *best = (*best).min(bits);
        } else {
            for i in at..order.len() {
                order.swap(at, i);
                visit(order, at + 1, adjacency, best);
                order.swap(at, i);
            }
        }
    }
    let mut best = u64::MAX;
    visit(&mut (0..n).collect::<Vec<_>>(), 0, &adjacency, &mut best);
    best
}

pub fn prism() -> Vec<(usize, usize)> {
    vec![
        (0, 1),
        (1, 2),
        (2, 0),
        (3, 4),
        (4, 5),
        (5, 3),
        (0, 3),
        (1, 4),
        (2, 5),
    ]
}

pub fn bipartite() -> Vec<(usize, usize)> {
    (0..3).flat_map(|a| (3..6).map(move |b| (a, b))).collect()
}

pub fn mixed_roles() -> Vec<(usize, usize)> {
    let mut edges = Vec::new();
    for a in 0usize..8 {
        for b in a + 1..8 {
            if (a ^ b).count_ones() == 1 && (a, b) != (0, 1) && (a, b) != (6, 7) {
                edges.push((a, b));
            }
        }
    }
    edges.extend([(0, 6), (1, 7)]);
    edges
}

/// Count edge-preserving maps, allowing repeated target vertices. These are
/// homomorphisms, not counts of distinct embedded subgraphs.
pub fn homomorphisms(
    pattern: &[(usize, usize)],
    pattern_nodes: usize,
    target: &[(usize, usize)],
    target_nodes: usize,
) -> usize {
    fn visit(
        at: usize,
        mapping: &mut [usize],
        pattern: &[(usize, usize)],
        target: &[(usize, usize)],
        n: usize,
    ) -> usize {
        if at == mapping.len() {
            return 1;
        }
        let mut count = 0;
        for candidate in 0..n {
            mapping[at] = candidate;
            if pattern.iter().all(|&(a, b)| {
                a > at
                    || b > at
                    || target.contains(&(mapping[a], mapping[b]))
                    || target.contains(&(mapping[b], mapping[a]))
            }) {
                count += visit(at + 1, mapping, pattern, target, n);
            }
        }
        count
    }
    visit(
        0,
        &mut vec![0; pattern_nodes],
        pattern,
        target,
        target_nodes,
    )
}

fn observation(graph: &Graph) -> Result<Value> {
    let policy = HashPolicy::new(
        "pilot:wl-observation/1",
        ViewMode::AnonymousShape,
        CyclePolicy::CondenseScc,
    )?;
    let result = digest_graph(
        &DigestRequest::all_dimensions(graph.graph_key.clone(), policy.clone()),
        graph,
    )?;
    let coarse = result
        .hashes
        .facet_address(&Facet::structure_only(policy.policy_id()))?
        .address_digest();
    let fine = result
        .hashes
        .facet_address(&Facet::full(policy.policy_id()))?
        .address_digest();
    let mut cells = std::collections::BTreeMap::new();
    for component in &result.hashes.components {
        for colors in component.member_colors.values() {
            *cells
                .entry(*colors.get(Dimension::Structure).unwrap())
                .or_insert(0usize) += 1;
        }
    }
    Ok(
        json!({ "coarse": coarse.to_hex(), "labels_retained": fine.to_hex(),
        "sccs": result.hashes.components.len(), "wl_cell_sizes": cells.values().collect::<Vec<_>>() }),
    )
}

pub fn run() -> Result<Value> {
    let left = prism();
    let right = bipartite();
    let left_observation = observation(&graph(&left, 6, "prism"))?;
    let right_observation = observation(&graph(&right, 6, "bipartite"))?;
    anyhow::ensure!(
        left_observation["coarse"] == right_observation["coarse"],
        "expected coarse match"
    );
    anyhow::ensure!(
        left_observation["labels_retained"] != right_observation["labels_retained"],
        "labels must separate examples"
    );
    anyhow::ensure!(
        exact_form(&left, 6) != exact_form(&right, 6),
        "exact oracle must reject isomorphism"
    );
    let mixed = mixed_roles();
    let mixed_observation = observation(&graph(&mixed, 8, "mixed"))?;
    let triangle_counts = triangles_per_vertex(&mixed, 8);
    anyhow::ensure!(
        mixed_observation["wl_cell_sizes"] == json!([8]),
        "expected one coarse vertex cell"
    );
    anyhow::ensure!(
        triangle_counts[0] != triangle_counts[2],
        "triangle counts witness distinct orbits"
    );
    let mut tree_queries = Vec::new();
    for (name, n, pattern) in [
        ("path_3", 3, vec![(0, 1), (1, 2)]),
        ("star_4", 4, vec![(0, 1), (0, 2), (0, 3)]),
        ("path_5", 5, vec![(0, 1), (1, 2), (2, 3), (3, 4)]),
    ] {
        let a = homomorphisms(&pattern, n, &left, 6);
        let b = homomorphisms(&pattern, n, &right, 6);
        anyhow::ensure!(a == b, "tree query must agree on these regular graphs");
        tree_queries.push(json!({ "query": name, "prism": a, "bipartite": b }));
    }
    let triangle = vec![(0, 1), (1, 2), (2, 0)];
    let triangle_maps = [
        homomorphisms(&triangle, 3, &left, 6),
        homomorphisms(&triangle, 3, &right, 6),
    ];
    anyhow::ensure!(
        triangle_maps == [12, 0],
        "triangle maps distinguish this class"
    );
    Ok(json!({
        "prism": left_observation, "bipartite": right_observation,
        "same_coarse_address": true, "exact_isomorphic": false,
        "shared_answers": { "vertices": 6, "undirected_edges": 9, "degree": 3 },
        "triangle_count": { "prism": 2, "bipartite": 0 },
        "tree_homomorphism_queries": tree_queries,
        "triangle_homomorphisms": triangle_maps,
        "mixed_role_scc": mixed_observation,
        "mixed_role_triangles_per_vertex": triangle_counts,
        "lesson": "A coarse address groups equal observations. Only queries constant on that class can reuse one answer."
    }))
}
