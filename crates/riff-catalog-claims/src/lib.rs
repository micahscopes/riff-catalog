//! riff-catalog-claims: witnessed assertions over facet addresses.
//!
//! Two shapes, two directions (invariant I13):
//! - [`Claim`] (binary, `A ≅ B at facet F, witnessed by W`) only ever
//!   *coarsens*: the [`FacetClosure`] union-find merges address classes.
//! - [`Attestation`] (unary, `A attests property P, witnessed by W`) only
//!   ever *filters*: guarantee-bearing queries exclude unattested artifacts.
//!
//! The core contract (invariant I11): **claims are inputs, not discoveries.**
//! Nothing here inspects witness payloads — a wrong witness merges, and the
//! merge is attributable via [`FacetClosure::supporting_claims`]. Validity
//! belongs to whoever audits the witness; attributability belongs to us.

mod attest;
mod claim;
mod closure;
mod error;
mod union_find;

pub use attest::{Attestation, AttestationSet};
pub use claim::{Claim, ClaimSet, Relation, Witness};
pub use closure::{ClaimGatedIndex, ClaimGatedLookup, FacetClosure};
pub use error::ClaimsError;

pub const CLAIMS_SCHEMA_VERSION: u32 = 1;
