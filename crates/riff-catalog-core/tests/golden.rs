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
//!
//! SCHEMA_VERSION 2 -> 3 (consolidation, 2026-10-09): one fold for every cycle
//! policy, and `graph.full` refines colours over the whole skeleton so free
//! mode tells flat edges between equal-content nodes apart; the graph key is no
//! longer hashed. The policy is now the recipe and the keys mode only (no
//! algorithm name, no cycle policy). Regenerated via `examples/golden_probe.rs`.

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
    let identity = HashPolicy::new("test/1", Keys::Bound).unwrap();
    assert_eq!(
        identity.policy_id().to_hex(),
        "b2a53a46bf979ebe7ddd7060107b379244555300eae561f7facff50e8415213a"
    );
    let anonymous =
        HashPolicy::new("test/1", Keys::Free).unwrap();
    assert_eq!(
        anonymous.policy_id().to_hex(),
        "867c75215b4383e77d887c737ffa7093b9944e2d9578abd3affedaf98a398f69"
    );
}

#[test]
fn golden_bound_fixture() {
    let policy = HashPolicy::new("test/1", Keys::Bound).unwrap();
    let digests = graph_digests(&policy);
    let expect = [
        (
            Dimension::Structure,
            "d083167dc593169c762d7d859a37e19e061c4f65cca211fd49a43bae4017f5ab",
        ),
        (
            Dimension::Names,
            "823cabf1d7e8b57fd0177d88371f41cd2cd0ddde56b3275e638e4b229fb1afe8",
        ),
        (
            Dimension::Constants,
            "0d148f3a8ad5aa8d551582205ec00bf79b28ebd2fa0da6af108964f5fb9ce2e4",
        ),
        (
            Dimension::Types,
            "0b7cbb70f0931a3f07622e3360892c55a2340c44e400767831d4088dd15f6250",
        ),
        (
            Dimension::TraceEvents,
            "f777fde3348b40fb6ac2219f4c90c1f28f37d66b335c46a753f9753335d33d4e",
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
fn golden_free_fixture() {
    let policy =
        HashPolicy::new("test/1", Keys::Free).unwrap();
    let digests = graph_digests(&policy);
    let expect = [
        (
            Dimension::Structure,
            "2d57910e4aa2dadbbe02413bdc675a38c639c02b32a2773144ac41694b97a2a9",
        ),
        (
            Dimension::Names,
            "df2a7dd5ae81690d2cb585bae1fa1ff63670c9e72bd44f99a60ff0ee7fa0ff48",
        ),
        (
            Dimension::Constants,
            "fe15af979180ce737c17ce74b1acc2fe51197bebc67238317afd25e977b9944c",
        ),
        (
            Dimension::Types,
            "8808d3304feb1745186865b76782e961f0ea4ef977e4dbb8c75b43ade28aa7f0",
        ),
        (
            Dimension::TraceEvents,
            "efd463ff13c2157ca8a813519edba89ebe62007b014395bc014982989672329f",
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
        HashPolicy::new("test/1", Keys::Free).unwrap();
    assert_eq!(
        Facet::names_blind(policy.policy_id()).facet_id().to_hex(),
        "7602d6196f953298273e9e3bdafeb60be4aaac9486e10f5ec61938ffdf3b0e16"
    );
}
