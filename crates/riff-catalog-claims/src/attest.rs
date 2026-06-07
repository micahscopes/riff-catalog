use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use riff_catalog_core::meta::MetaRecord;
use riff_catalog_core::{Digest, FacetAddress, Name};

use crate::CLAIMS_SCHEMA_VERSION;
use crate::claim::Witness;
use crate::error::ClaimsError;

/// A unary, witnessed property of one artifact at one facet:
/// `subject attests property P, witnessed by W` — the gating direction of
/// invariant I13. Structural hashing cannot see "verified-total" or
/// "memory-safe"; attestations carry such guarantees, and guarantee-bearing
/// queries exclude whatever lacks them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attestation {
    pub schema_version: u32,
    pub subject: FacetAddress,
    pub property: Name,
    pub witness: Witness,
    /// Display-only; excluded from the id.
    pub note: Option<String>,
}

impl Attestation {
    pub fn new(
        subject: FacetAddress,
        property: impl Into<String>,
        witness: Witness,
        note: Option<String>,
    ) -> Result<Self, ClaimsError> {
        Ok(Self {
            schema_version: CLAIMS_SCHEMA_VERSION,
            subject,
            property: Name::new(property, "attestation property").map_err(ClaimsError::Core)?,
            witness,
            note,
        })
    }

    pub fn attestation_id(&self) -> Digest {
        let mut record = MetaRecord::new("riffcat.attestation");
        record
            .push_u32(self.schema_version)
            .push_digest(&self.subject.facet.facet_id())
            .push_digest(&self.subject.address_digest())
            .push_str(self.property.as_str())
            .push_str(self.witness.kind.as_str())
            .push_u32(self.witness.payload.len() as u32);
        for (key, value) in &self.witness.payload {
            record.push_str(key).push_str(value);
        }
        record.finish()
    }
}

/// Deduplicating set of attestations, keyed by attestation id.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttestationSet {
    attestations: BTreeMap<Digest, Attestation>,
}

impl AttestationSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, attestation: Attestation) -> bool {
        self.attestations
            .insert(attestation.attestation_id(), attestation)
            .is_none()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Digest, &Attestation)> {
        self.attestations.iter()
    }

    pub fn len(&self) -> usize {
        self.attestations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.attestations.is_empty()
    }

    /// Address digests attested with `property` (any witness).
    pub fn subjects_with(&self, property: &str) -> BTreeSet<Digest> {
        self.attestations
            .values()
            .filter(|attestation| attestation.property.as_str() == property)
            .map(|attestation| attestation.subject.address_digest())
            .collect()
    }

    /// The gate: does this address carry the property?
    pub fn permits(&self, property: &str, address: &Digest) -> bool {
        self.subjects_with(property).contains(address)
    }
}
