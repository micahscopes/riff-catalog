//! Final `graph.full` assembly per dimension.

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

    // Edge records (Structure only): every role except Origin, flat.
    let edges: Vec<_> = graph
        .edges
        .iter()
        .filter(|edge| edge.role != EdgeRole::Origin)
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

        if dimension == Dimension::Structure {
            match policy.view_mode {
                ViewMode::IdentityBound => {
                    let mut records: Vec<(String, &str, &str, String, Vec<u8>)> = edges
                        .iter()
                        .map(|edge| {
                            let src = view.id_of(&edge.source).expect("validated");
                            let dst = view.id_of(&edge.target).expect("validated");
                            let mut payload = Vec::new();
                            encode::push_node_key(&mut payload, &edge.source);
                            encode::push_node_key(&mut payload, &edge.target);
                            encode::push_str(&mut payload, edge.role.as_str());
                            encode::push_str(&mut payload, edge.label.as_str());
                            encode::push_digest(&mut payload, &digest_of(src));
                            encode::push_digest(&mut payload, &digest_of(dst));
                            (
                                edge.source.canonical_key(),
                                edge.role.as_str(),
                                edge.label.as_str(),
                                edge.target.canonical_key(),
                                payload,
                            )
                        })
                        .collect();
                    records.sort_by(|a, b| (&a.0, a.1, a.2, &a.3).cmp(&(&b.0, b.1, b.2, &b.3)));
                    encode::push_u32(bytes, records.len() as u32);
                    for (_, _, _, _, payload) in records {
                        bytes.extend_from_slice(&payload);
                    }
                }
                ViewMode::AnonymousShape => {
                    let records = edges
                        .iter()
                        .map(|edge| {
                            let src = view.id_of(&edge.source).expect("validated");
                            let dst = view.id_of(&edge.target).expect("validated");
                            let mut record = Vec::new();
                            encode::push_str(&mut record, edge.role.as_str());
                            encode::push_str(&mut record, edge.label.as_str());
                            encode::push_digest(&mut record, &digest_of(src));
                            encode::push_digest(&mut record, &digest_of(dst));
                            record
                        })
                        .collect();
                    encode::push_sorted_records(bytes, records);
                }
            }
        } else {
            encode::push_u32(bytes, 0);
        }
    })
}
