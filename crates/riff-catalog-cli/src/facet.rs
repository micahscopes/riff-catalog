//! Facet specs and facet-address computation over digest rows. The corpus
//! stores exactly two hash runs per unit (identity + shape, all dimensions);
//! query-time facets are dimension subsets compared as tuples — facet
//! exploration costs nothing extra (invariant I9).

use std::collections::BTreeSet;

use anyhow::{Result, bail};
use riff_catalog_core::{Digest, Dimension, Facet, FacetAddress};

use crate::corpus::DigestRow;

pub fn parse_facet_dimensions(spec: &str) -> Result<BTreeSet<Dimension>> {
    match spec {
        "all" | "full" => Ok(Dimension::ALL.into_iter().collect()),
        "names-blind" => Ok(Dimension::ALL
            .into_iter()
            .filter(|dimension| *dimension != Dimension::Names)
            .collect()),
        other => {
            let mut dimensions = BTreeSet::new();
            for part in other.split('+') {
                match Dimension::parse(part.trim()) {
                    Some(dimension) => {
                        dimensions.insert(dimension);
                    }
                    None => bail!(
                        "unknown dimension `{part}` (expected structure, names, constants, \
                         types, trace_events, or the shorthands all / names-blind)"
                    ),
                }
            }
            if dimensions.is_empty() {
                bail!("facet must name at least one dimension");
            }
            Ok(dimensions)
        }
    }
}

pub fn facet_for_row(row: &DigestRow, dimensions: &BTreeSet<Dimension>) -> Result<Facet> {
    Ok(Facet::new(row.policy_id, dimensions.iter().copied())?)
}

/// The facet-address digest of one row at a dimension subset.
pub fn address_of(row: &DigestRow, dimensions: &BTreeSet<Dimension>) -> Result<Digest> {
    Ok(facet_address_of(row, dimensions)?.address_digest())
}

pub fn facet_address_of(row: &DigestRow, dimensions: &BTreeSet<Dimension>) -> Result<FacetAddress> {
    let facet = facet_for_row(row, dimensions)?;
    let digests = dimensions
        .iter()
        .map(|dimension| {
            row.digests
                .get(dimension)
                .copied()
                .map(|digest| (*dimension, digest))
                .ok_or_else(|| anyhow::anyhow!("row {} missing dimension {dimension:?}", row.name))
        })
        .collect::<Result<_>>()?;
    Ok(FacetAddress::new(facet, digests)?)
}
