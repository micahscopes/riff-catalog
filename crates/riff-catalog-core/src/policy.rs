use serde::{Deserialize, Serialize};

use crate::SCHEMA_VERSION;
use crate::encode;
use crate::error::CatalogError;
use crate::text::{Digest, Name};

pub type PolicyId = Digest;

/// Whether node keys take part in digests.
///
/// - `Bound`: keys pair nodes, so two graphs agree only node for node. Use it to
///   compare one thing across worlds when both sides key it the same way (for
///   example by source position).
/// - `Free`: graphs are compared up to renaming of keys. No byte derived from a
///   key enters any digest (invariant I5).
///
/// Keys mode is not the same as ignoring the `names` dimension: a `Free` digest
/// still counts every `names` fact, and a `Bound` digest that leaves out
/// `names` still tells nodes apart by key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Keys {
    Bound,
    Free,
}

impl Keys {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bound => "bound",
            Self::Free => "free",
        }
    }
}

/// What a digest was computed under: the format version, the recipe that made
/// the graph, and the keys mode.
///
/// The recipe names the function from an artifact to a graph, versioned like a
/// schema (`yul-ast/1`, `fe-query-record/1`). Two producers that use the same
/// recipe must make the same graph from the same artifact (invariant I10).
///
/// There is deliberately no dimension set here: a dimension's digest never
/// depends on which other dimensions were requested (invariant I9). The set
/// lives in [`crate::hash::DigestRequest`] and [`crate::reference::Facet`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HashPolicy {
    pub schema_version: u32,
    pub recipe: Name,
    pub keys: Keys,
}

impl HashPolicy {
    pub fn new(recipe: impl Into<String>, keys: Keys) -> Result<Self, CatalogError> {
        Ok(Self {
            schema_version: SCHEMA_VERSION,
            recipe: Name::new(recipe, "policy recipe")?,
            keys,
        })
    }

    /// Digest of the policy itself; the id references carry.
    pub fn policy_id(&self) -> PolicyId {
        encode::digest_meta("riffcat.policy", |bytes| {
            encode::push_u32(bytes, self.schema_version);
            encode::push_str(bytes, self.recipe.as_str());
            encode::push_str(bytes, self.keys.as_str());
        })
    }

    pub(crate) fn check_supported(&self) -> Result<(), CatalogError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(CatalogError::SchemaVersionMismatch {
                found: self.schema_version,
                supported: SCHEMA_VERSION,
            });
        }
        Ok(())
    }
}
