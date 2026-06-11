use std::collections::{BTreeMap, BTreeSet};

use riff_catalog_core::{Digest, Facet, FacetAddress, FacetIndex, GraphKey};

use crate::claim::ClaimSet;
use crate::error::ClaimsError;
use crate::union_find::UnionFind;

/// Equivalence closure of the claims that target one exact facet.
///
/// Facet scoping is exact-match on `facet_id` (invariant I12): no subset or
/// implication reasoning — a claim at facet F is invisible at facet G even if
/// G's dimensions are a subset of F's.
pub struct FacetClosure {
    facet: Facet,
    union: UnionFind,
    /// claim ids per class root id, merged on the fly.
    claims_by_root: BTreeMap<u32, Vec<Digest>>,
}

impl ClaimSet {
    /// Closure over unconditional claims only: conditional claims (those
    /// carrying an `assumptions` root) never merge unless explicitly
    /// accepted via [`Self::closure_for_facet_assuming`].
    pub fn closure_for_facet(&self, facet: &Facet) -> FacetClosure {
        self.closure_for_facet_assuming(facet, &BTreeSet::new())
    }

    /// Closure that additionally accepts conditional claims whose
    /// `assumptions` root is in `assumed_roots`. Acceptance is exact-match
    /// on the root digest — no reasoning about what the set contains; the
    /// querier is taking responsibility for the named assumption set, and
    /// that acceptance stays attributable through `supporting_claims`.
    pub fn closure_for_facet_assuming(
        &self,
        facet: &Facet,
        assumed_roots: &BTreeSet<Digest>,
    ) -> FacetClosure {
        let facet_id = facet.facet_id();
        let mut union = UnionFind::default();
        let mut claims: Vec<(u32, u32, Digest)> = Vec::new();
        for (claim_id, claim) in self.iter() {
            if claim.facet.facet_id() != facet_id {
                continue;
            }
            if claim
                .assumptions
                .as_ref()
                .is_some_and(|assumptions| !assumed_roots.contains(assumptions))
            {
                continue;
            }
            let left = union.intern(claim.left.address_digest());
            let right = union.intern(claim.right.address_digest());
            claims.push((left, right, *claim_id));
        }
        for &(left, right, _) in &claims {
            union.union(left, right);
        }
        let mut claims_by_root: BTreeMap<u32, Vec<Digest>> = BTreeMap::new();
        for (left, _, claim_id) in claims {
            let root = union.find(left);
            claims_by_root.entry(root).or_default().push(claim_id);
        }
        FacetClosure {
            facet: facet.clone(),
            union,
            claims_by_root,
        }
    }
}

impl FacetClosure {
    pub fn facet(&self) -> &Facet {
        &self.facet
    }

    /// Deterministic representative: the minimum digest in the class.
    /// Unknown addresses represent themselves.
    pub fn representative(&mut self, address: &Digest) -> Digest {
        self.class_members(address)
            .into_iter()
            .next()
            .unwrap_or(*address)
    }

    pub fn same_class(&mut self, a: &Digest, b: &Digest) -> bool {
        if a == b {
            return true;
        }
        match (self.union.lookup(a), self.union.lookup(b)) {
            (Some(ia), Some(ib)) => self.union.find(ia) == self.union.find(ib),
            _ => false,
        }
    }

    /// Every address in the class (sorted). Singleton for unknown addresses.
    pub fn class_members(&mut self, address: &Digest) -> BTreeSet<Digest> {
        let Some(id) = self.union.lookup(address) else {
            return [*address].into();
        };
        let root = self.union.find(id);
        let mut members = BTreeSet::new();
        for other in 0..self.union.len() as u32 {
            if self.union.find(other) == root {
                members.insert(self.union.digest_of(other));
            }
        }
        members
    }

    /// Claim ids supporting the class of `address` (audit trail, I14).
    pub fn supporting_claims(&mut self, address: &Digest) -> Vec<Digest> {
        let Some(id) = self.union.lookup(address) else {
            return Vec::new();
        };
        let root = self.union.find(id);
        self.claims_by_root.get(&root).cloned().unwrap_or_default()
    }
}

/// A facet index extended by a claims closure: structural lookups plus
/// claim-merged classes, reported separately (invariant I14).
pub struct ClaimGatedIndex<'a> {
    pub index: &'a FacetIndex,
    pub closure: &'a mut FacetClosure,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimGatedLookup {
    pub address: Digest,
    pub representative: Digest,
    /// The structural bucket for the queried address itself.
    pub direct: Vec<GraphKey>,
    /// Buckets of other class members reachable only via claims.
    pub via_claims: Vec<(Digest, Vec<GraphKey>)>,
    pub supporting_claims: Vec<Digest>,
}

impl ClaimGatedIndex<'_> {
    pub fn lookup(&mut self, address: &FacetAddress) -> Result<ClaimGatedLookup, ClaimsError> {
        if address.facet != self.index.facet || address.facet != *self.closure.facet() {
            return Err(ClaimsError::LookupFacetMismatch);
        }
        let digest = address.address_digest();
        let direct = self.index.bucket(&digest).to_vec();
        let via_claims = self
            .closure
            .class_members(&digest)
            .into_iter()
            .filter(|member| member != &digest)
            .map(|member| (member, self.index.bucket(&member).to_vec()))
            .filter(|(_, graphs)| !graphs.is_empty())
            .collect();
        Ok(ClaimGatedLookup {
            address: digest,
            representative: self.closure.representative(&digest),
            direct,
            via_claims,
            supporting_claims: self.closure.supporting_claims(&digest),
        })
    }
}
