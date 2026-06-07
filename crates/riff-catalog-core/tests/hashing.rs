//! Behavior tests for the hashing engine: the M3 (acyclic) and M4 (WL/SCC)
//! gates from PLAN.md. Each test names the invariant it enforces.

use std::collections::BTreeSet;

use riff_catalog_core::{
    CatalogError, CyclePolicy, DigestRequest, Dimension, EdgeRole, EntityKey, Graph, GraphKey,
    HashPolicy, NodeKey, ViewMode, digest_graph,
};

fn entity(kind: &str, owner: &str, local: &str) -> NodeKey {
    NodeKey::entity(EntityKey::new(kind, owner, local).unwrap())
}

fn graph_key(owner: &str) -> GraphKey {
    GraphKey::new(
        EntityKey::new("test.unit", owner, "unit:0").unwrap(),
        "unit",
    )
    .unwrap()
}

fn policy(view_mode: ViewMode, cycle_policy: CyclePolicy) -> HashPolicy {
    HashPolicy::new("test/1", view_mode, cycle_policy).unwrap()
}

fn digests(
    graph: &Graph,
    view_mode: ViewMode,
    cycle_policy: CyclePolicy,
) -> riff_catalog_core::GraphHashes {
    let request =
        DigestRequest::all_dimensions(graph.graph_key.clone(), policy(view_mode, cycle_policy));
    digest_graph(&request, graph).unwrap().hashes
}

fn structure_of(
    graph: &Graph,
    view_mode: ViewMode,
    cycle_policy: CyclePolicy,
) -> riff_catalog_core::Digest {
    *digests(graph, view_mode, cycle_policy)
        .graph
        .get(Dimension::Structure)
        .unwrap()
}

/// owner-parameterized literal graph: body -> literal(value)
fn literal_graph(owner: &str, value: u64) -> Graph {
    let mut graph = Graph::new(graph_key(owner));
    let body = entity("test.body", owner, "body:0");
    let lit = entity("test.expr", owner, "expr:0");
    graph.add_node(body.clone(), "body").unwrap();
    graph.add_node(lit.clone(), "literal").unwrap();
    graph
        .add_field(&lit, Dimension::Constants, "value", value)
        .unwrap();
    graph.add_child(&body, "expr", 0, &lit).unwrap();
    graph
}

// ---------------------------------------------------------------- M3: acyclic

/// I2: a Constants-only edit moves Constants and nothing else.
#[test]
fn dimension_purity() {
    let one = digests(
        &literal_graph("demo", 1),
        ViewMode::AnonymousShape,
        CyclePolicy::Reject,
    );
    let two = digests(
        &literal_graph("demo", 2),
        ViewMode::AnonymousShape,
        CyclePolicy::Reject,
    );
    assert_ne!(
        one.graph.get(Dimension::Constants),
        two.graph.get(Dimension::Constants)
    );
    for dimension in [
        Dimension::Structure,
        Dimension::Names,
        Dimension::Types,
        Dimension::TraceEvents,
    ] {
        assert_eq!(
            one.graph.get(dimension),
            two.graph.get(dimension),
            "{dimension:?}"
        );
    }
}

/// I6: field insertion order never matters.
#[test]
fn field_insertion_order_independence() {
    let owner = "demo";
    let build = |swap: bool| {
        let mut graph = Graph::new(graph_key(owner));
        let node = entity("test.node", owner, "n:0");
        graph.add_node(node.clone(), "node").unwrap();
        let fields = [("a", "first"), ("b", "second")];
        let order: Vec<_> = if swap {
            fields.iter().rev().collect()
        } else {
            fields.iter().collect()
        };
        for (name, value) in order {
            graph
                .add_field(&node, Dimension::Names, *name, *value)
                .unwrap();
        }
        graph
    };
    assert_eq!(
        digests(&build(false), ViewMode::IdentityBound, CyclePolicy::Reject),
        digests(&build(true), ViewMode::IdentityBound, CyclePolicy::Reject)
    );
}

/// I6: child ordinal is semantics — swapping it changes Structure.
#[test]
fn child_order_sensitivity() {
    let owner = "demo";
    let build = |swap: bool| {
        let mut graph = Graph::new(graph_key(owner));
        let body = entity("test.body", owner, "body:0");
        let a = entity("test.expr", owner, "expr:a");
        let b = entity("test.expr", owner, "expr:b");
        graph.add_node(body.clone(), "body").unwrap();
        graph.add_node(a.clone(), "literal").unwrap();
        graph.add_node(b.clone(), "name").unwrap();
        let (first, second) = if swap { (&b, &a) } else { (&a, &b) };
        graph.add_child(&body, "expr", 0, first).unwrap();
        graph.add_child(&body, "expr", 1, second).unwrap();
        graph
    };
    assert_ne!(
        structure_of(&build(false), ViewMode::AnonymousShape, CyclePolicy::Reject),
        structure_of(&build(true), ViewMode::AnonymousShape, CyclePolicy::Reject)
    );
}

/// I6 + label sensitivity: edge insertion order never matters, labels do.
#[test]
fn edge_insertion_order_independent_and_label_sensitive() {
    let owner = "demo";
    let build = |swap: bool, label: &str| {
        let mut graph = literal_graph(owner, 1);
        let body = entity("test.body", owner, "body:0");
        let lit = entity("test.expr", owner, "expr:0");
        let edges: [(&NodeKey, &str, &NodeKey, EdgeRole); 2] = [
            (&body, label, &lit, EdgeRole::Control),
            (&lit, "data:use", &body, EdgeRole::Data),
        ];
        let order: Vec<_> = if swap {
            edges.iter().rev().collect()
        } else {
            edges.iter().collect()
        };
        for (source, label, target, role) in order {
            graph.add_edge(source, *label, target, *role).unwrap();
        }
        graph
    };
    assert_eq!(
        digests(
            &build(false, "cfg:then"),
            ViewMode::IdentityBound,
            CyclePolicy::Reject
        ),
        digests(
            &build(true, "cfg:then"),
            ViewMode::IdentityBound,
            CyclePolicy::Reject
        )
    );
    assert_ne!(
        structure_of(
            &build(false, "cfg:then"),
            ViewMode::IdentityBound,
            CyclePolicy::Reject
        ),
        structure_of(
            &build(false, "cfg:else"),
            ViewMode::IdentityBound,
            CyclePolicy::Reject
        )
    );
}

/// I5: same shape under different keys — identity digests differ, anonymous
/// digests are equal.
#[test]
fn identity_and_anonymous_modes_separate() {
    let left = literal_graph("owner-a", 1);
    let right = literal_graph("owner-b", 1);
    assert_ne!(
        structure_of(&left, ViewMode::IdentityBound, CyclePolicy::Reject),
        structure_of(&right, ViewMode::IdentityBound, CyclePolicy::Reject)
    );
    // Anonymous graph-level digests fold all node digests, so comparing
    // `.graph` is the whole-graph comparison (nodes maps differ by key only).
    assert_eq!(
        digests(&left, ViewMode::AnonymousShape, CyclePolicy::Reject).graph,
        digests(&right, ViewMode::AnonymousShape, CyclePolicy::Reject).graph
    );
}

/// Node kind is structure: changing it moves the Structure digest.
#[test]
fn node_kind_is_structural() {
    let owner = "demo";
    let mut left = Graph::new(graph_key(owner));
    let mut right = Graph::new(graph_key(owner));
    let node = entity("test.expr", owner, "expr:0");
    left.add_node(node.clone(), "literal").unwrap();
    right.add_node(node.clone(), "name").unwrap();
    assert_ne!(
        structure_of(&left, ViewMode::AnonymousShape, CyclePolicy::Reject),
        structure_of(&right, ViewMode::AnonymousShape, CyclePolicy::Reject)
    );
}

/// Child cycles are rejected under both acyclic policies.
#[test]
fn child_cycles_rejected() {
    let owner = "demo";
    let mut graph = Graph::new(graph_key(owner));
    let a = entity("test.expr", owner, "a");
    let b = entity("test.expr", owner, "b");
    graph.add_node(a.clone(), "a").unwrap();
    graph.add_node(b.clone(), "b").unwrap();
    graph.add_child(&a, "next", 0, &b).unwrap();
    graph.add_child(&b, "next", 0, &a).unwrap();
    for cycle_policy in [CyclePolicy::Reject, CyclePolicy::NonRecursiveGraphEdges] {
        let request = DigestRequest::all_dimensions(
            graph.graph_key.clone(),
            policy(ViewMode::IdentityBound, cycle_policy),
        );
        assert!(matches!(
            digest_graph(&request, &graph),
            Err(CatalogError::CycleDetected { .. })
        ));
    }
}

/// The cycle policies are actually distinct (fix #8): Dependency cycles are
/// rejected by `Reject`, tolerated by `NonRecursiveGraphEdges`.
#[test]
fn dependency_cycles_distinguish_policies() {
    let owner = "demo";
    let mut graph = Graph::new(graph_key(owner));
    let a = entity("test.block", owner, "block:a");
    let b = entity("test.block", owner, "block:b");
    graph.add_node(a.clone(), "block").unwrap();
    graph.add_node(b.clone(), "block").unwrap();
    graph
        .add_edge(&a, "cfg:next", &b, EdgeRole::Dependency)
        .unwrap();
    graph
        .add_edge(&b, "cfg:back", &a, EdgeRole::Dependency)
        .unwrap();

    let reject = DigestRequest::all_dimensions(
        graph.graph_key.clone(),
        policy(ViewMode::IdentityBound, CyclePolicy::Reject),
    );
    assert!(matches!(
        digest_graph(&reject, &graph),
        Err(CatalogError::CycleDetected { .. })
    ));

    let tolerate = DigestRequest::all_dimensions(
        graph.graph_key.clone(),
        policy(ViewMode::IdentityBound, CyclePolicy::NonRecursiveGraphEdges),
    );
    digest_graph(&tolerate, &graph).unwrap();
}

// ---------------------------------------------------- M4: SCC + WL refinement

/// Two mutually-recursive "functions" with a body literal hanging off one.
/// `spelling` perturbs every key; structure is identical.
fn recursive_pair(owner: &str, spelling: &str, lit: u64) -> Graph {
    let mut graph = Graph::new(graph_key(owner));
    let f = entity("test.fn", owner, &format!("fn:{spelling}:f"));
    let g = entity("test.fn", owner, &format!("fn:{spelling}:g"));
    let body = entity("test.expr", owner, &format!("expr:{spelling}:0"));
    graph.add_node(f.clone(), "function").unwrap();
    graph.add_node(g.clone(), "function").unwrap();
    graph.add_node(body.clone(), "literal").unwrap();
    graph
        .add_field(&body, Dimension::Constants, "value", lit)
        .unwrap();
    graph
        .add_edge(&f, "calls", &g, EdgeRole::Dependency)
        .unwrap();
    graph
        .add_edge(&g, "calls", &f, EdgeRole::Dependency)
        .unwrap();
    graph.add_child(&f, "body", 0, &body).unwrap();
    graph
}

/// HEADLINE (I5 fix): isomorphic recursive SCCs with different key spellings
/// hash EQUAL anonymously and DIFFERENT identity-bound.
#[test]
fn isomorphic_sccs_with_different_keys_match_anonymously() {
    let left = recursive_pair("owner-a", "alpha", 7);
    let right = recursive_pair("owner-b", "zeta", 7);
    assert_eq!(
        digests(&left, ViewMode::AnonymousShape, CyclePolicy::CondenseScc).graph,
        digests(&right, ViewMode::AnonymousShape, CyclePolicy::CondenseScc).graph
    );
    assert_ne!(
        structure_of(&left, ViewMode::IdentityBound, CyclePolicy::CondenseScc),
        structure_of(&right, ViewMode::IdentityBound, CyclePolicy::CondenseScc)
    );
}

/// Non-isomorphic cyclic graphs differ: 2-cycle vs 3-cycle.
#[test]
fn cycle_length_is_structural() {
    let cycle = |owner: &str, n: usize| {
        let mut graph = Graph::new(graph_key(owner));
        let keys: Vec<_> = (0..n)
            .map(|i| entity("test.fn", owner, &format!("fn:{i}")))
            .collect();
        for key in &keys {
            graph.add_node(key.clone(), "function").unwrap();
        }
        for i in 0..n {
            graph
                .add_edge(&keys[i], "calls", &keys[(i + 1) % n], EdgeRole::Dependency)
                .unwrap();
        }
        graph
    };
    assert_ne!(
        structure_of(
            &cycle("demo", 2),
            ViewMode::AnonymousShape,
            CyclePolicy::CondenseScc
        ),
        structure_of(
            &cycle("demo", 3),
            ViewMode::AnonymousShape,
            CyclePolicy::CondenseScc
        )
    );
}

/// Same-size cycles with different edge labels differ in Structure.
#[test]
fn scc_edge_labels_are_structural() {
    let cycle = |label: &str| {
        let owner = "demo";
        let mut graph = Graph::new(graph_key(owner));
        let a = entity("test.block", owner, "block:a");
        let b = entity("test.block", owner, "block:b");
        graph.add_node(a.clone(), "block").unwrap();
        graph.add_node(b.clone(), "block").unwrap();
        graph.add_edge(&a, label, &b, EdgeRole::Dependency).unwrap();
        graph
            .add_edge(&b, "cfg:back", &a, EdgeRole::Dependency)
            .unwrap();
        graph
    };
    assert_ne!(
        structure_of(
            &cycle("cfg:next"),
            ViewMode::AnonymousShape,
            CyclePolicy::CondenseScc
        ),
        structure_of(
            &cycle("cfg:else"),
            ViewMode::AnonymousShape,
            CyclePolicy::CondenseScc
        )
    );
}

/// Symmetric orbits (label-identical 2-cycle) are deterministic and stable
/// across edge insertion order.
#[test]
fn symmetric_orbit_determinism() {
    let build = |swap: bool| {
        let owner = "demo";
        let mut graph = Graph::new(graph_key(owner));
        let a = entity("test.block", owner, "block:a");
        let b = entity("test.block", owner, "block:b");
        graph.add_node(a.clone(), "block").unwrap();
        graph.add_node(b.clone(), "block").unwrap();
        let edges = [(&a, &b), (&b, &a)];
        let order: Vec<_> = if swap {
            edges.iter().rev().collect()
        } else {
            edges.iter().collect()
        };
        for (src, dst) in order {
            graph
                .add_edge(src, "next", dst, EdgeRole::Dependency)
                .unwrap();
        }
        graph
    };
    assert_eq!(
        digests(
            &build(false),
            ViewMode::AnonymousShape,
            CyclePolicy::CondenseScc
        ),
        digests(
            &build(true),
            ViewMode::AnonymousShape,
            CyclePolicy::CondenseScc
        )
    );
}

/// Regression for prototype flaw #6: child ordinals are hash-sensitive under
/// CondenseScc (the prototype dropped them from recursive edge records).
#[test]
fn child_ordinals_sensitive_under_condense_scc() {
    let owner = "demo";
    let build = |swap: bool| {
        let mut graph = Graph::new(graph_key(owner));
        let body = entity("test.body", owner, "body:0");
        let a = entity("test.expr", owner, "expr:a");
        let b = entity("test.expr", owner, "expr:b");
        graph.add_node(body.clone(), "body").unwrap();
        graph.add_node(a.clone(), "literal").unwrap();
        graph.add_node(b.clone(), "name").unwrap();
        let (first, second) = if swap { (&b, &a) } else { (&a, &b) };
        graph.add_child(&body, "expr", 0, first).unwrap();
        graph.add_child(&body, "expr", 1, second).unwrap();
        graph
    };
    assert_ne!(
        structure_of(
            &build(false),
            ViewMode::AnonymousShape,
            CyclePolicy::CondenseScc
        ),
        structure_of(
            &build(true),
            ViewMode::AnonymousShape,
            CyclePolicy::CondenseScc
        )
    );
}

/// Acyclic graphs hash consistently under CondenseScc too (all singletons),
/// and anonymous equality survives key renames in a mixed graph.
#[test]
fn mixed_graph_stable_under_key_renames() {
    let left = recursive_pair("owner-a", "spell-one", 9);
    let right = recursive_pair("owner-a", "spell-two", 9);
    assert_eq!(
        digests(&left, ViewMode::AnonymousShape, CyclePolicy::CondenseScc).graph,
        digests(&right, ViewMode::AnonymousShape, CyclePolicy::CondenseScc).graph
    );
}

/// Components are reported with members and colors; the recursive pair forms
/// one 2-member component plus a singleton for the literal.
#[test]
fn component_reporting() {
    let graph = recursive_pair("demo", "alpha", 7);
    let hashes = digests(&graph, ViewMode::AnonymousShape, CyclePolicy::CondenseScc);
    let sizes: Vec<usize> = hashes.components.iter().map(|c| c.members.len()).collect();
    assert!(sizes.contains(&2), "expected a 2-member SCC, got {sizes:?}");
    assert_eq!(
        hashes
            .components
            .iter()
            .map(|c| c.members.len())
            .sum::<usize>(),
        3
    );
}

/// WL distinguishes members in asymmetric cycles: a 2-cycle where one member
/// has a child must give its members different colors.
#[test]
fn wl_separates_asymmetric_members() {
    let graph = recursive_pair("demo", "alpha", 7);
    let hashes = digests(&graph, ViewMode::AnonymousShape, CyclePolicy::CondenseScc);
    let component = hashes
        .components
        .iter()
        .find(|c| c.members.len() == 2)
        .expect("recursive component");
    let colors: BTreeSet<_> = component
        .member_colors
        .values()
        .map(|d| *d.get(Dimension::Structure).unwrap())
        .collect();
    assert_eq!(colors.len(), 2, "f (has body child) and g must color apart");
}
