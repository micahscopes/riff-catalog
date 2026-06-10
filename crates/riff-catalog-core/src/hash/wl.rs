//! Weisfeiler-Leman color refinement inside one strongly-connected component.
//!
//! This replaces the prototype's key-ordered SCC hashing (its anonymity flaw):
//! member ordering inside every payload comes from refined colors and byte-
//! sorted records, never from node keys (invariant I5).
//!
//! Caveat, documented on purpose: 1-WL is incomplete on pathological regular
//! graphs, so anonymous equality means "WL-equivalent under policy", never
//! "isomorphic". Isomorphic components always hash equal (no false splits);
//! adversarial non-isomorphic pairs can collide. The equivalence is the kernel
//! of this function, versioned by the schema — and the claims layer exists to
//! govern exactly such residuals.

use std::collections::{BTreeMap, BTreeSet};

use crate::dimension::Dimension;
use crate::encode;
use crate::error::CatalogError;
use crate::policy::HashPolicy;
use crate::text::Digest;

/// An internal edge of the component, in id space.
pub(crate) struct InternalEdge<'g> {
    pub role: &'static str,
    pub label: &'g str,
    pub ordinal: u32,
    pub src: u32,
    pub dst: u32,
}

/// Refine colors to a fixpoint. `init` maps each member to its starting color
/// (its per-dimension local digest). Returns the final color per member.
///
/// Stop condition: the signature includes the node's own previous color, so
/// the partition can only split, never merge — when the class count is stable
/// the partition is stable. Round cap |members| is the theoretical maximum
/// number of splits.
pub(crate) fn refine(
    policy: &HashPolicy,
    dimension: Dimension,
    members: &[u32],
    internal: &[InternalEdge<'_>],
    init: &BTreeMap<u32, Digest>,
) -> Result<BTreeMap<u32, Digest>, CatalogError> {
    let mut color = init.clone();
    let mut classes = distinct(&color);

    for _ in 0..members.len() {
        let mut next = BTreeMap::new();
        for &member in members {
            let mut out_records: Vec<Vec<u8>> = Vec::new();
            let mut in_records: Vec<Vec<u8>> = Vec::new();
            for edge in internal {
                if edge.src == member {
                    out_records.push(edge_record(dimension, edge, &color[&edge.dst]));
                }
                if edge.dst == member {
                    in_records.push(edge_record(dimension, edge, &color[&edge.src]));
                }
            }
            let digest = encode::digest_record(policy, dimension, "wl.signature", |bytes| {
                encode::push_digest(bytes, &color[&member]);
                encode::push_sorted_records(bytes, out_records);
                encode::push_sorted_records(bytes, in_records);
            })?;
            next.insert(member, digest);
        }
        let next_classes = distinct(&next);
        color = next;
        if next_classes == classes {
            break;
        }
        classes = next_classes;
    }

    Ok(color)
}

// Edge topology (role/label/ordinal) is bound in every dimension, not just
// Structure: the out/in records feed a byte-sorted WL signature, so omitting it
// would let a member's color ignore how its incident edges are wired and
// labeled — collapsing distinct cyclic positions at the non-Structure facets.
fn edge_record(_dimension: Dimension, edge: &InternalEdge<'_>, neighbor_color: &Digest) -> Vec<u8> {
    let mut record = Vec::new();
    encode::push_str(&mut record, edge.role);
    encode::push_str(&mut record, edge.label);
    encode::push_u32(&mut record, edge.ordinal);
    encode::push_digest(&mut record, neighbor_color);
    record
}

fn distinct(colors: &BTreeMap<u32, Digest>) -> usize {
    colors.values().collect::<BTreeSet<_>>().len()
}

/// Digest of one component: member-count, the sorted multiset of final colors
/// (duplicates kept — orbit sizes count), and the quotient edge multiset over
/// colors.
///
/// The quotient edge multiset is bound in every dimension, not just Structure:
/// it carries the component's internal wiring (role/label/ordinal) that the
/// member-color multiset alone can leave ambiguous, so two components with the
/// same colors but different cyclic wiring must not collide at the Names,
/// Constants, or Types facets either.
pub(crate) fn component_digest(
    policy: &HashPolicy,
    dimension: Dimension,
    members: &[u32],
    internal: &[InternalEdge<'_>],
    color: &BTreeMap<u32, Digest>,
) -> Result<Digest, CatalogError> {
    let mut colors: Vec<&Digest> = members.iter().map(|m| &color[m]).collect();
    colors.sort_unstable();

    encode::digest_record(policy, dimension, "wl.component", |bytes| {
        encode::push_u32(bytes, members.len() as u32);
        for digest in &colors {
            encode::push_digest(bytes, digest);
        }
        let records = internal
            .iter()
            .map(|edge| {
                let mut record = Vec::new();
                encode::push_str(&mut record, edge.role);
                encode::push_str(&mut record, edge.label);
                encode::push_u32(&mut record, edge.ordinal);
                encode::push_digest(&mut record, &color[&edge.src]);
                encode::push_digest(&mut record, &color[&edge.dst]);
                record
            })
            .collect();
        encode::push_sorted_records(bytes, records);
    })
}
