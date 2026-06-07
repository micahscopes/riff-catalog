//! Golden digests freezing the canonical encoding contract (invariant I8).
//!
//! IF A VALUE IN THIS FILE CHANGES, THE ENCODING CHANGED. That is a breaking
//! event: bump `SCHEMA_VERSION`, regenerate the constants (see
//! `examples/golden_probe.rs`), and record the migration in the changelog.
//! Never "fix" a golden value to make a refactor pass.
//!
//! Pre-release exception, exercised once (2026-06-07): the flat-edge
//! per-dimension folding fix (external review P1) changed non-Structure
//! digests; since no schema has ever been published, the constants were
//! regenerated without a version bump. After the first release this path
//! is closed — bump SCHEMA_VERSION instead.

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
            "8e89a8898f19c751fbaee2923569b20638bc57ad2b8c82f3b3e647e9f5520637",
        ),
        (
            Dimension::Constants,
            "76b365a5842819c19788500f191d983f298deb905a158c1c4c70c3d703dc6dc5",
        ),
        (
            Dimension::Types,
            "f0843ae7b5022968121719a1e4f1a687d11e723836756c43434d9452852b7117",
        ),
        (
            Dimension::TraceEvents,
            "357d41cb491523f5e11f57eec2f3549c4916915c85c616224983e1dc58180f4a",
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
            "263794a276babe9a8a9b7bc6d908d3aeb93a34b315286926d3a67642265bc9eb",
        ),
        (
            Dimension::Constants,
            "11605a82066058f15f7c8212d983de9e90e35fa6b77f28a58789497fdfb67381",
        ),
        (
            Dimension::Types,
            "e39af97974a08ceb187a093dfba712838e82e7a594071b187836c3b8a5e91069",
        ),
        (
            Dimension::TraceEvents,
            "76f591bef6ed4e9154bba4d6fcb404728fc5a58e1f6e09a0b12cfa76b1b170a6",
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
