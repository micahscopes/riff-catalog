//! riff-catalog-core: facet-relative content addressing for compiler artifacts.
//!
//! An artifact is lowered into a [`Graph`] - nodes with dimension-tagged fields,
//! ordered labeled children (the Merkle skeleton), and labeled role-tagged edges.
//! [`digest_graph`] computes one digest *per dimension* under a [`HashPolicy`];
//! "equal at facet F" is equality of the digests for F's dimension subset
//! ([`FacetAddress::address_digest`]).
//!
//! Design invariants (see PLAN.md at the workspace root for the full list):
//! - I1: every digest record's header commits to schema version, algorithm,
//!   level, view mode, cycle policy, and dimension - a bare hash never overclaims.
//! - I5: in [`ViewMode::AnonymousShape`], no node-key-derived bytes influence any
//!   digest; WL colors replace key order inside SCCs.
//! - I8: any change to the encoding requires a [`SCHEMA_VERSION`] bump; golden
//!   tests enforce this mechanically.

pub mod hash;
pub mod index;
pub mod meta;
pub mod policy;
pub mod reference;
pub mod root;

mod encode;

// The pure-data interchange contract lives in riff-catalog-schema. Re-export its
// modules at the crate root so existing `riff_catalog_core::{...}` item paths and
// the internal `crate::<module>::Item` references in the hashing modules keep
// resolving unchanged. Hashing (encode, hash, policy, reference, index) stays here.
pub use riff_catalog_schema::{dimension, error, graph, key, text, value};
use riff_catalog_schema::serde_pairs;

pub use dimension::Dimension;
pub use error::CatalogError;
pub use graph::{ChildEdge, Edge, EdgeRole, Field, Graph, GraphSink, Node};
pub use hash::{
    ComponentHash, DigestRequest, DigestResult, DimensionDigests, GraphHashes, NodeHashes,
    digest_graph,
};
pub use index::{
    DigestIndex, DigestIndexEntry, FacetIndex, FacetIndexEntry, LookupRequest, LookupResult,
};
pub use key::{EntityKey, GraphKey, NodeKey};
pub use policy::{Algorithm, CyclePolicy, HashPolicy, PolicyId, ViewMode};
pub use reference::{ArtifactRef, Facet, FacetAddress};
pub use root::set_root;
pub use text::{Digest, Name};
pub use value::Value;

/// Version of the canonical encoding contract. Any change to record layouts,
/// tags, or canonicalization rules requires bumping this (invariant I8).
///
/// v2: edge topology (role/label/ordinal) is bound into every dimension's edge
/// records, not only Structure, so a rewiring or argument-reorder moves every
/// dimension's digest. Fixes the `f(1, 2)` vs `f(2, 1)` collision at the
/// Constants/Names/Types facets and the flat-edge role-swap collision.
pub const SCHEMA_VERSION: u32 = 2;
