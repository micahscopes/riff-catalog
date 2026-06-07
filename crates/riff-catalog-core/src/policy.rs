use serde::{Deserialize, Serialize};

use crate::SCHEMA_VERSION;
use crate::encode;
use crate::error::CatalogError;
use crate::text::{Digest, Name};

pub type PolicyId = Digest;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Algorithm {
    Blake3_256,
    /// Reserved; not implemented. Kept in the enum so references that name it
    /// parse, but hashing with it errors.
    Sha2_256,
}

impl Algorithm {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Blake3_256 => "blake3-256",
            Self::Sha2_256 => "sha2-256",
        }
    }
}

/// Whether node/graph keys participate in digests.
///
/// `IdentityBound` digests are stable provenance references; `AnonymousShape`
/// digests match structure across independently-keyed artifacts.
///
/// Design note (documented door): the interop conversation framed identity as
/// "a further projection of a facet" — i.e. a dimension you forget. We keep it
/// as a policy axis instead because node keys appear in *every* record kind
/// (`node.local`, `node.component_context`, `graph.full`, edge ordering), so
/// dimension-izing it would special-case every record and double the digest
/// count. The two modes are still two points in facet space (distinct
/// policy_ids), so references never overclaim; only the formal projection
/// relation between them is unexpressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewMode {
    IdentityBound,
    AnonymousShape,
}

impl ViewMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IdentityBound => "identity_bound",
            Self::AnonymousShape => "anonymous_shape",
        }
    }
}

/// How cycles among recursive edges (children + `Dependency` role) are treated.
///
/// - `Reject`: children **and** Dependency edges must be acyclic.
/// - `NonRecursiveGraphEdges`: only children must be acyclic; Dependency
///   cycles are tolerated (Dependency edges appear only as flat `graph.full`
///   records, like every other role).
/// - `CondenseScc`: children + Dependency feed SCC analysis; components are
///   WL-refined and hashed as units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CyclePolicy {
    Reject,
    NonRecursiveGraphEdges,
    CondenseScc,
}

impl CyclePolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reject => "reject",
            Self::NonRecursiveGraphEdges => "non_recursive_graph_edges",
            Self::CondenseScc => "condense_scc",
        }
    }
}

/// The encoding contract a digest was computed under.
///
/// Note there is deliberately **no dimension set** here: per-dimension digests
/// never depend on which other dimensions were requested, so baking the set
/// into the policy id would make identical digests incomparable (invariant I9).
/// The dimension set lives in [`crate::hash::DigestRequest`] (execution detail)
/// and [`crate::reference::Facet`] (the cross-artifact contract).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HashPolicy {
    pub schema_version: u32,
    pub algorithm: Algorithm,
    /// Open string identifying the *lowering contract*, versioned like a
    /// schema: e.g. "yul-ast/1", "yul-ssa-cfg/1", "sol-ast/1", "evm/1".
    /// Two producers at the same level must lower identically (invariant I10).
    pub level: Name,
    pub view_mode: ViewMode,
    pub cycle_policy: CyclePolicy,
}

impl HashPolicy {
    pub fn new(
        level: impl Into<String>,
        view_mode: ViewMode,
        cycle_policy: CyclePolicy,
    ) -> Result<Self, CatalogError> {
        Ok(Self {
            schema_version: SCHEMA_VERSION,
            algorithm: Algorithm::Blake3_256,
            level: Name::new(level, "policy level")?,
            view_mode,
            cycle_policy,
        })
    }

    /// Digest of the policy definition itself; the id carried by references.
    pub fn policy_id(&self) -> PolicyId {
        encode::digest_meta("riffcat.policy", |bytes| {
            encode::push_u32(bytes, self.schema_version);
            encode::push_str(bytes, self.algorithm.as_str());
            encode::push_str(bytes, self.level.as_str());
            encode::push_str(bytes, self.view_mode.as_str());
            encode::push_str(bytes, self.cycle_policy.as_str());
        })
    }

    pub(crate) fn check_supported(&self) -> Result<(), CatalogError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(CatalogError::SchemaVersionMismatch {
                found: self.schema_version,
                supported: SCHEMA_VERSION,
            });
        }
        if self.algorithm != Algorithm::Blake3_256 {
            return Err(CatalogError::UnsupportedAlgorithm {
                algorithm: self.algorithm.as_str(),
            });
        }
        Ok(())
    }
}
