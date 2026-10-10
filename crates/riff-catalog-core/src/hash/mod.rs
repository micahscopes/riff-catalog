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
use crate::policy::{HashPolicy, PolicyId};
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
    /// The bare shape: nodes and edges with no facts and no kinds.
    pub shape: Digest,
    /// Pairs of dimensions that vary together inside one group of nodes the
    /// bare shape cannot tell apart (bit [`pair_bit`]). A facet containing
    /// such a pair cannot be computed by combining per-dimension digests: two
    /// graphs can agree in each dimension separately yet pair the facts
    /// differently. Not hashed.
    pub mixed_pairs: u32,
}

/// The bit for a pair of distinct dimensions in [`GraphHashes::mixed_pairs`];
/// no bit for a dimension paired with itself.
pub fn pair_bit(a: Dimension, b: Dimension) -> u32 {
    if a == b {
        return 0;
    }
    let index = |d: Dimension| Dimension::ALL.iter().position(|x| *x == d).expect("closed set");
    let (i, j) = {
        let (x, y) = (index(a), index(b));
        if x < y { (x, y) } else { (y, x) }
    };
    let n = Dimension::ALL.len();
    // Pairs in order (0,1), (0,2), ..., (0,n-1), (1,2), ...
    let before: usize = (0..i).map(|k| n - 1 - k).sum();
    1 << (before + (j - i - 1))
}

/// Whether a mixed-pairs mask flags a pair inside this set of dimensions.
pub fn mask_needs_joint(mixed_pairs: u32, dimensions: &BTreeSet<Dimension>) -> bool {
    let dims: Vec<Dimension> = dimensions.iter().copied().collect();
    dims.iter()
        .enumerate()
        .any(|(i, &a)| dims[i + 1..].iter().any(|&b| mixed_pairs & pair_bit(a, b) != 0))
}

/// A stable text key for a set of dimensions: their names joined by `+`, in
/// the fixed dimension order.
pub fn dimensions_key(dimensions: &BTreeSet<Dimension>) -> String {
    dimensions.iter().map(|d| d.as_str()).collect::<Vec<_>>().join("+")
}

impl GraphHashes {
    /// Whether this graph's address at `facet` needs the joint pass.
    pub fn needs_joint(&self, facet: &Facet) -> bool {
        mask_needs_joint(self.mixed_pairs, &facet.dimensions)
    }

    /// The combined address at a facet. Refuses a facet that needs the joint
    /// pass, since combining would over-match there; use
    /// [`facet_address`](crate::hash::facet_address) with the graph instead.
    pub fn facet_address(&self, facet: &Facet) -> Result<FacetAddress, CatalogError> {
        if facet.policy_id != self.policy_id {
            return Err(CatalogError::IndexPolicyMismatch {
                expected: facet.policy_id.to_hex(),
                actual: self.policy_id.to_hex(),
            });
        }
        if self.needs_joint(facet) {
            return Err(CatalogError::FacetNeedsJointPass);
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

/// Whether the graph's child edges, and with `include_dependency` also its
/// Dependency edges, form no cycle. Hashing accepts cycles; a recipe that
/// promises acyclic graphs checks this at ingest.
pub fn check_acyclic(graph: &Graph, include_dependency: bool) -> Result<(), CatalogError> {
    graph.validate()?;
    acyclic::check_acyclic(&view::IndexedGraph::build(graph), include_dependency)
}

/// Check a graph against its recipe's declaration, acyclicity included.
pub fn check_recipe(
    decl: &crate::recipe::RecipeDecl,
    graph: &Graph,
) -> Result<(), Vec<crate::recipe::Violation>> {
    use crate::recipe::{Acyclic, Violation};
    // The same structural checks hashing applies: a graph that could not be
    // hashed is not conformant either.
    if let Err(error) = graph.validate() {
        return Err(vec![Violation { at: None, problem: error.to_string() }]);
    }
    let mut violations = decl.check(graph).err().unwrap_or_default();
    let cycle = match decl.acyclic {
        Acyclic::No => Ok(()),
        Acyclic::Children => check_acyclic(graph, false),
        Acyclic::ChildrenAndDependency => check_acyclic(graph, true),
    };
    if let Err(error) = cycle {
        violations.push(Violation { at: None, problem: error.to_string() });
    }
    if violations.is_empty() { Ok(()) } else { Err(violations) }
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

    let (final_digests, components) = condensed::condensed_digests(policy, &view, dims, &local)?;

    let mut graph_digests = DimensionDigests::default();
    let mut colours: BTreeMap<Dimension, Vec<Digest>> = BTreeMap::new();
    for dimension in dims {
        let (digest, final_colours) = graph_digest::graph_digest_for_dimension(
            policy,
            graph,
            &view,
            &final_digests,
            *dimension,
        )?;
        graph_digests.insert(*dimension, digest);
        colours.insert(*dimension, final_colours);
    }
    let (shape, shape_colours) = graph_digest::shape_digest(policy, graph, &view)?;
    let mixed_pairs = mixed_pairs(&shape_colours, &colours);

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
            shape,
            mixed_pairs,
        },
    })
}

/// For every group of nodes the bare shape cannot tell apart, which
/// dimensions vary inside it; every pair of dimensions that vary inside one
/// group is recorded.
fn mixed_pairs(shape_colours: &[Digest], colours: &BTreeMap<Dimension, Vec<Digest>>) -> u32 {
    let mut groups: BTreeMap<&Digest, Vec<usize>> = BTreeMap::new();
    for (id, colour) in shape_colours.iter().enumerate() {
        groups.entry(colour).or_default().push(id);
    }
    let mut mask = 0;
    for members in groups.values().filter(|members| members.len() > 1) {
        let varying: Vec<Dimension> = colours
            .iter()
            .filter(|(_, c)| members.iter().any(|&m| c[m] != c[members[0]]))
            .map(|(d, _)| *d)
            .collect();
        for (i, &a) in varying.iter().enumerate() {
            for &b in &varying[i + 1..] {
                mask |= pair_bit(a, b);
            }
        }
    }
    mask
}

/// The address of a graph at a facet: combined from the stored per-dimension
/// digests where that is exact, otherwise from the joint pass.
pub fn facet_address(
    policy: &HashPolicy,
    graph: &Graph,
    hashes: &GraphHashes,
    facet: &Facet,
) -> Result<FacetAddress, CatalogError> {
    if facet.policy_id != hashes.policy_id || !hashes.needs_joint(facet) {
        return hashes.facet_address(facet);
    }
    Ok(FacetAddress::joint(facet.clone(), joint_digest(policy, graph, &facet.dimensions)?))
}

/// The joint digest of every set of computed dimensions the mask flags, keyed
/// by [`dimensions_key`]. Empty for a graph with no flagged pair, which is
/// nearly every graph. Stored at ingest so that a facet query never needs the
/// graph.
pub fn joint_digests(
    policy: &HashPolicy,
    graph: &Graph,
    hashes: &GraphHashes,
) -> Result<BTreeMap<String, Digest>, CatalogError> {
    let mut out = BTreeMap::new();
    if hashes.mixed_pairs == 0 {
        return Ok(out);
    }
    let computed: Vec<Dimension> = hashes.graph.values.keys().copied().collect();
    for subset in 1u32..(1 << computed.len()) {
        let dimensions: BTreeSet<Dimension> = computed
            .iter()
            .enumerate()
            .filter(|(i, _)| subset & (1 << i) != 0)
            .map(|(_, d)| *d)
            .collect();
        if dimensions.len() > 1 && mask_needs_joint(hashes.mixed_pairs, &dimensions) {
            out.insert(dimensions_key(&dimensions), joint_digest(policy, graph, &dimensions)?);
        }
    }
    Ok(out)
}

/// The joint pass: hash the graph with all of `dimensions`' facts as one
/// dimension. Every kept fact moves into one bin under a name prefixed by its
/// dimension; kinds are kept only when structure is among the dimensions.
/// The skeleton is unchanged.
pub fn joint_digest(
    policy: &HashPolicy,
    graph: &Graph,
    dimensions: &BTreeSet<Dimension>,
) -> Result<Digest, CatalogError> {
    let mut joint = Graph::new(graph.graph_key.clone());
    for node in graph.nodes.values() {
        let kind = if dimensions.contains(&Dimension::Structure) { node.kind.as_str() } else { "_" };
        joint.add_node(node.key.clone(), kind)?;
        for field in node.fields.iter().filter(|f| dimensions.contains(&f.dimension)) {
            joint.add_field(
                &node.key,
                Dimension::Structure,
                format!("{}/{}", field.dimension.as_str(), field.name.as_str()),
                field.value.clone(),
            )?;
        }
    }
    joint.children = graph.children.clone();
    joint.edges = graph.edges.clone();
    let request = DigestRequest::new(graph.graph_key.clone(), policy.clone(), [Dimension::Structure])?;
    let hashes = digest_graph(&request, &joint)?.hashes;
    Ok(*hashes.graph.get(Dimension::Structure).expect("structure requested"))
}
