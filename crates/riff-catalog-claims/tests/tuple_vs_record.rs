//! End-to-end integration of the whole layer stack (M7 gate from the core
//! plan): two small graphs shaped like a tuple-style and a record-style
//! constructor do NOT share a structural bucket; a constructor-correspondence
//! claim with an ordered field-map witness merges them, attributably, via
//! [`ClaimGatedIndex`]. This is the playground's tuple ≅ record moment.

use riff_catalog_claims::*;
use riff_catalog_core::*;

/// `make(a, b)` as a tuple constructor: mk -> [arg0, arg1]
fn tuple_graph(owner: &str) -> (GraphKey, Graph) {
    let gk = GraphKey::new(EntityKey::new("test.fn", owner, "fn:tmul").unwrap(), "fn").unwrap();
    let mut graph = Graph::new(gk.clone());
    let mk = NodeKey::entity(EntityKey::new("test.expr", owner, "mk").unwrap());
    let a = NodeKey::entity(EntityKey::new("test.expr", owner, "a").unwrap());
    let b = NodeKey::entity(EntityKey::new("test.expr", owner, "b").unwrap());
    graph.add_node(mk.clone(), "tuple").unwrap();
    graph.add_node(a.clone(), "param").unwrap();
    graph.add_node(b.clone(), "param").unwrap();
    graph.add_child(&mk, "item", 0, &a).unwrap();
    graph.add_child(&mk, "item", 1, &b).unwrap();
    (gk, graph)
}

/// `make{re: a, im: b}` as a record constructor: rec -> {re: arg0, im: arg1}
fn record_graph(owner: &str) -> (GraphKey, Graph) {
    let gk = GraphKey::new(EntityKey::new("test.fn", owner, "fn:rmul").unwrap(), "fn").unwrap();
    let mut graph = Graph::new(gk.clone());
    let rec = NodeKey::entity(EntityKey::new("test.expr", owner, "rec").unwrap());
    let a = NodeKey::entity(EntityKey::new("test.expr", owner, "a").unwrap());
    let b = NodeKey::entity(EntityKey::new("test.expr", owner, "b").unwrap());
    graph.add_node(rec.clone(), "record").unwrap();
    graph.add_node(a.clone(), "param").unwrap();
    graph.add_node(b.clone(), "param").unwrap();
    graph.add_child(&rec, "re", 0, &a).unwrap();
    graph.add_child(&rec, "im", 1, &b).unwrap();
    (gk, graph)
}

#[test]
fn tuple_and_record_unify_only_via_claim() {
    let policy =
        HashPolicy::new("test/1", ViewMode::AnonymousShape, CyclePolicy::CondenseScc).unwrap();
    let facet = Facet::structure_only(policy.policy_id());

    let (tuple_key, tuple) = tuple_graph("prog-a");
    let (record_key, record) = record_graph("prog-b");
    let tuple_result = digest_graph(
        &DigestRequest::all_dimensions(tuple_key.clone(), policy.clone()),
        &tuple,
    )
    .unwrap();
    let record_result = digest_graph(
        &DigestRequest::all_dimensions(record_key.clone(), policy.clone()),
        &record,
    )
    .unwrap();

    let tuple_address = tuple_result.hashes.facet_address(&facet).unwrap();
    let record_address = record_result.hashes.facet_address(&facet).unwrap();

    // Structurally distinct: tuple vs record genuinely differ (different
    // kinds and child labels).
    assert_ne!(
        tuple_address.address_digest(),
        record_address.address_digest()
    );

    let index = FacetIndex::from_results(facet.clone(), vec![tuple_result, record_result]).unwrap();
    assert_eq!(index.entries.len(), 2, "no shared bucket without a claim");

    // Assert the bridge: constructors coincide, witnessed by the field order.
    let claim = Claim::new(
        facet.clone(),
        tuple_address.clone(),
        record_address.clone(),
        Witness::new(
            "constructor-correspondence",
            [
                ("0".to_string(), "re".to_string()),
                ("1".to_string(), "im".to_string()),
            ],
        )
        .unwrap(),
        Some("tuple [a,b] ≅ record {re,im} by declared field order".into()),
    )
    .unwrap();
    let claim_id = claim.claim_id();
    let mut claims = ClaimSet::new();
    claims.insert(claim);

    let mut closure = claims.closure_for_facet(&facet);
    let mut gated = ClaimGatedIndex {
        index: &index,
        closure: &mut closure,
    };
    let lookup = gated.lookup(&tuple_address).unwrap();

    assert_eq!(lookup.direct, vec![tuple_key]);
    assert_eq!(
        lookup.via_claims.len(),
        1,
        "record graph reachable via the claim"
    );
    assert_eq!(lookup.via_claims[0].1, vec![record_key]);
    assert_eq!(lookup.supporting_claims, vec![claim_id]);
}
