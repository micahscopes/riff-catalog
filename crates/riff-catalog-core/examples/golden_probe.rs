use riff_catalog_core::*;

fn main() {
    let policy = HashPolicy::new("test/1", ViewMode::IdentityBound, CyclePolicy::Reject).unwrap();
    println!(
        "policy_id_identity_reject = {}",
        policy.policy_id().to_hex()
    );
    let anon =
        HashPolicy::new("test/1", ViewMode::AnonymousShape, CyclePolicy::CondenseScc).unwrap();
    println!("policy_id_anon_condense = {}", anon.policy_id().to_hex());

    // fixture: body -> literal(13), one reference edge
    let owner = "golden";
    let gk = GraphKey::new(
        EntityKey::new("test.unit", owner, "unit:0").unwrap(),
        "unit",
    )
    .unwrap();
    let mut graph = Graph::new(gk.clone());
    let body = NodeKey::entity(EntityKey::new("test.body", owner, "body:0").unwrap());
    let lit = NodeKey::entity(EntityKey::new("test.expr", owner, "expr:0").unwrap());
    graph.add_node(body.clone(), "body").unwrap();
    graph.add_node(lit.clone(), "literal").unwrap();
    graph
        .add_field(&lit, Dimension::Constants, "value", 13u64)
        .unwrap();
    graph
        .add_field(&lit, Dimension::Names, "name", "x")
        .unwrap();
    graph
        .add_field(&lit, Dimension::Types, "type", "u256")
        .unwrap();
    graph.add_child(&body, "expr", 0, &lit).unwrap();
    graph
        .add_edge(&lit, "uses", &body, EdgeRole::Reference)
        .unwrap();

    for (label, p) in [("identity_reject", &policy), ("anon_condense", &anon)] {
        let req = DigestRequest::all_dimensions(gk.clone(), p.clone());
        let hashes = digest_graph(&req, &graph).unwrap().hashes;
        for (dim, digest) in hashes.graph.iter() {
            println!("{label}.{} = {}", dim.as_str(), digest.to_hex());
        }
    }
    let facet = Facet::names_blind(anon.policy_id());
    println!("facet_id_names_blind = {}", facet.facet_id().to_hex());
}
