//! riff-catalog-core: facet-relative content addressing for compiler artifacts.
//!
//! An artifact is lowered into a [`Graph`] - nodes with dimension-tagged fields,
//! ordered labeled children (the Merkle skeleton), and labeled role-tagged edges.
//! [`digest_graph`] computes one digest *per dimension* under a [`HashPolicy`];
//! "equal at facet F" is equality of the digests for F's dimension subset
//! ([`FacetAddress::address_digest`]).
//!
//! Design invariants (see PLAN.md at the workspace root for the full list):
//! - I1: every digest record's header commits to schema version, recipe,
//!   dimension and keys mode - a bare hash never overclaims.
//! - I5: in [`Keys::Free`], no node-key-derived bytes influence any
//!   digest; refined colours replace key order.
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
use riff_catalog_schema::serde_pairs;
pub use riff_catalog_schema::{dimension, error, graph, key, recipe, record, text, unit, value};

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
pub use hash::{
    check_acyclic, check_recipe, dimensions_key, facet_address, joint_digest, joint_digests,
    mask_needs_joint, pair_bit,
};
pub use policy::{HashPolicy, Keys, PolicyId};
pub use recipe::{
    Acyclic, ChildDecl, EdgeDecl, FieldDecl, KindDecl, LabelPattern, Occurs, RecipeDecl, Shape,
    ValueType, Violation,
};
pub use record::FingerprintRecord;
pub use reference::{AddressValue, ArtifactRef, Facet, FacetAddress};
pub use root::set_root;
pub use text::{Digest, Name};
pub use unit::{Status, Unit, read_units, write_units};
pub use value::Value;

/// Version of the canonical encoding contract. Any change to record layouts,
/// tags, or canonicalization rules requires bumping this (invariant I8).
///
/// v2: edge topology (role/label/ordinal) is bound into every dimension's edge
/// records, not only Structure, so a rewiring or argument-reorder moves every
/// dimension's digest. Fixes the `f(1, 2)` vs `f(2, 1)` collision at the
/// Constants/Names/Types facets and the flat-edge role-swap collision.
///
/// v3: one fold for every cycle policy (the condensed fold), and `graph.full`
/// refines colours over the whole skeleton, so in free mode a node is known by
/// its full context and a flat edge cannot be swapped between equal-content
/// nodes unnoticed. The graph key is no longer hashed. The policy is the
/// recipe and the keys mode only: the cycle policy and the algorithm name are
/// gone from the header and the policy id.
pub const SCHEMA_VERSION: u32 = 3;
