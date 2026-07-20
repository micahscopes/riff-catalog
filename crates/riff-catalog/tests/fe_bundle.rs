//! End-to-end verification on a real fe trace bundle.
//!
//! Reads the origin graph out of an actual `fe dev trace emit` bundle, digests
//! it through the same facade entry a producer would call, and checks the counts
//! and a stable facet address. This proves fe -> riffcat works on real compiler
//! output, not a fixture. The bundle lives outside the repo, so the test skips
//! (rather than fails) when it is absent, keeping `cargo test` green anywhere.

use std::path::Path;

use riff_catalog::{ArtifactRef, Dimension, EdgeRole, Facet, ingest_graph, ingest_trace_bundle};

const BUNDLE_PATH: &str = "/workspace/fe-trace-example.jsonl";

#[test]
fn real_fe_bundle_ingests_to_a_stable_origin_graph() {
    if !Path::new(BUNDLE_PATH).exists() {
        eprintln!("skipping: {BUNDLE_PATH} not present");
        return;
    }
    let jsonl = std::fs::read_to_string(BUNDLE_PATH).unwrap();

    let graphs = ingest_trace_bundle(&jsonl).unwrap();
    assert_eq!(graphs.len(), 1, "the bundle folds into one origin graph");
    let graph = &graphs[0];

    // The counts fe emitted: 4503 origin nodes, 1357 origin edges.
    assert_eq!(graph.nodes.len(), 4503, "origin node count");
    assert_eq!(graph.edges.len(), 1357, "origin edge count");
    assert!(
        graph.edges.iter().all(|e| e.role == EdgeRole::Origin),
        "every ingested edge is provenance"
    );
    graph.validate().unwrap();

    // Digest it, and confirm the address is deterministic across two ingests.
    let hashes = ingest_graph(graph, "fe.origin.v1").unwrap();
    let again = ingest_graph(graph, "fe.origin.v1").unwrap();

    let structure = Facet::structure_only(hashes.policy_id);
    let addr = hashes.facet_address(&structure).unwrap().address_digest();
    let addr_again = again.facet_address(&structure).unwrap().address_digest();
    assert_eq!(addr, addr_again, "the facet address is stable");

    let structure_digest = *hashes.graph.get(Dimension::Structure).unwrap();
    let artifact = ArtifactRef::new(hashes.policy_id, Dimension::Structure, structure_digest);

    eprintln!("fe bundle -> riff origin graph:");
    eprintln!("  nodes            = {}", graph.nodes.len());
    eprintln!("  origin edges     = {}", graph.edges.len());
    eprintln!("  policy_id        = {}", hashes.policy_id.to_hex());
    eprintln!("  structure facet  = {}", addr.to_hex());
    eprintln!("  sample ArtifactRef = {}", artifact.to_uri());
}
