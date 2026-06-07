//! Per-dimension digest computation over a [`Graph`].

mod acyclic;
mod condensed;
mod graph_digest;
mod local;
mod scc;
mod view;
mod wl;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::dimension::Dimension;
use crate::error::CatalogError;
use crate::graph::Graph;
use crate::key::{GraphKey, NodeKey};
use crate::policy::{CyclePolicy, HashPolicy, PolicyId};
use crate::reference::{Facet, FacetAddress};
use crate::text::Digest;

/// What to compute: a graph, the encoding contract, and which dimensions.
///
/// The dimension set is an execution detail, not part of identity — see the
/// note on [`HashPolicy`] (invariant I9).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DigestRequest {
    pub graph: GraphKey,
    pub policy: HashPolicy,
    pub dimensions: BTreeSet<Dimension>,
}

impl DigestRequest {
    pub fn new(
        graph: GraphKey,
        policy: HashPolicy,
        dimensions: impl IntoIterator<Item = Dimension>,
    ) -> Result<Self, CatalogError> {
        let dimensions = dimensions.into_iter().collect::<BTreeSet<_>>();
        if dimensions.is_empty() {
            return Err(CatalogError::EmptyDimensions);
        }
        Ok(Self {
            graph,
            policy,
            dimensions,
        })
    }

    pub fn all_dimensions(graph: GraphKey, policy: HashPolicy) -> Self {
        Self {
            graph,
            policy,
            dimensions: Dimension::ALL.into_iter().collect(),
        }
    }

    pub fn policy_id(&self) -> PolicyId {
        self.policy.policy_id()
    }
}

/// One digest per computed dimension.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DimensionDigests {
    pub values: BTreeMap<Dimension, Digest>,
}

impl DimensionDigests {
    pub fn insert(&mut self, dimension: Dimension, digest: Digest) {
        self.values.insert(dimension, digest);
    }

    pub fn get(&self, dimension: Dimension) -> Option<&Digest> {
        self.values.get(&dimension)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Dimension, &Digest)> {
        self.values.iter()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeHashes {
    /// The node's own fields (and kind/key per policy), no context.
    pub local: DimensionDigests,
    /// The node's context-free digest: Merkle subtree fold (acyclic policies)
    /// or component context (CondenseScc).
    pub tree: DimensionDigests,
    /// Under CondenseScc: the digests of the component this node belongs to.
    pub component: Option<DimensionDigests>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentHash {
    /// Position in Tarjan emission order. Reporting only — never hashed.
    pub component_index: u32,
    pub members: Vec<NodeKey>,
    /// Final WL color per member, for debugging/provenance.
    #[serde(with = "crate::serde_pairs")]
    pub member_colors: BTreeMap<NodeKey, DimensionDigests>,
    /// The component.tree digests (component + everything it reaches).
    pub digests: DimensionDigests,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphHashes {
    pub policy_id: PolicyId,
    #[serde(with = "crate::serde_pairs")]
    pub nodes: BTreeMap<NodeKey, NodeHashes>,
    pub components: Vec<ComponentHash>,
    pub graph: DimensionDigests,
}

impl GraphHashes {
    /// Project the graph-level digests onto a facet (invariant I7: the facet
    /// travels with the digests).
    pub fn facet_address(&self, facet: &Facet) -> Result<FacetAddress, CatalogError> {
        if facet.policy_id != self.policy_id {
            return Err(CatalogError::IndexPolicyMismatch {
                expected: facet.policy_id.to_hex(),
                actual: self.policy_id.to_hex(),
            });
        }
        let mut digests = BTreeMap::new();
        for dimension in &facet.dimensions {
            let digest = self
                .graph
                .get(*dimension)
                .ok_or(CatalogError::MissingDimension {
                    dimension: dimension.as_str(),
                })?;
            digests.insert(*dimension, *digest);
        }
        FacetAddress::new(facet.clone(), digests)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DigestResult {
    pub request: DigestRequest,
    pub hashes: GraphHashes,
}

/// Compute per-dimension digests for a graph under a policy.
pub fn digest_graph(request: &DigestRequest, graph: &Graph) -> Result<DigestResult, CatalogError> {
    request.policy.check_supported()?;
    if graph.graph_key != request.graph {
        return Err(CatalogError::GraphKeyMismatch {
            requested: request.graph.canonical_key(),
            actual: graph.graph_key.canonical_key(),
        });
    }
    graph.validate()?;

    let policy = &request.policy;
    let dims = &request.dimensions;
    let view = view::IndexedGraph::build(graph);

    let mut local = Vec::with_capacity(view.len());
    for id in 0..view.len() {
        local.push(local::local_node_digests(policy, view.node(id), dims)?);
    }

    let (final_digests, components) = match policy.cycle_policy {
        CyclePolicy::Reject => {
            acyclic::check_acyclic(&view, true)?;
            let tree = acyclic::tree_digests(policy, &view, dims, &local)?;
            (tree, Vec::new())
        }
        CyclePolicy::NonRecursiveGraphEdges => {
            acyclic::check_acyclic(&view, false)?;
            let tree = acyclic::tree_digests(policy, &view, dims, &local)?;
            (tree, Vec::new())
        }
        CyclePolicy::CondenseScc => condensed::condensed_digests(policy, &view, dims, &local)?,
    };

    let mut graph_digests = DimensionDigests::default();
    for dimension in dims {
        graph_digests.insert(
            *dimension,
            graph_digest::graph_digest_for_dimension(
                policy,
                graph,
                &view,
                &final_digests,
                *dimension,
            )?,
        );
    }

    let component_of_node: BTreeMap<u32, usize> = components
        .iter()
        .enumerate()
        .flat_map(|(ci, c)| {
            c.members
                .iter()
                .map(move |m| (ci, m.clone()))
                .collect::<Vec<_>>()
        })
        .filter_map(|(ci, key)| view.id_of(&key).map(|id| (id, ci)))
        .collect();

    let nodes = (0..view.len())
        .map(|id| {
            let key = view.key(id).clone();
            let component = component_of_node
                .get(&(id as u32))
                .map(|&ci| components[ci].digests.clone());
            (
                key,
                NodeHashes {
                    local: local[id].clone(),
                    tree: final_digests[id].clone(),
                    component,
                },
            )
        })
        .collect();

    Ok(DigestResult {
        request: request.clone(),
        hashes: GraphHashes {
            policy_id: policy.policy_id(),
            nodes,
            components,
            graph: graph_digests,
        },
    })
}
