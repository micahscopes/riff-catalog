//! Self-describing references (invariant I7): the schema version, policy, and
//! facet always travel with a digest, so a bare hash never overclaims.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::SCHEMA_VERSION;
use crate::dimension::Dimension;
use crate::encode;
use crate::error::CatalogError;
use crate::policy::PolicyId;
use crate::text::Digest;

/// A single-dimension reference, parsable from/to a URI:
/// `riffcat:<schema>:<policy-hex>:<dimension>:<digest-hex>`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub schema_version: u32,
    pub policy_id: PolicyId,
    pub dimension: Dimension,
    pub digest: Digest,
}

impl ArtifactRef {
    pub fn new(policy_id: PolicyId, dimension: Dimension, digest: Digest) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            policy_id,
            dimension,
            digest,
        }
    }

    pub fn to_uri(&self) -> String {
        format!(
            "riffcat:{}:{}:{}:{}",
            self.schema_version,
            self.policy_id.to_hex(),
            self.dimension.as_str(),
            self.digest.to_hex()
        )
    }

    pub fn parse(uri: &str) -> Result<Self, CatalogError> {
        let invalid = |reason: &str| CatalogError::InvalidReference {
            reason: reason.to_string(),
        };
        let mut parts = uri.split(':');
        if parts.next() != Some("riffcat") {
            return Err(invalid("expected riffcat: scheme"));
        }
        let schema_version: u32 = parts
            .next()
            .ok_or_else(|| invalid("missing schema version"))?
            .parse()
            .map_err(|_| invalid("schema version is not a number"))?;
        let policy_id =
            Digest::from_hex(parts.next().ok_or_else(|| invalid("missing policy id"))?)?;
        let dimension = Dimension::parse(parts.next().ok_or_else(|| invalid("missing dimension"))?)
            .ok_or_else(|| invalid("unknown dimension"))?;
        let digest = Digest::from_hex(parts.next().ok_or_else(|| invalid("missing digest"))?)?;
        if parts.next().is_some() {
            return Err(invalid("trailing segments"));
        }
        Ok(Self {
            schema_version,
            policy_id,
            dimension,
            digest,
        })
    }
}

/// A facet: an equivalence given by a policy plus the dimension subset you
/// compare on. "Equal at facet F" = equality of [`FacetAddress::address_digest`].
///
/// Named constructors say what they *forget*, not what they keep.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Facet {
    pub policy_id: PolicyId,
    pub dimensions: BTreeSet<Dimension>,
}

impl Facet {
    pub fn new(
        policy_id: PolicyId,
        dimensions: impl IntoIterator<Item = Dimension>,
    ) -> Result<Self, CatalogError> {
        let dimensions = dimensions.into_iter().collect::<BTreeSet<_>>();
        if dimensions.is_empty() {
            return Err(CatalogError::EmptyDimensions);
        }
        Ok(Self {
            policy_id,
            dimensions,
        })
    }

    /// All five dimensions: the finest facet under this policy.
    pub fn full(policy_id: PolicyId) -> Self {
        Self {
            policy_id,
            dimensions: Dimension::ALL.into_iter().collect(),
        }
    }

    /// Everything except Names: forgets naming, keeps structure, constants,
    /// types, and trace events.
    pub fn names_blind(policy_id: PolicyId) -> Self {
        Self {
            policy_id,
            dimensions: Dimension::ALL
                .into_iter()
                .filter(|d| *d != Dimension::Names)
                .collect(),
        }
    }

    /// Structure only: forgets names, constants, types, and trace events.
    pub fn structure_only(policy_id: PolicyId) -> Self {
        Self {
            policy_id,
            dimensions: [Dimension::Structure].into_iter().collect(),
        }
    }

    pub fn facet_id(&self) -> Digest {
        encode::digest_meta("riffcat.facet", |bytes| {
            encode::push_digest(bytes, &self.policy_id);
            encode::push_u32(bytes, self.dimensions.len() as u32);
            for dimension in &self.dimensions {
                encode::push_str(bytes, dimension.as_str());
            }
        })
    }
}

/// A graph's digests projected onto a facet — the unit that indexes and the
/// claims layer key on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FacetAddress {
    pub facet: Facet,
    pub digests: BTreeMap<Dimension, Digest>,
}

impl FacetAddress {
    pub fn new(facet: Facet, digests: BTreeMap<Dimension, Digest>) -> Result<Self, CatalogError> {
        if digests.keys().copied().collect::<BTreeSet<_>>() != facet.dimensions {
            return Err(CatalogError::FacetDimensionMismatch);
        }
        Ok(Self { facet, digests })
    }

    /// The digest that defines "equal at this facet".
    pub fn address_digest(&self) -> Digest {
        encode::digest_meta("riffcat.facet_address", |bytes| {
            encode::push_digest(bytes, &self.facet.facet_id());
            for (dimension, digest) in &self.digests {
                encode::push_str(bytes, dimension.as_str());
                encode::push_digest(bytes, digest);
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_ref_uri_round_trip() {
        let r = ArtifactRef::new(
            Digest::from_bytes([3; 32]),
            Dimension::Structure,
            Digest::from_bytes([7; 32]),
        );
        let uri = r.to_uri();
        assert_eq!(ArtifactRef::parse(&uri).unwrap(), r);
        assert!(ArtifactRef::parse("riffcat:1:zz").is_err());
        assert!(ArtifactRef::parse("other:1:aa:structure:bb").is_err());
    }

    #[test]
    fn names_blind_forgets_only_names() {
        let facet = Facet::names_blind(Digest::from_bytes([0; 32]));
        assert!(!facet.dimensions.contains(&Dimension::Names));
        assert_eq!(facet.dimensions.len(), Dimension::ALL.len() - 1);
    }

    #[test]
    fn facet_address_requires_exact_dimension_set() {
        let facet = Facet::structure_only(Digest::from_bytes([0; 32]));
        let wrong: BTreeMap<Dimension, Digest> =
            [(Dimension::Names, Digest::from_bytes([1; 32]))].into();
        assert!(matches!(
            FacetAddress::new(facet.clone(), wrong),
            Err(CatalogError::FacetDimensionMismatch)
        ));
        let right: BTreeMap<Dimension, Digest> =
            [(Dimension::Structure, Digest::from_bytes([1; 32]))].into();
        FacetAddress::new(facet, right).unwrap();
    }

    #[test]
    fn facet_id_distinguishes_dimension_sets() {
        let policy_id = Digest::from_bytes([0; 32]);
        assert_ne!(
            Facet::full(policy_id).facet_id(),
            Facet::names_blind(policy_id).facet_id()
        );
    }
}
