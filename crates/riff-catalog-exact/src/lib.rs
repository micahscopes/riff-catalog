//! Bounded exact canonicalization of directed labelled incidence multigraphs.
//! Cycles are edge records, not recursive address-evaluation dependencies.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    pub role: String,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Graph {
    pub labels: Vec<String>,
    pub edges: Vec<Edge>,
}

#[derive(Clone, Copy)]
pub struct Budget {
    pub vertices: usize,
    pub edges: usize,
    pub states: usize,
    pub duration: Duration,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            vertices: 64,
            edges: 256,
            states: 100_000,
            duration: Duration::from_secs(2),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Outcome {
    Exact {
        canonical: Graph,
        /// Original occurrence index to canonical index.
        mapping: Vec<usize>,
        states: usize,
    },
    Unsupported {
        reason: String,
    },
    BudgetExceeded {
        reason: String,
        states: usize,
    },
}

fn ranks<T: Ord + Clone>(keys: &[T]) -> Vec<usize> {
    let map: BTreeMap<_, _> = keys.iter().cloned().map(|k| (k, 0)).collect();
    let map: BTreeMap<_, _> = map
        .into_keys()
        .enumerate()
        .map(|(rank, key)| (key, rank))
        .collect();
    keys.iter().map(|k| map[k]).collect()
}

fn refine(graph: &Graph, mut colors: Vec<usize>) -> Vec<usize> {
    loop {
        let before = colors.iter().max().map_or(0, |n| n + 1);
        let signatures: Vec<_> = (0..graph.labels.len())
            .map(|v| {
                let mut outgoing = Vec::new();
                let mut incoming = Vec::new();
                for edge in &graph.edges {
                    if edge.from == v {
                        outgoing.push((&edge.role, colors[edge.to]));
                    }
                    if edge.to == v {
                        incoming.push((&edge.role, colors[edge.from]));
                    }
                }
                outgoing.sort();
                incoming.sort();
                (colors[v], outgoing, incoming)
            })
            .collect();
        colors = ranks(&signatures);
        if colors.iter().max().map_or(0, |n| n + 1) == before {
            return colors;
        }
    }
}

fn relabel(graph: &Graph, mapping: &[usize]) -> Graph {
    let mut labels = vec![String::new(); graph.labels.len()];
    for (old, &new) in mapping.iter().enumerate() {
        labels[new] = graph.labels[old].clone();
    }
    let mut edges: Vec<_> = graph
        .edges
        .iter()
        .map(|e| Edge {
            from: mapping[e.from],
            to: mapping[e.to],
            role: e.role.clone(),
        })
        .collect();
    edges.sort();
    Graph { labels, edges }
}

/// Verify a supplied bijection against all labels, edge directions, roles and
/// multiplicities. A witness is not accepted merely because its hashes match.
pub fn verify(left: &Graph, right: &Graph, mapping: &[usize]) -> bool {
    let n = left.labels.len();
    if n != right.labels.len()
        || mapping.len() != n
        || left.edges.iter().any(|e| e.from >= n || e.to >= n)
        || right.edges.iter().any(|e| e.from >= n || e.to >= n)
    {
        return false;
    }
    let mut sorted = mapping.to_vec();
    sorted.sort();
    if sorted != (0..n).collect::<Vec<_>>() {
        return false;
    }
    let mut expected = right.clone();
    expected.edges.sort();
    relabel(left, mapping) == expected
}

pub fn canonicalize(graph: &Graph, budget: Budget) -> Outcome {
    let n = graph.labels.len();
    if graph.edges.iter().any(|e| e.from >= n || e.to >= n) {
        return Outcome::Unsupported {
            reason: "edge endpoint outside graph".into(),
        };
    }
    if n > budget.vertices || graph.edges.len() > budget.edges {
        return Outcome::BudgetExceeded {
            reason: "graph size budget".into(),
            states: 0,
        };
    }
    // Bound label storage before refinement clones any data.
    if graph.labels.iter().map(String::len).sum::<usize>()
        + graph.edges.iter().map(|e| e.role.len()).sum::<usize>()
        > 1_048_576
    {
        return Outcome::BudgetExceeded {
            reason: "label byte budget".into(),
            states: 0,
        };
    }
    struct Search<'a> {
        graph: &'a Graph,
        budget: Budget,
        start: Instant,
        states: usize,
        best: Option<(Graph, Vec<usize>)>,
    }
    impl Search<'_> {
        fn visit(&mut self, colors: Vec<usize>) -> Result<(), ()> {
            if self.states >= self.budget.states || self.start.elapsed() >= self.budget.duration {
                return Err(());
            }
            self.states += 1;
            let colors = refine(self.graph, colors);
            let mut cells: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
            for (v, &color) in colors.iter().enumerate() {
                cells.entry(color).or_default().push(v);
            }
            if let Some(cell) = cells.values().find(|cell| cell.len() > 1) {
                for &chosen in cell {
                    let keys: Vec<_> = colors
                        .iter()
                        .enumerate()
                        .map(|(v, &c)| (c, usize::from(v == chosen)))
                        .collect();
                    self.visit(ranks(&keys))?;
                }
            } else {
                let candidate = relabel(self.graph, &colors);
                if self.best.as_ref().is_none_or(|(best, _)| candidate < *best) {
                    self.best = Some((candidate, colors));
                }
            }
            Ok(())
        }
    }
    let mut search = Search {
        graph,
        budget,
        start: Instant::now(),
        states: 0,
        best: None,
    };
    if search.visit(ranks(&graph.labels)).is_err() {
        return Outcome::BudgetExceeded {
            reason: "exact search state/time budget; no partial canonical answer".into(),
            states: search.states,
        };
    }
    let (canonical, mapping) = search.best.unwrap();
    Outcome::Exact {
        canonical,
        mapping,
        states: search.states,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn canon(g: &Graph) -> Graph {
        match canonicalize(g, Budget::default()) {
            Outcome::Exact {
                canonical, mapping, ..
            } => {
                assert!(verify(g, &canonical, &mapping));
                canonical
            }
            o => panic!("{o:?}"),
        }
    }
    fn graph(n: usize, pairs: &[(usize, usize)]) -> Graph {
        Graph {
            labels: vec!["node".into(); n],
            edges: pairs
                .iter()
                .map(|&(from, to)| Edge {
                    from,
                    to,
                    role: "flow".into(),
                })
                .collect(),
        }
    }
    fn permutations(values: &mut [usize], i: usize, f: &mut impl FnMut(&[usize])) {
        if i == values.len() {
            f(values);
            return;
        }
        for j in i..values.len() {
            values.swap(i, j);
            permutations(values, i + 1, f);
            values.swap(i, j);
        }
    }
    #[test]
    fn cycles_and_transport_renaming() {
        let g = graph(4, &[(0, 1), (1, 2), (2, 0), (2, 3)]);
        let expected = canon(&g);
        permutations(&mut [0, 1, 2, 3], 0, &mut |p| {
            let mut changed = relabel(&g, p);
            changed.edges.reverse();
            assert_eq!(canon(&changed), expected);
        });
    }
    #[test]
    fn labels_roles_direction_and_multiplicity_are_content() {
        let g = graph(3, &[(0, 1), (1, 2), (2, 0)]);
        let expected = canon(&g);
        let mut changed = g.clone();
        changed.edges.push(changed.edges[0].clone());
        assert_ne!(canon(&changed), expected);
        changed = g.clone();
        changed.labels[0] = "entry".into();
        assert_ne!(canon(&changed), expected);
        changed = g.clone();
        changed.edges[0].role = "data".into();
        assert_ne!(canon(&changed), expected);
        changed = g.clone();
        changed.edges[0].from = 1;
        changed.edges[0].to = 0;
        assert_ne!(canon(&changed), expected);
    }
    #[test]
    fn exhaustive_small_graph_oracle_agrees() {
        // All 64 directed graphs on three vertices without self edges.
        // Oracle is raw enumeration of all bijections, not refinement/search.
        let pairs = [(0, 1), (0, 2), (1, 0), (1, 2), (2, 0), (2, 1)];
        let graphs: Vec<_> = (0..64)
            .map(|mask| {
                graph(
                    3,
                    &pairs
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| mask & (1 << i) != 0)
                        .map(|(_, p)| *p)
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        let canonical: Vec<_> = graphs.iter().map(canon).collect();
        for (i, left) in graphs.iter().enumerate() {
            for (j, right) in graphs.iter().enumerate() {
                let mut isomorphic = false;
                permutations(&mut [0, 1, 2], 0, &mut |p| {
                    let labels = (0..3).all(|v| left.labels[v] == right.labels[p[v]]);
                    let mut mapped: Vec<_> = left
                        .edges
                        .iter()
                        .map(|e| (p[e.from], p[e.to], &e.role))
                        .collect();
                    let mut expected: Vec<_> = right
                        .edges
                        .iter()
                        .map(|e| (e.from, e.to, &e.role))
                        .collect();
                    mapped.sort();
                    expected.sort();
                    isomorphic |= labels && mapped == expected;
                });
                assert_eq!(canonical[i] == canonical[j], isomorphic, "{i} vs {j}");
            }
        }
    }
    #[test]
    fn regular_wl_tie_does_not_collapse_nonisomorphic_graphs() {
        let undirected = |pairs: &[(usize, usize)]| {
            graph(
                6,
                &pairs
                    .iter()
                    .flat_map(|&(a, b)| [(a, b), (b, a)])
                    .collect::<Vec<_>>(),
            )
        };
        let prism = undirected(&[
            (0, 1),
            (1, 2),
            (2, 0),
            (3, 4),
            (4, 5),
            (5, 3),
            (0, 3),
            (1, 4),
            (2, 5),
        ]);
        let bipartite = undirected(&[
            (0, 3),
            (0, 4),
            (0, 5),
            (1, 3),
            (1, 4),
            (1, 5),
            (2, 3),
            (2, 4),
            (2, 5),
        ]);
        assert_eq!(refine(&prism, vec![0; 6]), refine(&bipartite, vec![0; 6]));
        assert_ne!(canon(&prism), canon(&bipartite));
    }

    #[test]
    fn budget_and_bad_witness_never_establish_equality() {
        let g = graph(3, &[(0, 1), (1, 2), (2, 0)]);
        assert!(matches!(
            canonicalize(
                &g,
                Budget {
                    states: 1,
                    ..Default::default()
                }
            ),
            Outcome::BudgetExceeded { .. }
        ));
        assert!(!verify(&g, &g, &[0, 0, 0]));
        let bad = graph(2, &[(0, 3)]);
        assert!(matches!(
            canonicalize(&bad, Budget::default()),
            Outcome::Unsupported { .. }
        ));
    }
}
