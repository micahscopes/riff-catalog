//! Cycle checks and the ordered-children Merkle fold, iterative throughout
//! (producer ASTs get deep; no recursion).

use std::collections::BTreeSet;

use crate::dimension::Dimension;
use crate::encode;
use crate::error::CatalogError;
use crate::hash::DimensionDigests;
use crate::hash::view::IndexedGraph;
use crate::policy::{HashPolicy, ViewMode};
use crate::text::Digest;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Unvisited,
    Visiting,
    Done,
}

/// DFS cycle check. `include_dependency` selects between `Reject` (children +
/// Dependency edges) and `NonRecursiveGraphEdges` (children only).
pub(crate) fn check_acyclic(
    view: &IndexedGraph<'_>,
    include_dependency: bool,
) -> Result<(), CatalogError> {
    let n = view.len();
    let succ: Vec<Vec<u32>> = if include_dependency {
        view.recursive_succ.clone()
    } else {
        (0..n)
            .map(|id| {
                let mut list: Vec<u32> =
                    view.children[id].iter().map(|child| child.child).collect();
                list.sort_unstable();
                list.dedup();
                list
            })
            .collect()
    };

    let mut marks = vec![Mark::Unvisited; n];
    for root in 0..n {
        if marks[root] != Mark::Unvisited {
            continue;
        }
        // (node, next successor position)
        let mut frames: Vec<(u32, usize)> = vec![(root as u32, 0)];
        while let Some(&(node, pos)) = frames.last() {
            if pos == 0 {
                if marks[node as usize] == Mark::Done {
                    frames.pop();
                    continue;
                }
                marks[node as usize] = Mark::Visiting;
            }
            if pos < succ[node as usize].len() {
                let next = succ[node as usize][pos];
                frames.last_mut().unwrap().1 += 1;
                match marks[next as usize] {
                    Mark::Visiting => {
                        return Err(CatalogError::CycleDetected {
                            key: view.key(next as usize).canonical_key(),
                        });
                    }
                    Mark::Done => {}
                    Mark::Unvisited => frames.push((next, 0)),
                }
            } else {
                marks[node as usize] = Mark::Done;
                frames.pop();
            }
        }
    }
    Ok(())
}

/// Per-node, per-dimension Merkle digests over the ordered-children skeleton.
///
/// Child payload ordering is per view mode: identity-bound ties on the child's
/// canonical key; anonymous ties on the child's digest — so no key bytes
/// influence anonymous payloads even for duplicate (ordinal, label) pairs
/// (invariant I5).
pub(crate) fn tree_digests(
    policy: &HashPolicy,
    view: &IndexedGraph<'_>,
    dimensions: &BTreeSet<Dimension>,
    local: &[DimensionDigests],
) -> Result<Vec<DimensionDigests>, CatalogError> {
    let n = view.len();
    let mut tree: Vec<Option<DimensionDigests>> = vec![None; n];
    let mut marks = vec![Mark::Unvisited; n];

    for root in 0..n {
        if tree[root].is_some() {
            continue;
        }
        // (node, entered) — post-order with explicit stack
        let mut frames: Vec<(u32, bool)> = vec![(root as u32, false)];
        while let Some(&(node, entered)) = frames.last() {
            let id = node as usize;
            if !entered {
                if tree[id].is_some() {
                    frames.pop();
                    continue;
                }
                if marks[id] == Mark::Visiting {
                    return Err(CatalogError::CycleDetected {
                        key: view.key(id).canonical_key(),
                    });
                }
                marks[id] = Mark::Visiting;
                frames.last_mut().unwrap().1 = true;
                for child in view.children[id].iter().rev() {
                    if tree[child.child as usize].is_none() {
                        if marks[child.child as usize] == Mark::Visiting {
                            return Err(CatalogError::CycleDetected {
                                key: view.key(child.child as usize).canonical_key(),
                            });
                        }
                        frames.push((child.child, false));
                    }
                }
            } else {
                let mut digests = DimensionDigests::default();
                for dimension in dimensions {
                    digests.insert(
                        *dimension,
                        node_tree_digest(policy, view, local, &tree, id, *dimension)?,
                    );
                }
                marks[id] = Mark::Done;
                tree[id] = Some(digests);
                frames.pop();
            }
        }
    }

    Ok(tree.into_iter().map(|d| d.expect("computed")).collect())
}

fn node_tree_digest(
    policy: &HashPolicy,
    view: &IndexedGraph<'_>,
    local: &[DimensionDigests],
    tree: &[Option<DimensionDigests>],
    id: usize,
    dimension: Dimension,
) -> Result<Digest, CatalogError> {
    // (ordinal, label, child digest, child id) — sort key depends on view mode.
    let mut entries: Vec<(u32, &str, Digest, u32)> = view.children[id]
        .iter()
        .map(|child| {
            let digest = *tree[child.child as usize]
                .as_ref()
                .expect("child computed before parent")
                .get(dimension)
                .expect("dimension computed");
            (child.ordinal, child.label, digest, child.child)
        })
        .collect();
    match policy.view_mode {
        ViewMode::IdentityBound => entries.sort_by(|a, b| {
            (a.0, a.1).cmp(&(b.0, b.1)).then_with(|| {
                view.key(a.3 as usize)
                    .canonical_key()
                    .cmp(&view.key(b.3 as usize).canonical_key())
            })
        }),
        ViewMode::AnonymousShape => {
            entries.sort_by(|a, b| (a.0, a.1, a.2).cmp(&(b.0, b.1, b.2)));
        }
    }

    encode::digest_record(policy, dimension, "node.tree", |bytes| {
        encode::push_digest(bytes, local[id].get(dimension).expect("local computed"));
        encode::push_u32(bytes, entries.len() as u32);
        for (ordinal, label, digest, _) in &entries {
            if dimension == Dimension::Structure {
                encode::push_u32(bytes, *ordinal);
                encode::push_str(bytes, label);
            }
            encode::push_digest(bytes, digest);
        }
    })
}
