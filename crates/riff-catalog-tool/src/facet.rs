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

/// The dimensions a facet request means for one row. The shorthands (`all`,
/// and `names-blind`, i.e. every dimension but names) mean "of the dimensions
/// this unit's recipe fills". Any other request must name filled dimensions
/// only: a dimension the recipe never fills is unknown there, not equal.
pub fn dimensions_for_row(row: &DigestRow, dimensions: &BTreeSet<Dimension>) -> Result<BTreeSet<Dimension>> {
    let all: BTreeSet<Dimension> = Dimension::ALL.into_iter().collect();
    let names_blind: BTreeSet<Dimension> = all.iter().copied().filter(|d| *d != Dimension::Names).collect();
    if *dimensions == all || *dimensions == names_blind {
        let filled: BTreeSet<Dimension> =
            dimensions.iter().copied().filter(|d| row.digests.contains_key(d)).collect();
        if filled.is_empty() {
            bail!("row {} has none of the requested dimensions", row.name);
        }
        return Ok(filled);
    }
    if let Some(missing) = dimensions.iter().find(|d| !row.digests.contains_key(d)) {
        bail!(
            "recipe {} does not fill dimension {}; row {} has no digest for it",
            row.level,
            missing.as_str(),
            row.name
        );
    }
    Ok(dimensions.clone())
}

pub fn facet_for_row(row: &DigestRow, dimensions: &BTreeSet<Dimension>) -> Result<Facet> {
    Ok(Facet::new(row.policy_id, dimensions_for_row(row, dimensions)?)?)
}

/// The facet-address digest of one row at a dimension subset.
pub fn address_of(row: &DigestRow, dimensions: &BTreeSet<Dimension>) -> Result<Digest> {
    Ok(facet_address_of(row, dimensions)?.address_digest())
}

pub fn facet_address_of(row: &DigestRow, dimensions: &BTreeSet<Dimension>) -> Result<FacetAddress> {
    let facet = facet_for_row(row, dimensions)?;
    let dimensions = &facet.dimensions.clone();
    // Where the unit's stored digests cannot simply be combined for this
    // facet, ingest stored the joint digest; use it.
    if riff_catalog_core::mask_needs_joint(row.mixed_pairs, dimensions) {
        let key = riff_catalog_core::dimensions_key(dimensions);
        let digest = row.joint.get(&key).ok_or_else(|| {
            anyhow::anyhow!("row {} needs a joint digest for {key} and has none; ingest it again", row.name)
        })?;
        return Ok(FacetAddress::joint(facet, *digest));
    }
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
