//! CondenseScc orchestration: Tarjan condensation, WL refinement per
//! component, condensation-DAG folding, and per-node component-context
//! digests. No node keys, component indices, or internal ids enter any
//! anonymous payload (invariant I5); identity mode adds node keys only in
//! `node.component_context` payloads.
//!
//! Components are processed in Tarjan emission order (every component a
//! member reaches is finished first), so a member's **outgoing cross-component
//! edges** can be folded into its WL *initial* color (`wl.init`). Without
//! this, two members of a symmetric cycle that differ only in what hangs off
//! them outside the SCC would never color apart (caught by
//! `wl_separates_asymmetric_members`). Cross-*incoming* edges are context,
//! not content, and deliberately stay out (invariant I3).

use std::collections::{BTreeMap, BTreeSet};

use crate::dimension::Dimension;
use crate::encode;
use crate::error::CatalogError;
use crate::hash::view::IndexedGraph;
use crate::hash::wl::{self, InternalEdge};
use crate::hash::{ComponentHash, DimensionDigests};
use crate::policy::{HashPolicy, ViewMode};
use crate::text::Digest;

pub(crate) fn condensed_digests(
    policy: &HashPolicy,
    view: &IndexedGraph<'_>,
    dimensions: &BTreeSet<Dimension>,
    local: &[DimensionDigests],
) -> Result<(Vec<DimensionDigests>, Vec<ComponentHash>), CatalogError> {
    let components = super::scc::strongly_connected_components(&view.recursive_succ);
    let mut comp_of = vec![usize::MAX; view.len()];
    for (ci, members) in components.iter().enumerate() {
        for &member in members {
            comp_of[member as usize] = ci;
        }
    }

    // Partition recursive edges: internals per component, cross edges grouped
    // by source *member* (for wl.init) and by source component (for the fold).
    let mut internal: Vec<Vec<InternalEdge<'_>>> =
        (0..components.len()).map(|_| Vec::new()).collect();
    let mut cross_by_component: Vec<Vec<InternalEdge<'_>>> =
        (0..components.len()).map(|_| Vec::new()).collect();
    let mut cross_by_member: Vec<Vec<InternalEdge<'_>>> =
        (0..view.len()).map(|_| Vec::new()).collect();
    for edge in &view.recursive {
        let source_comp = comp_of[edge.src as usize];
        let target_comp = comp_of[edge.dst as usize];
        let rec = || InternalEdge {
            role: edge.role,
            label: edge.label,
            ordinal: edge.ordinal,
            src: edge.src,
            dst: edge.dst,
        };
        if source_comp == target_comp {
            internal[source_comp].push(rec());
        } else {
            cross_by_component[source_comp].push(rec());
            cross_by_member[edge.src as usize].push(rec());
        }
    }

    // Emission order: successors of a component are always processed first.
    let mut member_colors: Vec<BTreeMap<Dimension, BTreeMap<u32, Digest>>> =
        Vec::with_capacity(components.len());
    let mut wl_component: Vec<DimensionDigests> = Vec::with_capacity(components.len());
    let mut component_tree: Vec<DimensionDigests> = Vec::with_capacity(components.len());

    for (ci, members) in components.iter().enumerate() {
        let mut per_dimension = BTreeMap::new();
        let mut component = DimensionDigests::default();
        let mut tree = DimensionDigests::default();

        for dimension in dimensions {
            // wl.init: local digest + sorted outgoing cross-edge records.
            let mut init: BTreeMap<u32, Digest> = BTreeMap::new();
            for &member in members {
                let records = cross_by_member[member as usize]
                    .iter()
                    .map(|edge| {
                        let target_comp = comp_of[edge.dst as usize];
                        debug_assert!(
                            target_comp < ci,
                            "tarjan emission order: successors emitted first"
                        );
                        cross_edge_record(
                            *dimension,
                            edge,
                            component_tree[target_comp]
                                .get(*dimension)
                                .expect("successor component computed"),
                            &member_colors[target_comp][dimension][&edge.dst],
                        )
                    })
                    .collect();
                let digest = encode::digest_record(policy, *dimension, "wl.init", |bytes| {
                    encode::push_digest(
                        bytes,
                        local[member as usize].get(*dimension).expect("local"),
                    );
                    encode::push_sorted_records(bytes, records);
                })?;
                init.insert(member, digest);
            }

            let colors = wl::refine(policy, *dimension, members, &internal[ci], &init)?;
            component.insert(
                *dimension,
                wl::component_digest(policy, *dimension, members, &internal[ci], &colors)?,
            );

            // component.tree: own component digest + cross-edge records over
            // final colors. Partially redundant with wl.init by construction;
            // kept as cheap insurance over the quotient structure.
            let records = cross_by_component[ci]
                .iter()
                .map(|edge| {
                    let target_comp = comp_of[edge.dst as usize];
                    let mut record = cross_edge_record(
                        *dimension,
                        edge,
                        component_tree[target_comp]
                            .get(*dimension)
                            .expect("successor component computed"),
                        &member_colors[target_comp][dimension][&edge.dst],
                    );
                    let mut with_source = Vec::new();
                    encode::push_digest(&mut with_source, &colors[&edge.src]);
                    with_source.extend_from_slice(&record);
                    record = with_source;
                    record
                })
                .collect();
            let digest = encode::digest_record(policy, *dimension, "component.tree", |bytes| {
                encode::push_digest(bytes, component.get(*dimension).expect("component digest"));
                encode::push_sorted_records(bytes, records);
            })?;
            tree.insert(*dimension, digest);

            per_dimension.insert(*dimension, colors);
        }

        member_colors.push(per_dimension);
        wl_component.push(component);
        component_tree.push(tree);
    }

    // Per-node final digests: local + final WL color + own component's fold.
    let mut node_final: Vec<DimensionDigests> = Vec::with_capacity(view.len());
    for id in 0..view.len() {
        let ci = comp_of[id];
        let mut digests = DimensionDigests::default();
        for dimension in dimensions {
            let digest =
                encode::digest_record(policy, *dimension, "node.component_context", |bytes| {
                    if policy.view_mode == ViewMode::IdentityBound {
                        encode::push_node_key(bytes, view.key(id));
                    }
                    encode::push_digest(bytes, local[id].get(*dimension).expect("local"));
                    encode::push_digest(bytes, &member_colors[ci][dimension][&(id as u32)]);
                    encode::push_digest(
                        bytes,
                        component_tree[ci].get(*dimension).expect("component tree"),
                    );
                })?;
            digests.insert(*dimension, digest);
        }
        node_final.push(digests);
    }

    let component_hashes = components
        .iter()
        .enumerate()
        .map(|(ci, members)| {
            let member_keys: Vec<_> = members
                .iter()
                .map(|&member| view.key(member as usize).clone())
                .collect();
            let colors = members
                .iter()
                .map(|&member| {
                    let mut digests = DimensionDigests::default();
                    for dimension in dimensions {
                        digests.insert(*dimension, member_colors[ci][dimension][&member]);
                    }
                    (view.key(member as usize).clone(), digests)
                })
                .collect();
            ComponentHash {
                component_index: ci as u32,
                members: member_keys,
                member_colors: colors,
                digests: component_tree[ci].clone(),
            }
        })
        .collect();

    Ok((node_final, component_hashes))
}

/// Record for one outgoing cross-component edge: (Structure only) role, label,
/// ordinal; then the target component's fold and the target member's color.
fn cross_edge_record(
    dimension: Dimension,
    edge: &InternalEdge<'_>,
    target_tree: &Digest,
    target_color: &Digest,
) -> Vec<u8> {
    let mut record = Vec::new();
    if dimension == Dimension::Structure {
        encode::push_str(&mut record, edge.role);
        encode::push_str(&mut record, edge.label);
        encode::push_u32(&mut record, edge.ordinal);
    }
    encode::push_digest(&mut record, target_tree);
    encode::push_digest(&mut record, target_color);
    record
}
