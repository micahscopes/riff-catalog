//! Hermetic tests for the origin-bundle reader: the mapping, tolerance to the
//! non-origin fact kinds fe's stream carries, and the edge provenance payload.
//! No hashing engine here; the engine-side inertness contract lives in the
//! `riff-catalog` facade.

use riff_catalog_ingest_trace::ingest_trace_bundle;
use riff_catalog_schema::{Dimension, EdgeRole, NodeKey, Value};

/// A miniature bundle in fe's exact wire form: a metadata header, three origin
/// nodes, two origin edges (one with an `introduced_by` phase, one without), and
/// several non-origin and unknown record kinds that must all be skipped.
const BUNDLE: &str = r#"
{"record":"metadata","schema_version":1,"input_path":"demo.fe"}
{"record":"fact","type":"origin_node","key":{"kind":"hir.expr","owner_key":"pkg:demo","local_key":"0"}}
{"record":"fact","type":"origin_node","key":{"kind":"runtime.stmt","owner_key":"pkg:demo","local_key":"block:0:stmt:0"}}
{"record":"fact","type":"instruction","instruction":{"kind":"bytecode.pc","owner_key":"pkg:demo","local_key":"pc:0"},"function":{"kind":"bytecode.function","owner_key":"pkg:demo","local_key":"f"},"index":0,"mnemonic":"PUSH1"}
{"record":"fact","type":"origin_node","key":{"kind":"runtime.local","owner_key":"pkg:demo","local_key":"local:a"}}
{"record":"fact","type":"origin_edge","from":{"kind":"runtime.stmt","owner_key":"pkg:demo","local_key":"block:0:stmt:0"},"to":{"kind":"hir.expr","owner_key":"pkg:demo","local_key":"0"},"label":"lowered_from","introduced_by":"mir"}
{"record":"fact","type":"source_span","origin":{"kind":"hir.expr","owner_key":"pkg:demo","local_key":"0"},"file":{"kind":"source.file","owner_key":"pkg:demo","local_key":"demo.fe"},"start_byte":0,"end_byte":1,"start_line":1,"start_column":0,"end_line":1,"end_column":1}
{"record":"fact","type":"origin_edge","from":{"kind":"runtime.local","owner_key":"pkg:demo","local_key":"local:a"},"to":{"kind":"hir.expr","owner_key":"pkg:demo","local_key":"0"},"label":"emitted_from"}
{"record":"fact","type":"a_kind_this_reader_has_never_heard_of","payload":{"anything":true}}
"#;

#[test]
fn maps_origin_facts_and_skips_everything_else() {
    let graphs = ingest_trace_bundle(BUNDLE).unwrap();
    assert_eq!(graphs.len(), 1, "one origin graph spans the whole bundle");
    let g = &graphs[0];

    // Three origin nodes; the instruction / source_span / unknown records are
    // skipped, so they contribute no nodes.
    assert_eq!(g.nodes.len(), 3, "only origin_node records become nodes");
    assert_eq!(g.edges.len(), 2, "only origin_edge records become edges");

    // The graph is keyed off the bundle's input_path.
    assert_eq!(g.graph_key.owner.owner(), "demo.fe");

    // Every node is keyed by its entity, with the entity kind as node kind.
    for (key, node) in &g.nodes {
        assert!(matches!(key, NodeKey::Entity(_)));
        assert_eq!(node.kind.as_str(), key.owner().kind());
    }

    // Every edge is an Origin edge; nothing else fabricated another role.
    assert!(g.edges.iter().all(|e| e.role == EdgeRole::Origin));

    // The first edge carries its phase as a payload field; the second (no
    // `introduced_by`) carries none.
    let lowered = g
        .edges
        .iter()
        .find(|e| e.label.as_str() == "lowered_from")
        .expect("lowered_from edge present");
    assert_eq!(lowered.fields.len(), 1);
    assert_eq!(lowered.fields[0].dimension, Dimension::Names);
    assert_eq!(lowered.fields[0].name.as_str(), "introduced_by");
    assert_eq!(lowered.fields[0].value, Value::Text("mir".to_string()));

    let emitted = g
        .edges
        .iter()
        .find(|e| e.label.as_str() == "emitted_from")
        .expect("emitted_from edge present");
    assert!(emitted.fields.is_empty(), "no phase means no payload field");

    // The graph is internally consistent: every edge endpoint is a node.
    g.validate().unwrap();
}

#[test]
fn a_bundle_with_no_origin_facts_yields_no_graph() {
    let only_metadata_and_other_facts = concat!(
        "{\"record\":\"metadata\",\"schema_version\":1,\"input_path\":\"x.fe\"}\n",
        "{\"record\":\"fact\",\"type\":\"block\",\"block\":{\"kind\":\"runtime.block\",\"owner_key\":\"o\",\"local_key\":\"b0\"},",
        "\"function\":{\"kind\":\"runtime.function\",\"owner_key\":\"o\",\"local_key\":\"f\"},\"phase\":\"mir\",\"ordinal\":0,\"name\":null}\n"
    );
    let graphs = ingest_trace_bundle(only_metadata_and_other_facts).unwrap();
    assert!(graphs.is_empty(), "no origin facts, no origin graph");
}

#[test]
fn blank_lines_are_ignored_and_bad_json_is_located() {
    // Blank lines are fine.
    assert!(ingest_trace_bundle("\n\n\n").unwrap().is_empty());

    // A malformed line reports its 1-based line number.
    let err = ingest_trace_bundle("{\"record\":\"metadata\"}\nnot json\n").unwrap_err();
    assert!(err.to_string().contains("line 2"), "got: {err}");
}
