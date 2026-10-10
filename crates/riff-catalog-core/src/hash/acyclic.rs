//! Cycle checks for the `Reject` and `NonRecursiveGraphEdges` policies, iterative
//! (producer ASTs get deep; no recursion).

use crate::error::CatalogError;
use crate::hash::view::IndexedGraph;

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
