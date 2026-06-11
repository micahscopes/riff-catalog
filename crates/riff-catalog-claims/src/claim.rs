use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use riff_catalog_core::meta::MetaRecord;
use riff_catalog_core::{Digest, Facet, FacetAddress, Name};

use crate::CLAIMS_SCHEMA_VERSION;
use crate::error::ClaimsError;

/// The relation a claim asserts. Non-exhaustive on purpose: refinements
/// (e.g. directional refines-into, observationally-equivalent-under-tests)
/// are expected to grow here, each a schema change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Relation {
    Equivalent,
}

impl Relation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Equivalent => "equivalent",
        }
    }
}

/// A witness: who/what backs the assertion. `payload` is ORDERED — field
/// correspondences are order-sensitive (a scrambled order is a *different*,
/// equally valid-looking witness; that is the point of the wrong-witness
/// demo). Semantics belong to `kind`'s auditor; never validated here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Witness {
    pub kind: Name,
    pub payload: Vec<(String, String)>,
}

impl Witness {
    pub fn new(
        kind: impl Into<String>,
        payload: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Self, ClaimsError> {
        Ok(Self {
            kind: Name::new(kind, "witness kind").map_err(ClaimsError::Core)?,
            payload: payload.into_iter().collect(),
        })
    }
}

/// `left ≅ right at facet, witnessed` — conditional on `assumptions` when
/// present. `note` is display-only and excluded from [`Claim::claim_id`].
///
/// `assumptions` (schema v2, adopted from Ixon) is a merkle root over an
/// assumption set (see `riff_catalog_core::set_root`): the claim holds
/// *modulo* that set (e.g. cross-optimization equivalence modulo
/// no-overflow). A conditional claim never merges unless the querier
/// explicitly accepts its root — see [`ClaimSet::closure_for_facet_assuming`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub schema_version: u32,
    pub relation: Relation,
    pub facet: Facet,
    pub left: FacetAddress,
    pub right: FacetAddress,
    pub witness: Witness,
    pub note: Option<String>,
    /// Merkle root of the assumption set this claim is conditional on.
    /// Optional and skipped when absent, so v1 records round-trip untouched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assumptions: Option<Digest>,
}

impl Claim {
    pub fn new(
        facet: Facet,
        left: FacetAddress,
        right: FacetAddress,
        witness: Witness,
        note: Option<String>,
        assumptions: Option<Digest>,
    ) -> Result<Self, ClaimsError> {
        if left.facet != facet || right.facet != facet {
            return Err(ClaimsError::FacetMismatch);
        }
        Ok(Self {
            schema_version: CLAIMS_SCHEMA_VERSION,
            relation: Relation::Equivalent,
            facet,
            left,
            right,
            witness,
            note,
            assumptions,
        })
    }

    pub fn claim_id(&self) -> Digest {
        let mut record = MetaRecord::new("riffcat.claim");
        record
            .push_u32(self.schema_version)
            .push_str(self.relation.as_str())
            .push_digest(&self.facet.facet_id())
            .push_digest(&self.left.address_digest())
            .push_digest(&self.right.address_digest())
            .push_str(self.witness.kind.as_str())
            .push_u32(self.witness.payload.len() as u32);
        for (key, value) in &self.witness.payload {
            record.push_str(key).push_str(value);
        }
        // Hashed only when present: the payload length above already pins
        // the witness fields, so the marker cannot be confused with payload
        // bytes, and unconditional claims keep a stable encoding.
        if let Some(assumptions) = &self.assumptions {
            record.push_str("assumptions").push_digest(assumptions);
        }
        record.finish()
    }
}

/// Deduplicating set of claims, keyed by claim id.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimSet {
    claims: BTreeMap<Digest, Claim>,
}

impl ClaimSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns false if an identical claim (same id) was already present.
    pub fn insert(&mut self, claim: Claim) -> bool {
        self.claims.insert(claim.claim_id(), claim).is_none()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Digest, &Claim)> {
        self.claims.iter()
    }

    pub fn len(&self) -> usize {
        self.claims.len()
    }

    pub fn is_empty(&self) -> bool {
        self.claims.is_empty()
    }

    pub fn get(&self, claim_id: &Digest) -> Option<&Claim> {
        self.claims.get(claim_id)
    }
}
