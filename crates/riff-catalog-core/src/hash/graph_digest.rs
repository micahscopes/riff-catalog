//! Final `graph.full` assembly per dimension.
//!
//! Meaning: `graph.full` for dimension d names the graph that keeps only d's
//! facts on the skeleton every dimension shares (nodes, child edges with label
//! and ordinal, flat and Dependency edges with role and label), up to renaming
//! of keys in free (anonymous) mode, and exactly in bound (identity) mode.
//!
//! A node's per-node digest (`tree`) says what is below it, not where it is.
//! So the graph digest refines colours over the whole graph: each node starts
//! from its `tree` digest and repeatedly takes in the colours of its neighbours
//! along every edge, in both directions, until the partition into colours
//! stops splitting. Every node then carries its full context: below, above and
//! what it is linked to. Without this, a flat edge pointing at one of two
//! equal-content nodes could not be told from one pointing at the other
//! (found by the facet oracle, `tests/hash_oracle.rs`).
//!
//! The digest is the bag of final colours plus the bag of edge records written
//! with colours. Colour refinement cannot separate some regular graphs; equal
//! digests on such pairs are the documented limit (invariant I5).
//!
//! Only the skeleton and d's facts enter, so editing a fact of another
//! dimension never moves this digest (invariant I2). In free mode no key bytes
//! enter (I5); in bound mode keys are already in every `local` digest, and the
//! graph key is never hashed, so units compare across worlds.

use crate::dimension::Dimension;
use crate::encode;
use crate::error::CatalogError;
use crate::graph::{EdgeRole, Graph};
use crate::hash::DimensionDigests;
use crate::hash::view::IndexedGraph;
use crate::policy::{HashPolicy, Keys};
use crate::text::Digest;

/// One edge of the skeleton, in id space: child edges carry their ordinal,
/// flat and Dependency edges carry ordinal 0 and their role.
struct SkeletonEdge<'g> {
    role: &'g str,
    label: &'g str,
    ordinal: u32,
    src: u32,
    dst: u32,
}

fn skeleton_edges<'g>(graph: &'g Graph, view: &IndexedGraph<'g>) -> Vec<SkeletonEdge<'g>> {
    let mut edges = Vec::new();
    for (parent, list) in view.children.iter().enumerate() {
        for child in list {
            edges.push(SkeletonEdge {
                role: "child",
                label: child.label,
                ordinal: child.ordinal,
                src: parent as u32,
                dst: child.child,
            });
        }
    }
    for edge in &graph.edges {
        if edge.role == EdgeRole::Origin {
            continue;
        }
        edges.push(SkeletonEdge {
            role: edge.role.as_str(),
            label: edge.label.as_str(),
            ordinal: 0,
            src: view.id_of(&edge.source).expect("validated"),
            dst: view.id_of(&edge.target).expect("validated"),
        });
    }
    edges
}

fn edge_record(edge: &SkeletonEdge<'_>, end: &Digest) -> Vec<u8> {
    let mut record = Vec::new();
    encode::push_str(&mut record, edge.role);
    encode::push_str(&mut record, edge.label);
    encode::push_u32(&mut record, edge.ordinal);
    encode::push_digest(&mut record, end);
    record
}

fn class_count(colours: &[Digest]) -> usize {
    let mut sorted: Vec<&Digest> = colours.iter().collect();
    sorted.sort_unstable();
    sorted.dedup();
    sorted.len()
}

/// Refine colours to a fixpoint over the skeleton edges, both directions,
/// then hash the bag of colours and the edges written with colours. Returns
/// the graph digest and the final colour of every node.
fn refine_and_digest(
    policy: &HashPolicy,
    bin: &str,
    edges: &[SkeletonEdge<'_>],
    mut colours: Vec<Digest>,
) -> Result<(Digest, Vec<Digest>), CatalogError> {
    let n = colours.len();
    let mut outgoing: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut incoming: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (index, edge) in edges.iter().enumerate() {
        outgoing[edge.src as usize].push(index);
        incoming[edge.dst as usize].push(index);
    }
    let mut classes = class_count(&colours);

    // The new colour includes the old one, so the partition only splits; it is
    // stable once the class count stops growing. At most n rounds.
    for _ in 0..n {
        let mut next = Vec::with_capacity(n);
        for id in 0..n {
            let out_records = outgoing[id]
                .iter()
                .map(|&e| edge_record(&edges[e], &colours[edges[e].dst as usize]))
                .collect();
            let in_records = incoming[id]
                .iter()
                .map(|&e| edge_record(&edges[e], &colours[edges[e].src as usize]))
                .collect();
            next.push(encode::digest_record_in(policy, bin, "graph.colour", |bytes| {
                encode::push_digest(bytes, &colours[id]);
                encode::push_sorted_records(bytes, out_records);
                encode::push_sorted_records(bytes, in_records);
            })?);
        }
        let next_classes = class_count(&next);
        colours = next;
        if next_classes == classes {
            break;
        }
        classes = next_classes;
    }

    let mut sorted_colours = colours.clone();
    sorted_colours.sort_unstable();
    let edge_records = edges
        .iter()
        .map(|edge| {
            let mut record = edge_record(edge, &colours[edge.src as usize]);
            encode::push_digest(&mut record, &colours[edge.dst as usize]);
            record
        })
        .collect();

    let digest = encode::digest_record_in(policy, bin, "graph.full", |bytes| {
        encode::push_u32(bytes, sorted_colours.len() as u32);
        for colour in &sorted_colours {
            encode::push_digest(bytes, colour);
        }
        encode::push_sorted_records(bytes, edge_records);
    })?;
    Ok((digest, colours))
}

/// `graph.full` for one dimension, and the final colour of every node.
pub(crate) fn graph_digest_for_dimension(
    policy: &HashPolicy,
    graph: &Graph,
    view: &IndexedGraph<'_>,
    final_digests: &[DimensionDigests],
    dimension: Dimension,
) -> Result<(Digest, Vec<Digest>), CatalogError> {
    let colours = (0..view.len())
        .map(|id| *final_digests[id].get(dimension).expect("dimension computed"))
        .collect();
    refine_and_digest(policy, dimension.as_str(), &skeleton_edges(graph, view), colours)
}

/// The bare shape: the same refinement with no facts and no kinds, only the
/// skeleton (and keys, with bound keys). Its digest names the coarsest atom,
/// "same shape, nothing else"; its colours tell which nodes the skeleton
/// alone cannot tell apart.
pub(crate) fn shape_digest(
    policy: &HashPolicy,
    graph: &Graph,
    view: &IndexedGraph<'_>,
) -> Result<(Digest, Vec<Digest>), CatalogError> {
    let colours = (0..view.len())
        .map(|id| {
            encode::digest_record_in(policy, "shape", "shape.init", |bytes| {
                if policy.keys == Keys::Bound {
                    encode::push_node_key(bytes, view.key(id));
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    refine_and_digest(policy, "shape", &skeleton_edges(graph, view), colours)
}
