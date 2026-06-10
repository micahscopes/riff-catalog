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
//!
//! SCHEMA_VERSION 1 -> 2 (review P1): edge topology (role/label/ordinal) is now
//! bound into every dimension's edge records, fixing the `f(1, 2)` vs `f(2, 1)`
//! and flat-edge role-swap collisions. The schema version is committed in every
//! record header, so all constants below were regenerated via
//! `examples/golden_probe.rs`.

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
        "7d08fcebdad5a41b3e764b977504de08ef61716f37a7284ff1985fba91a6fda1"
    );
    let anonymous =
        HashPolicy::new("test/1", ViewMode::AnonymousShape, CyclePolicy::CondenseScc).unwrap();
    assert_eq!(
        anonymous.policy_id().to_hex(),
        "bb6ebe47be5f1291b8f5a27a31f61a1b0d979ca9b2208e82ac539283f2d1e429"
    );
}

#[test]
fn golden_identity_reject_fixture() {
    let policy = HashPolicy::new("test/1", ViewMode::IdentityBound, CyclePolicy::Reject).unwrap();
    let digests = graph_digests(&policy);
    let expect = [
        (
            Dimension::Structure,
            "d3ae63812d6601b7ea2158ef04aa44f351b6a2066fe68ff276ec7daf0a0a273e",
        ),
        (
            Dimension::Names,
            "ff6d0d724f8f4f9a7c9e6999b18c7a65c7c9d02d5c73ebb4bc389ec5ad4c6fb4",
        ),
        (
            Dimension::Constants,
            "9105cde16c6ddb9147da2ed96cbca8c8ecc5ca1e142c2c0261cfe7fcb54c06b4",
        ),
        (
            Dimension::Types,
            "fe392c9872c47ed58e9a8b0c50223c0c3c89a71e0904fb4430744040d9856961",
        ),
        (
            Dimension::TraceEvents,
            "e7f7a6e4ca259fa2759178c1bdccac26a4a28bafa70c54cca71f01d951dd40fc",
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
            "a16d23242592298bebe7b6a9a48895ec4a28bb395fdaf5d883797b28169cd887",
        ),
        (
            Dimension::Names,
            "bc33a965b8eab28a97082f878716d1ce36a9b566653471a81d68c164bf1e2177",
        ),
        (
            Dimension::Constants,
            "04f613b01a20112d783bb347c73bb20a25deabdf5d3bbfcc924a51dce081d9fb",
        ),
        (
            Dimension::Types,
            "42bae8a527269863e4b12b5d1e0d603c8511ef9279559b4b15d5122b88e9589e",
        ),
        (
            Dimension::TraceEvents,
            "29aef97058617b2b40395b6d28d34e6e4ab79abd81169fc02edd758c7d7e0e54",
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
        "a54714f657ab35ba25195db8c2b71dca1e03f993c1f261a4db0e7b6575aaa990"
    );
}
