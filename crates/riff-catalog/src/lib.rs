//! riff-catalog: a thin facade over the engine.
//!
//! It re-exports the core API (so producers depend on one crate), adds a
//! solc-free graph ingest, and adds weighted-containment similarity over the
//! per-node Merkle subtree digests. The similarity is the basis for "this shape
//! is contained in that one", which is how modified variants are found: a fork
//! that edited part of a function still contains most of its subtree shapes.

pub use riff_catalog_core::*;

use std::collections::BTreeSet;

/// Ingest a graph directly, with no solc and no Solidity in sight: compute the
/// all-dimension digests under the anonymous-shape policy at the given level.
/// The level names the lowering contract (two producers at one level must
/// encode identically). This is the entry a producer such as fe would call.
pub fn ingest_graph(graph: &Graph, level: &str) -> Result<GraphHashes, CatalogError> {
    let policy = HashPolicy::new(level, ViewMode::AnonymousShape, CyclePolicy::CondenseScc)?;
    let request = DigestRequest::all_dimensions(graph.graph_key.clone(), policy);
    Ok(digest_graph(&request, graph)?.hashes)
}

/// The multiset of per-node subtree digests at a dimension, as a set of distinct
/// digests (uniform node weights; size weighting is a future refinement).
fn subtree_digests(hashes: &GraphHashes, dim: Dimension) -> BTreeSet<Digest> {
    hashes
        .nodes
        .values()
        .filter_map(|n| n.tree.get(dim).copied())
        .collect()
}

/// Weighted containment Cw(a in b) at a dimension: the fraction of a's nodes
/// whose subtree digest also occurs among b's nodes. 1.0 means every shape in a
/// is present in b (a is structurally contained in b); lower values measure how
/// much of a survives in b, which is what catches an edited fork.
pub fn containment(a: &GraphHashes, b: &GraphHashes, dim: Dimension) -> f64 {
    let in_b = subtree_digests(b, dim);
    let total = a.nodes.len();
    if total == 0 {
        return 0.0;
    }
    let shared = a
        .nodes
        .values()
        .filter(|n| n.tree.get(dim).is_some_and(|d| in_b.contains(d)))
        .count();
    shared as f64 / total as f64
}

/// Symmetric similarity: Jaccard over the distinct per-node subtree digests at a
/// dimension (intersection over union).
pub fn similarity(a: &GraphHashes, b: &GraphHashes, dim: Dimension) -> f64 {
    let da = subtree_digests(a, dim);
    let db = subtree_digests(b, dim);
    let union = da.union(&db).count();
    if union == 0 {
        return 0.0;
    }
    da.intersection(&db).count() as f64 / union as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use riff_catalog_music::{Note, encode_riff};

    fn ingest(name: &str, spec: &[(i32, u32)]) -> GraphHashes {
        let notes: Vec<Note> = spec.iter().map(|&(pitch, dur)| Note { pitch, dur }).collect();
        let (_key, graph) = encode_riff(name, &notes).unwrap();
        ingest_graph(&graph, "riff.v1").unwrap()
    }

    #[test]
    fn identical_shapes_fully_contain_each_other() {
        // different names, same shape: anonymous-shape view ignores the names.
        let motif = [(60, 2), (62, 1), (64, 1), (67, 2)];
        let a = ingest("a", &motif);
        let b = ingest("b", &motif);
        for d in Dimension::ALL {
            assert_eq!(containment(&a, &b, d), 1.0, "identical at {d:?}");
        }
        assert_eq!(similarity(&a, &b, Dimension::Structure), 1.0);
    }

    #[test]
    fn transposition_is_contained_at_structure_but_not_names() {
        let a = ingest("a", &[(60, 2), (62, 1), (64, 1), (67, 2)]);
        let b = ingest("b", &[(67, 2), (69, 1), (71, 1), (74, 2)]); // up a fifth
        assert_eq!(
            containment(&a, &b, Dimension::Structure),
            1.0,
            "intervals survive transposition, so the structure is fully contained"
        );
        assert!(
            containment(&a, &b, Dimension::Names) < 1.0,
            "absolute pitches differ under transposition"
        );
    }

    #[test]
    fn an_edited_variant_is_partially_contained() {
        // same opening, a changed tail: most subtree shapes still match.
        let a = ingest("a", &[(60, 2), (62, 1), (64, 1), (67, 2), (64, 2)]);
        let edited = ingest("edited", &[(60, 2), (62, 1), (64, 1), (60, 3), (59, 1)]);
        let c = containment(&a, &edited, Dimension::Structure);
        assert!(c > 0.0 && c < 1.0, "partial containment, got {c}");
    }

    /// The property the fe pairing rests on: recording provenance (where each
    /// node came from) as `EdgeRole::Origin` edges does not move the shape
    /// address at any facet. Origin edges are payload the engine excludes from
    /// the fold, so a producer can attach a full origin/attribution graph
    /// without perturbing the fingerprint the catalog dedups on.
    #[test]
    fn origin_edges_do_not_perturb_the_shape() {
        let ek = |kind: &str, local: &str| EntityKey::new(kind, "pkg:token", local).unwrap();
        let build = |with_origin: bool| -> Graph {
            let mut g =
                Graph::new(GraphKey::new(ek("mir.body", "transfer"), "body").unwrap());
            let body = NodeKey::entity(ek("mir.body", "transfer"));
            let s0 = NodeKey::entity(ek("mir.stmt", "stmt:0"));
            let s1 = NodeKey::entity(ek("mir.stmt", "stmt:1"));
            g.add_node(body.clone(), "body").unwrap();
            g.add_node(s0.clone(), "stmt").unwrap();
            g.add_node(s1.clone(), "stmt").unwrap();
            g.add_field(&s0, Dimension::Structure, "op", "add").unwrap();
            g.add_field(&s1, Dimension::Structure, "op", "ret").unwrap();
            g.add_child(&body, "stmt", 0, &s0).unwrap();
            g.add_child(&body, "stmt", 1, &s1).unwrap();
            g.add_edge(&s0, "flows_to", &s1, EdgeRole::Data).unwrap();
            if with_origin {
                // provenance edges: each statement records the unit it lowered
                // from. Same node set, only Origin edges added.
                g.add_edge(&s0, "lowered_from", &body, EdgeRole::Origin).unwrap();
                g.add_edge(&s1, "lowered_from", &body, EdgeRole::Origin).unwrap();
            }
            g
        };

        let plain = ingest_graph(&build(false), "fe.mir.v1").unwrap();
        let traced = ingest_graph(&build(true), "fe.mir.v1").unwrap();
        for facet_of in [Facet::full, Facet::names_blind, Facet::structure_only] {
            let a = plain.facet_address(&facet_of(plain.policy_id)).unwrap();
            let b = traced.facet_address(&facet_of(traced.policy_id)).unwrap();
            assert_eq!(
                a.address_digest(),
                b.address_digest(),
                "origin edges moved a facet address"
            );
        }
        // the provenance really is present in the traced graph, just inert.
        assert_eq!(build(false).edges.len() + 2, build(true).edges.len());
    }
}
