//! Golden digests freezing the canonical encoding contract (invariant I8).
//!
//! IF A VALUE IN THIS FILE CHANGES, THE ENCODING CHANGED. That is a breaking
//! event: bump `SCHEMA_VERSION`, regenerate the constants (see
//! `examples/golden_probe.rs`), and record the migration in the changelog.
//! Never "fix" a golden value to make a refactor pass.

use riff_catalog_core::*;

fn fixture() -> (GraphKey, Graph) {
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
    (gk, graph)
}

fn graph_digests(policy: &HashPolicy) -> DimensionDigests {
    let (gk, graph) = fixture();
    let request = DigestRequest::all_dimensions(gk, policy.clone());
    digest_graph(&request, &graph).unwrap().hashes.graph
}

#[test]
fn golden_policy_ids() {
    let identity = HashPolicy::new("test/1", ViewMode::IdentityBound, CyclePolicy::Reject).unwrap();
    assert_eq!(
        identity.policy_id().to_hex(),
        "2e841f98f8ff971578698d9f9847b05ef61ad2a339827c89af9816976d50c2bc"
    );
    let anonymous =
        HashPolicy::new("test/1", ViewMode::AnonymousShape, CyclePolicy::CondenseScc).unwrap();
    assert_eq!(
        anonymous.policy_id().to_hex(),
        "cb67607b7e9fee126445e15ecfc995c804f96d3fbd41a169aedf06b0706f77b8"
    );
}

#[test]
fn golden_identity_reject_fixture() {
    let policy = HashPolicy::new("test/1", ViewMode::IdentityBound, CyclePolicy::Reject).unwrap();
    let digests = graph_digests(&policy);
    let expect = [
        (
            Dimension::Structure,
            "249eb4f249a4758dfaf820d3efcbc4606926dfc50a3ef4761b1a1fa84aff1d35",
        ),
        (
            Dimension::Names,
            "7f084f2aa551d718cd473ff9d41cba9c1779b732f509ee5d2f30fadda5250420",
        ),
        (
            Dimension::Constants,
            "2ebe331bbe690aed87fc17df452a8d786bfd5ec27d740e64b27e391eb6ccea2a",
        ),
        (
            Dimension::Types,
            "d360cd7e3d5245fc634626891547db808c5b2cbb4ad44f5f95c6a76df8dc8c52",
        ),
        (
            Dimension::TraceEvents,
            "54756a629124474ea488fd8103fdeec31f8da9c7ea3cd1765c357291fdaf3938",
        ),
    ];
    for (dimension, hex) in expect {
        assert_eq!(
            digests.get(dimension).unwrap().to_hex(),
            hex,
            "{dimension:?}"
        );
    }
}

#[test]
fn golden_anonymous_condense_fixture() {
    let policy =
        HashPolicy::new("test/1", ViewMode::AnonymousShape, CyclePolicy::CondenseScc).unwrap();
    let digests = graph_digests(&policy);
    let expect = [
        (
            Dimension::Structure,
            "d6d7dbdf03ec6f28f226e6085069a0cf061a304915316c0e5d9f179e014a3c53",
        ),
        (
            Dimension::Names,
            "5a128bcd6a7459ce015e3edb327914f614841ffd83b810b77f7f5a66bcad0a8c",
        ),
        (
            Dimension::Constants,
            "e8b151e495d4b1b3bdd5338ab72190f70ef808d57bbabe8bb966eee57d294914",
        ),
        (
            Dimension::Types,
            "bcdc3767899dbd0058204716affb3444f090ee655fd90ac22360d6ce2827d75e",
        ),
        (
            Dimension::TraceEvents,
            "aae15e93fe3214750575f6f6d5fad143fd3043ece14be0a46f6febeb8b7574ce",
        ),
    ];
    for (dimension, hex) in expect {
        assert_eq!(
            digests.get(dimension).unwrap().to_hex(),
            hex,
            "{dimension:?}"
        );
    }
}

#[test]
fn golden_facet_id() {
    let policy =
        HashPolicy::new("test/1", ViewMode::AnonymousShape, CyclePolicy::CondenseScc).unwrap();
    assert_eq!(
        Facet::names_blind(policy.policy_id()).facet_id().to_hex(),
        "4e598df430c9c03f6ec94d8c2da73b9fa902418f6133a9b57bffa0c9eca7c73b"
    );
}
