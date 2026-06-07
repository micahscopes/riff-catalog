//! M5a gate: indexes group correctly, reject mismatches, and round-trip JSONL.

use riff_catalog_core::*;

fn entity(kind: &str, owner: &str, local: &str) -> NodeKey {
    NodeKey::entity(EntityKey::new(kind, owner, local).unwrap())
}

fn literal_graph(owner: &str, value: u64) -> (GraphKey, Graph) {
    let gk = GraphKey::new(
        EntityKey::new("test.unit", owner, "unit:0").unwrap(),
        "unit",
    )
    .unwrap();
    let mut graph = Graph::new(gk.clone());
    let body = entity("test.body", owner, "body:0");
    let lit = entity("test.expr", owner, "expr:0");
    graph.add_node(body.clone(), "body").unwrap();
    graph.add_node(lit.clone(), "literal").unwrap();
    graph
        .add_field(&lit, Dimension::Constants, "value", value)
        .unwrap();
    graph.add_child(&body, "expr", 0, &lit).unwrap();
    (gk, graph)
}

fn result_for(owner: &str, value: u64, policy: &HashPolicy) -> DigestResult {
    let (gk, graph) = literal_graph(owner, value);
    let request = DigestRequest::all_dimensions(gk, policy.clone());
    digest_graph(&request, &graph).unwrap()
}

fn anon_policy() -> HashPolicy {
    HashPolicy::new("test/1", ViewMode::AnonymousShape, CyclePolicy::CondenseScc).unwrap()
}

#[test]
fn digest_index_groups_same_shapes() {
    let policy = anon_policy();
    let left = result_for("owner-a", 13, &policy);
    let right = result_for("owner-b", 13, &policy);
    let digest = *left.hashes.graph.get(Dimension::Structure).unwrap();

    let index =
        DigestIndex::from_results(policy.policy_id(), vec![left.clone(), right.clone()]).unwrap();
    let lookup = index
        .lookup(LookupRequest {
            policy_id: policy.policy_id(),
            dimension: Dimension::Structure,
            digest,
        })
        .unwrap();
    assert_eq!(lookup.graphs.len(), 2);
}

#[test]
fn digest_index_rejects_policy_mismatch() {
    let policy = anon_policy();
    let other = HashPolicy::new("test/1", ViewMode::IdentityBound, CyclePolicy::Reject).unwrap();
    let result = result_for("owner-a", 13, &policy);
    assert!(matches!(
        DigestIndex::from_results(other.policy_id(), vec![result]),
        Err(CatalogError::IndexPolicyMismatch { .. })
    ));
}

#[test]
fn facet_index_separates_at_full_but_merges_names_blind() {
    // Same structure, but give one graph an extra Names field: full facet
    // separates, names-blind facet buckets them together.
    let policy = anon_policy();
    let (gk_a, mut a) = literal_graph("owner-a", 13);
    a.add_field(
        &entity("test.expr", "owner-a", "expr:0"),
        Dimension::Names,
        "name",
        "x",
    )
    .unwrap();
    let (gk_b, b) = literal_graph("owner-b", 13);
    let res_a = digest_graph(&DigestRequest::all_dimensions(gk_a, policy.clone()), &a).unwrap();
    let res_b = digest_graph(&DigestRequest::all_dimensions(gk_b, policy.clone()), &b).unwrap();

    let full = FacetIndex::from_results(
        Facet::full(policy.policy_id()),
        vec![res_a.clone(), res_b.clone()],
    )
    .unwrap();
    assert_eq!(full.entries.len(), 2, "full facet separates");

    let blind =
        FacetIndex::from_results(Facet::names_blind(policy.policy_id()), vec![res_a, res_b])
            .unwrap();
    assert_eq!(blind.entries.len(), 1, "names-blind facet merges");
    assert_eq!(blind.entries[0].graphs.len(), 2);
}

#[test]
fn indexes_round_trip_jsonl() {
    let policy = anon_policy();
    let results = vec![
        result_for("owner-a", 13, &policy),
        result_for("owner-b", 14, &policy),
    ];
    let index = FacetIndex::from_results(Facet::full(policy.policy_id()), results).unwrap();

    // one entry per line, header on the first
    let mut lines = vec![serde_json::to_string(&index.facet).unwrap()];
    for entry in &index.entries {
        lines.push(serde_json::to_string(entry).unwrap());
    }
    let jsonl = lines.join("\n");

    let mut parsed = jsonl.lines();
    let facet: Facet = serde_json::from_str(parsed.next().unwrap()).unwrap();
    let entries: Vec<FacetIndexEntry> = parsed
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(FacetIndex { facet, entries }, index);
}

#[test]
fn digest_results_round_trip_json() {
    let policy = anon_policy();
    let result = result_for("owner-a", 13, &policy);
    let json = serde_json::to_string(&result).unwrap();
    let back: DigestResult = serde_json::from_str(&json).unwrap();
    assert_eq!(back, result);
}
