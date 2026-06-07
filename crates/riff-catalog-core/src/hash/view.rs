//! Indexed view of a graph for the hashing algorithms.
//!
//! All internal algorithms (Tarjan, WL, tree walks) run on `u32` node ids.
//! INVARIANT (I5): no `u32` id ever enters a hash payload — ids order
//! traversal, digests and (in identity mode) canonical keys order payloads.

use std::collections::BTreeMap;

use crate::graph::{Graph, Node};
use crate::key::NodeKey;

pub(crate) struct ChildRef<'g> {
    pub label: &'g str,
    pub ordinal: u32,
    pub child: u32,
}

/// A recursive edge: a child edge (role "child", real ordinal) or a
/// Dependency-role edge (role "dependency", ordinal 0). The role string
/// disambiguates the defaulted ordinal.
pub(crate) struct RecEdge<'g> {
    pub role: &'static str,
    pub label: &'g str,
    pub ordinal: u32,
    pub src: u32,
    pub dst: u32,
}

pub(crate) struct IndexedGraph<'g> {
    keys: Vec<&'g NodeKey>,
    nodes: Vec<&'g Node>,
    ids: BTreeMap<&'g NodeKey, u32>,
    /// Children per parent id, sorted (ordinal, label, child canonical key) —
    /// traversal order only; payload-time ordering is decided per view mode.
    pub children: Vec<Vec<ChildRef<'g>>>,
    /// All recursive edges (children + Dependency), in deterministic order.
    pub recursive: Vec<RecEdge<'g>>,
    /// Successor ids over recursive edges, per node id (deduped).
    pub recursive_succ: Vec<Vec<u32>>,
}

impl<'g> IndexedGraph<'g> {
    pub fn build(graph: &'g Graph) -> Self {
        let mut keys: Vec<&'g NodeKey> = graph.nodes.keys().collect();
        keys.sort_by_key(|key| key.canonical_key());
        let ids: BTreeMap<&'g NodeKey, u32> = keys
            .iter()
            .enumerate()
            .map(|(id, key)| (*key, id as u32))
            .collect();
        let nodes: Vec<&'g Node> = keys.iter().map(|key| &graph.nodes[*key]).collect();

        let mut children: Vec<Vec<ChildRef<'g>>> = (0..keys.len()).map(|_| Vec::new()).collect();
        for child in &graph.children {
            let parent = ids[&child.parent];
            children[parent as usize].push(ChildRef {
                label: child.label.as_str(),
                ordinal: child.ordinal,
                child: ids[&child.child],
            });
        }
        for list in &mut children {
            list.sort_by(|a, b| {
                (a.ordinal, a.label)
                    .cmp(&(b.ordinal, b.label))
                    .then_with(|| a.child.cmp(&b.child))
            });
        }

        let mut recursive: Vec<RecEdge<'g>> = Vec::new();
        for (parent, list) in children.iter().enumerate() {
            for child in list {
                recursive.push(RecEdge {
                    role: "child",
                    label: child.label,
                    ordinal: child.ordinal,
                    src: parent as u32,
                    dst: child.child,
                });
            }
        }
        for edge in &graph.edges {
            if edge.role.is_recursive() {
                recursive.push(RecEdge {
                    role: "dependency",
                    label: edge.label.as_str(),
                    ordinal: 0,
                    src: ids[&edge.source],
                    dst: ids[&edge.target],
                });
            }
        }

        let mut recursive_succ: Vec<Vec<u32>> = vec![Vec::new(); keys.len()];
        for edge in &recursive {
            recursive_succ[edge.src as usize].push(edge.dst);
        }
        for list in &mut recursive_succ {
            list.sort_unstable();
            list.dedup();
        }

        Self {
            keys,
            nodes,
            ids,
            children,
            recursive,
            recursive_succ,
        }
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn key(&self, id: usize) -> &'g NodeKey {
        self.keys[id]
    }

    pub fn node(&self, id: usize) -> &'g Node {
        self.nodes[id]
    }

    pub fn id_of(&self, key: &NodeKey) -> Option<u32> {
        self.ids.get(key).copied()
    }
}
