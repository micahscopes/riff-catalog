//! Golden fixtures that freeze the on-the-wire form of the interchange contract.
//!
//! Each type is serialized and asserted byte-identical to a checked-in fixture,
//! then round-tripped back. The fixture files are the shared artifact a second
//! producer (fe) asserts against on its side, so a rename or a reordering that
//! would silently desync the two repos fails here first. Regenerate after an
//! intentional encoding change with `UPDATE_FIXTURES=1 cargo test -p
//! riff-catalog-schema --test wire_fixtures`, then review the diff.

use std::collections::BTreeMap;
use std::fmt::Debug;

use serde::Serialize;
use serde::de::DeserializeOwned;

use riff_catalog_schema::{
    Dimension, Digest, EdgeRole, EntityKey, Field, FingerprintRecord, Graph, GraphKey, NodeKey,
};

fn ek(kind: &str, owner: &str, local: &str) -> EntityKey {
    EntityKey::new(kind, owner, local).unwrap()
}

/// A tiny lowering that carries a provenance (Origin) edge: one MIR statement
/// lowered from one HIR expression, with a field on each side.
fn sample_graph() -> Graph {
    let mut g = Graph::new(GraphKey::new(ek("mir.body", "pkg:token", "transfer"), "body").unwrap());
    let src = NodeKey::entity(ek("hir.expr", "pkg:token", "expr:7"));
    let lowered = NodeKey::entity(ek("mir.stmt", "pkg:token", "stmt:3"));
    g.add_node(src.clone(), "expr").unwrap();
    g.add_node(lowered.clone(), "stmt").unwrap();
    g.add_field(&src, Dimension::Names, "ident", "amount").unwrap();
    g.add_field(&lowered, Dimension::Structure, "op", "add").unwrap();
    g.add_child(&lowered, "operand", 0, &src).unwrap();
    // Provenance: the statement came from the expression. Origin edges are
    // payload, not shape (the engine excludes them from the structural digest).
    g.add_edge(&lowered, "lowered_from", &src, EdgeRole::Origin)
        .unwrap();
    g.validate().unwrap();
    g
}

/// The same shape as [`sample_graph`], but the provenance edge carries a
/// payload field (the compiler phase that introduced it). This freezes the
/// edge-with-fields wire form the fe origin-bundle reader emits. The payload is
/// inert to every facet address (Origin edges are excluded from the fold).
fn sample_graph_edge_fields() -> Graph {
    let mut g = Graph::new(GraphKey::new(ek("mir.body", "pkg:token", "transfer"), "body").unwrap());
    let src = NodeKey::entity(ek("hir.expr", "pkg:token", "expr:7"));
    let lowered = NodeKey::entity(ek("mir.stmt", "pkg:token", "stmt:3"));
    g.add_node(src.clone(), "expr").unwrap();
    g.add_node(lowered.clone(), "stmt").unwrap();
    g.add_child(&lowered, "operand", 0, &src).unwrap();
    g.add_edge_with_fields(
        &lowered,
        "lowered_from",
        &src,
        EdgeRole::Origin,
        vec![Field::new(Dimension::Names, "introduced_by", "mir").unwrap()],
    )
    .unwrap();
    g.validate().unwrap();
    g
}

fn sample_record() -> FingerprintRecord {
    let digests = BTreeMap::from([
        (Dimension::Structure, Digest::from_bytes([0x11; 32])),
        (Dimension::Names, Digest::from_bytes([0x22; 32])),
        (Dimension::Constants, Digest::from_bytes([0x33; 32])),
        (Dimension::Types, Digest::from_bytes([0x44; 32])),
    ]);
    FingerprintRecord::new(
        GraphKey::new(ek("yul.object", "pkg:token", "Token"), "runtime").unwrap(),
        Digest::from_bytes([0xaa; 32]),
        2,
        digests,
        7,
    )
}

fn golden<T>(name: &str, value: &T)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let json = serde_json::to_string_pretty(value).unwrap();
    let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    if std::env::var("UPDATE_FIXTURES").is_ok() {
        std::fs::write(&path, format!("{json}\n")).unwrap();
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("read fixture {path}: {e} (run with UPDATE_FIXTURES=1 to create it)")
    });
    let want = want.trim_end_matches('\n');
    assert_eq!(json, want, "serialized `{name}` drifted from its frozen fixture");
    let back: T = serde_json::from_str(want).unwrap();
    assert_eq!(&back, value, "fixture `{name}` did not round-trip");
}

#[test]
fn wire_forms_are_frozen() {
    golden("entity_key", &ek("hir.expr", "pkg:token", "expr:7"));
    golden(
        "node_key_entity",
        &NodeKey::entity(ek("hir.expr", "pkg:token", "expr:7")),
    );
    golden(
        "node_key_derived",
        &NodeKey::derived(ek("mir.body", "pkg:token", "transfer"), "tmp:2").unwrap(),
    );
    golden(
        "graph_key",
        &GraphKey::new(ek("yul.object", "pkg:token", "Token"), "runtime").unwrap(),
    );
    golden("graph_with_origin", &sample_graph());
    golden("graph_with_origin_edge_fields", &sample_graph_edge_fields());
    golden("fingerprint_record", &sample_record());
}

/// fe emits its origin keys with `owner_key` / `local_key` field names; the
/// serde aliases let those parse into `EntityKey` unchanged during the rename
/// window, while the canonical serialized form stays `owner` / `local`.
#[test]
fn entity_key_accepts_fe_field_names() {
    let fe = r#"{"kind":"hir.expr","owner_key":"pkg:token","local_key":"expr:7"}"#;
    let parsed: EntityKey = serde_json::from_str(fe).unwrap();
    assert_eq!(parsed, ek("hir.expr", "pkg:token", "expr:7"));
    // and the canonical form still serializes with the new names
    let canonical = serde_json::to_string(&parsed).unwrap();
    assert!(canonical.contains("\"owner\""));
    assert!(!canonical.contains("owner_key"));
}
