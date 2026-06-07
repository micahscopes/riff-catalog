//! Iterative Tarjan over the recursive-edge successor lists.
//!
//! Components are emitted in reverse topological order of the condensation
//! (every component a node can reach is emitted before the node's own), with
//! members sorted by node id. Emission order and indices are NEVER hashed —
//! they only order computation and reporting (invariant I5).

pub(crate) fn strongly_connected_components(succ: &[Vec<u32>]) -> Vec<Vec<u32>> {
    let n = succ.len();
    const UNDEF: u32 = u32::MAX;
    let mut index = vec![UNDEF; n];
    let mut lowlink = vec![0u32; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<u32> = Vec::new();
    let mut next_index = 0u32;
    let mut components: Vec<Vec<u32>> = Vec::new();

    for root in 0..n {
        if index[root] != UNDEF {
            continue;
        }
        // (node, next successor position)
        let mut frames: Vec<(u32, usize)> = vec![(root as u32, 0)];
        while let Some(&(node, pos)) = frames.last() {
            let id = node as usize;
            if pos == 0 {
                index[id] = next_index;
                lowlink[id] = next_index;
                next_index += 1;
                stack.push(node);
                on_stack[id] = true;
            }
            if pos < succ[id].len() {
                let next = succ[id][pos];
                frames.last_mut().unwrap().1 += 1;
                if index[next as usize] == UNDEF {
                    frames.push((next, 0));
                } else if on_stack[next as usize] {
                    lowlink[id] = lowlink[id].min(index[next as usize]);
                }
            } else {
                if lowlink[id] == index[id] {
                    let mut component = Vec::new();
                    loop {
                        let member = stack.pop().expect("tarjan stack holds component");
                        on_stack[member as usize] = false;
                        component.push(member);
                        if member == node {
                            break;
                        }
                    }
                    component.sort_unstable();
                    components.push(component);
                }
                frames.pop();
                if let Some(&(parent, _)) = frames.last() {
                    let parent = parent as usize;
                    lowlink[parent] = lowlink[parent].min(lowlink[id]);
                }
            }
        }
    }
    components
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn singletons_and_cycle() {
        // 0 -> 1 -> 2 -> 1, 0 -> 3
        let succ = vec![vec![1, 3], vec![2], vec![1], vec![]];
        let comps = strongly_connected_components(&succ);
        // {1,2} must be one component; 0 emitted last (it reaches everything)
        assert!(comps.contains(&vec![1, 2]));
        assert_eq!(comps.last().unwrap(), &vec![0]);
        assert_eq!(comps.len(), 3);
    }

    #[test]
    fn emission_is_reverse_topological() {
        // 0 -> 1 -> 2 (a chain): 2 first, then 1, then 0
        let succ = vec![vec![1], vec![2], vec![]];
        let comps = strongly_connected_components(&succ);
        assert_eq!(comps, vec![vec![2], vec![1], vec![0]]);
    }

    #[test]
    fn self_loop_is_a_component() {
        let succ = vec![vec![0]];
        let comps = strongly_connected_components(&succ);
        assert_eq!(comps, vec![vec![0]]);
    }
}
