use std::collections::BTreeSet;

use crate::dimension::Dimension;
use crate::encode;
use crate::error::CatalogError;
use crate::graph::Node;
use crate::hash::DimensionDigests;
use crate::policy::{HashPolicy, ViewMode};

/// Per-dimension digest of a node's own content: key (identity mode only),
/// kind (Structure only), and this dimension's fields sorted by (name, value).
pub(crate) fn local_node_digests(
    policy: &HashPolicy,
    node: &Node,
    dimensions: &BTreeSet<Dimension>,
) -> Result<DimensionDigests, CatalogError> {
    let mut digests = DimensionDigests::default();
    for dimension in dimensions {
        let digest = encode::digest_record(policy, *dimension, "node.local", |bytes| {
            if policy.view_mode == ViewMode::IdentityBound {
                encode::push_node_key(bytes, &node.key);
            }
            if *dimension == Dimension::Structure {
                encode::push_str(bytes, node.kind.as_str());
            }
            let mut fields = node
                .fields
                .iter()
                .filter(|field| field.dimension == *dimension)
                .collect::<Vec<_>>();
            fields.sort_by(|left, right| {
                (left.name.as_str(), &left.value).cmp(&(right.name.as_str(), &right.value))
            });
            encode::push_u32(bytes, fields.len() as u32);
            for field in fields {
                encode::push_str(bytes, field.name.as_str());
                encode::push_value(bytes, &field.value);
            }
        })?;
        digests.insert(*dimension, digest);
    }
    Ok(digests)
}
