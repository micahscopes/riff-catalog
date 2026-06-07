//! In-memory indexes over digest results. Flat entry records on purpose:
//! one entry per JSONL line round-trips through serde with no custom format.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::dimension::Dimension;
use crate::error::CatalogError;
use crate::hash::DigestResult;
use crate::key::GraphKey;
use crate::policy::PolicyId;
use crate::reference::Facet;
use crate::text::Digest;

/// (dimension, digest) → graphs, under one policy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DigestIndex {
    pub policy_id: PolicyId,
    pub entries: Vec<DigestIndexEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DigestIndexEntry {
    pub dimension: Dimension,
    pub digest: Digest,
    pub graphs: Vec<GraphKey>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LookupRequest {
    pub policy_id: PolicyId,
    pub dimension: Dimension,
    pub digest: Digest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LookupResult {
    pub request: LookupRequest,
    pub graphs: Vec<GraphKey>,
}

impl DigestIndex {
    pub fn from_results(
        policy_id: PolicyId,
        results: impl IntoIterator<Item = DigestResult>,
    ) -> Result<Self, CatalogError> {
        let mut by_digest = BTreeMap::<(Dimension, Digest), Vec<GraphKey>>::new();
        for result in results {
            if result.hashes.policy_id != policy_id {
                return Err(CatalogError::IndexPolicyMismatch {
                    expected: policy_id.to_hex(),
                    actual: result.hashes.policy_id.to_hex(),
                });
            }
            for (dimension, digest) in result.hashes.graph.iter() {
                by_digest
                    .entry((*dimension, *digest))
                    .or_default()
                    .push(result.request.graph.clone());
            }
        }
        let entries = by_digest
            .into_iter()
            .map(|((dimension, digest), mut graphs)| {
                graphs.sort_by_key(GraphKey::canonical_key);
                DigestIndexEntry {
                    dimension,
                    digest,
                    graphs,
                }
            })
            .collect();
        Ok(Self { policy_id, entries })
    }

    pub fn lookup(&self, request: LookupRequest) -> Result<LookupResult, CatalogError> {
        if request.policy_id != self.policy_id {
            return Err(CatalogError::IndexPolicyMismatch {
                expected: self.policy_id.to_hex(),
                actual: request.policy_id.to_hex(),
            });
        }
        let graphs = self
            .entries
            .iter()
            .find(|entry| entry.dimension == request.dimension && entry.digest == request.digest)
            .map_or_else(Vec::new, |entry| entry.graphs.clone());
        Ok(LookupResult { request, graphs })
    }
}

/// facet-address digest → graphs, under one facet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FacetIndex {
    pub facet: Facet,
    pub entries: Vec<FacetIndexEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FacetIndexEntry {
    pub address: Digest,
    pub graphs: Vec<GraphKey>,
}

impl FacetIndex {
    pub fn from_results(
        facet: Facet,
        results: impl IntoIterator<Item = DigestResult>,
    ) -> Result<Self, CatalogError> {
        let mut by_address = BTreeMap::<Digest, Vec<GraphKey>>::new();
        for result in results {
            let address = result.hashes.facet_address(&facet)?;
            by_address
                .entry(address.address_digest())
                .or_default()
                .push(result.request.graph.clone());
        }
        let entries = by_address
            .into_iter()
            .map(|(address, mut graphs)| {
                graphs.sort_by_key(GraphKey::canonical_key);
                graphs.dedup();
                FacetIndexEntry { address, graphs }
            })
            .collect();
        Ok(Self { facet, entries })
    }

    pub fn bucket(&self, address: &Digest) -> &[GraphKey] {
        self.entries
            .iter()
            .find(|entry| &entry.address == address)
            .map_or(&[], |entry| entry.graphs.as_slice())
    }

    pub fn buckets(&self) -> impl Iterator<Item = &FacetIndexEntry> {
        self.entries.iter()
    }
}
