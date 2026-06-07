use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ClaimsError {
    #[error("claim subjects must share the claim's facet")]
    FacetMismatch,
    #[error("lookup facet does not match the index/closure facet")]
    LookupFacetMismatch,
    #[error(transparent)]
    Core(#[from] riff_catalog_core::CatalogError),
}
