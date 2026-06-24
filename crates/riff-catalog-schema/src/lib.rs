//! riff-catalog-schema: the pure-data interchange contract.
//!
//! The canonical graph form (nodes with dimension-tagged fields, ordered labeled
//! children which form the Merkle skeleton, and role-tagged edges), the identity
//! keys, the closed dimension set, field values, and the digest and name
//! newtypes. No hashing lives here.
//!
//! A producer (a compiler such as fe, or any tool) can depend on this crate to
//! speak riff-catalog's wire format without linking the hashing engine
//! (riff-catalog-core) or any backend adapter. The engine depends on this crate
//! and re-exports it, so `riff_catalog_core::{Graph, EntityKey, ...}` keeps
//! working unchanged.

pub mod dimension;
pub mod error;
pub mod graph;
pub mod key;
pub mod serde_pairs;
pub mod text;
pub mod value;

pub use dimension::Dimension;
pub use error::CatalogError;
pub use graph::{ChildEdge, Edge, EdgeRole, Field, Graph, GraphSink, Node};
pub use key::{EntityKey, GraphKey, NodeKey};
pub use text::{Digest, Name};
pub use value::Value;
