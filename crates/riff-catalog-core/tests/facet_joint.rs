//! A multi-dimension facet means the projection: equal once only those
//! dimensions' facts are kept. Where two shape-equal nodes differ in two of the
//! facet's dimensions, combining per-dimension digests would over-match; the
//! graph is flagged in `GraphHashes::mixed_pairs` and the address comes from
//! the joint pass. The same pair of graphs is in the Lean corpus
//! (`joint_pairs_a`, `joint_pairs_b`, see `lean_vectors.rs`).

use std::collections::BTreeMap;

use riff_catalog_core::{
    AddressValue, CatalogError, Digest, DigestRequest, Dimension, EntityKey, Facet, FacetAddress,
    Graph, GraphHashes, GraphKey, HashPolicy, Keys, NodeKey, digest_graph, facet_address,
    pair_bit,
};

fn entity(local: &str) -> NodeKey {
    NodeKey::entity(EntityKey::new("n", "demo", local).unwrap())
}

fn policy(keys: Keys) -> HashPolicy {
    HashPolicy::new("test/1", keys).unwrap()
}

/// One parent with two children under the same label and ordinal 0. Child `a`
/// is named `add` and `b` is named `mul`; the types are given per child.
fn two_children(type_of_add: &str, type_of_mul: &str) -> Graph {
    let key = GraphKey::new(EntityKey::new("test.unit", "g", "unit:0").unwrap(), "unit").unwrap();
    let mut graph = Graph::new(key);
    let (p, a, b) = (entity("p"), entity("a"), entity("b"));
    graph.add_node(p.clone(), "call").unwrap();
    graph.add_node(a.clone(), "op").unwrap();
    graph.add_node(b.clone(), "op").unwrap();
    graph.add_field(&a, Dimension::Names, "n", "add").unwrap();
    graph.add_field(&a, Dimension::Types, "t", type_of_add).unwrap();
    graph.add_field(&b, Dimension::Names, "n", "mul").unwrap();
    graph.add_field(&b, Dimension::Types, "t", type_of_mul).unwrap();
    graph.add_child(&p, "arg", 0, &a).unwrap();
    graph.add_child(&p, "arg", 0, &b).unwrap();
    graph
}

fn hashes(graph: &Graph, keys: Keys) -> GraphHashes {
    let request = DigestRequest::all_dimensions(graph.graph_key.clone(), policy(keys));
    digest_graph(&request, graph).unwrap().hashes
}

fn names_types(keys: Keys) -> Facet {
    Facet::new(policy(keys).policy_id(), [Dimension::Names, Dimension::Types]).unwrap()
}

fn digest(hashes: &GraphHashes, dimension: Dimension) -> Digest {
    *hashes.graph.get(dimension).unwrap()
}

#[test]
fn joint_pass_tells_apart_what_per_dimension_digests_cannot() {
    let graph_a = two_children("u8", "u16");
    let graph_b = two_children("u16", "u8");
    let (a, b) = (hashes(&graph_a, Keys::Free), hashes(&graph_b, Keys::Free));

    // Each dimension separately: the two graphs agree.
    assert_eq!(digest(&a, Dimension::Names), digest(&b, Dimension::Names));
    assert_eq!(digest(&a, Dimension::Types), digest(&b, Dimension::Types));

    // Both are flagged at the (names, types) pair.
    let bit = pair_bit(Dimension::Names, Dimension::Types);
    assert_ne!(a.mixed_pairs & bit, 0);
    assert_ne!(b.mixed_pairs & bit, 0);

    // The combined address refuses the flagged facet.
    let facet = names_types(Keys::Free);
    assert!(a.needs_joint(&facet) && b.needs_joint(&facet));
    for hashes in [&a, &b] {
        assert!(matches!(
            hashes.facet_address(&facet),
            Err(CatalogError::FacetNeedsJointPass)
        ));
    }

    // The joint addresses differ: the pairing of names and types differs.
    let policy = policy(Keys::Free);
    let joint_a = facet_address(&policy, &graph_a, &a, &facet).unwrap();
    let joint_b = facet_address(&policy, &graph_b, &b, &facet).unwrap();
    assert!(matches!(joint_a.value, AddressValue::Joint(_)));
    assert!(matches!(joint_b.value, AddressValue::Joint(_)));
    assert_ne!(joint_a.address_digest(), joint_b.address_digest());

    // Combining the stored digests would have over-matched.
    let combine = |hashes: &GraphHashes| -> FacetAddress {
        let digests: BTreeMap<Dimension, Digest> = [Dimension::Names, Dimension::Types]
            .into_iter()
            .map(|d| (d, digest(hashes, d)))
            .collect();
        FacetAddress::new(facet.clone(), digests).unwrap()
    };
    assert_eq!(combine(&a).address_digest(), combine(&b).address_digest());
    assert_ne!(combine(&a).address_digest(), joint_a.address_digest());
}

#[test]
fn bound_keys_flag_nothing_and_combine() {
    let graph_a = two_children("u8", "u16");
    let graph_b = two_children("u16", "u8");
    let (a, b) = (hashes(&graph_a, Keys::Bound), hashes(&graph_b, Keys::Bound));
    assert_eq!((a.mixed_pairs, b.mixed_pairs), (0, 0));

    let facet = names_types(Keys::Bound);
    let policy = policy(Keys::Bound);
    let address_a = facet_address(&policy, &graph_a, &a, &facet).unwrap();
    let address_b = facet_address(&policy, &graph_b, &b, &facet).unwrap();
    assert!(matches!(address_a.value, AddressValue::Combined(_)));
    assert_ne!(address_a.address_digest(), address_b.address_digest());
}

#[test]
fn equal_graphs_have_equal_joint_addresses_and_shapes() {
    let graph = two_children("u8", "u16");
    let twin = two_children("u8", "u16");
    let (a, b) = (hashes(&graph, Keys::Free), hashes(&twin, Keys::Free));
    let facet = names_types(Keys::Free);
    let policy = policy(Keys::Free);
    assert_eq!(
        facet_address(&policy, &graph, &a, &facet).unwrap().address_digest(),
        facet_address(&policy, &twin, &b, &facet).unwrap().address_digest()
    );
    assert_eq!(a.shape, b.shape);
    // The bare shape ignores every fact.
    assert_eq!(a.shape, hashes(&two_children("u16", "u8"), Keys::Free).shape);
}
