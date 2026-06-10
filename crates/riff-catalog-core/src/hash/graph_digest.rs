//! Final `graph.full` assembly per dimension.
//!
//! Flat edges (every role except Origin) fold through EVERY dimension using
//! that dimension's endpoint digests — not just Structure. Otherwise
//! rewiring an edge between two nodes that agree at Structure but differ at
//! Constants/Names/Types would collide at the full facet (invariant I2;
//! found by external review). Role and label remain Structure-only payload
//! (one-directional purity); node keys ride along in identity mode.

use crate::dimension::Dimension;
use crate::encode;
use crate::error::CatalogError;
use crate::graph::{EdgeRole, Graph};
use crate::hash::DimensionDigests;
use crate::hash::view::IndexedGraph;
use crate::policy::{HashPolicy, ViewMode};
use crate::text::Digest;

pub(crate) fn graph_digest_for_dimension(
    policy: &HashPolicy,
    graph: &Graph,
    view: &IndexedGraph<'_>,
    final_digests: &[DimensionDigests],
    dimension: Dimension,
) -> Result<Digest, CatalogError> {
    let digest_of = |id: u32| -> Digest {
        *final_digests[id as usize]
            .get(dimension)
            .expect("dimension computed")
    };

    // Node records: identity mode sorts by canonical key and includes keys;
    // anonymous mode sorts by digest and includes digests only.
    let mut node_entries: Vec<(u32, Digest)> = (0..view.len() as u32)
        .map(|id| (id, digest_of(id)))
        .collect();
    match policy.view_mode {
        ViewMode::IdentityBound => {
            node_entries.sort_by_key(|(id, _)| view.key(*id as usize).canonical_key());
        }
        ViewMode::AnonymousShape => {
            node_entries.sort_by(|a, b| a.1.cmp(&b.1));
        }
    }

    // Edge records: byte-sorted multiset in both view modes. Identity mode
    // prepends the endpoint keys (deterministic and key-laden, like every
    // identity payload). Role + label are edge topology and are carried in
    // every dimension, not just Structure: the records are a sorted multiset, so
    // without them swapping which edge plays which role between endpoints that
    // are equal in Structure but distinct in another dimension would leave the
    // multiset (and thus that dimension's digest) unchanged.
    let edge_records: Vec<Vec<u8>> = graph
        .edges
        .iter()
        .filter(|edge| edge.role != EdgeRole::Origin)
        .map(|edge| {
            let src = view.id_of(&edge.source).expect("validated");
            let dst = view.id_of(&edge.target).expect("validated");
            let mut record = Vec::new();
            if policy.view_mode == ViewMode::IdentityBound {
                encode::push_node_key(&mut record, &edge.source);
                encode::push_node_key(&mut record, &edge.target);
            }
            encode::push_str(&mut record, edge.role.as_str());
            encode::push_str(&mut record, edge.label.as_str());
            encode::push_digest(&mut record, &digest_of(src));
            encode::push_digest(&mut record, &digest_of(dst));
            record
        })
        .collect();

    encode::digest_record(policy, dimension, "graph.full", |bytes| {
        if policy.view_mode == ViewMode::IdentityBound {
            encode::push_str(bytes, &graph.graph_key.canonical_key());
        }

        encode::push_u32(bytes, node_entries.len() as u32);
        for (id, digest) in &node_entries {
            if policy.view_mode == ViewMode::IdentityBound {
                encode::push_node_key(bytes, view.key(*id as usize));
            }
            encode::push_digest(bytes, digest);
        }

        encode::push_sorted_records(bytes, edge_records);
    })
}
