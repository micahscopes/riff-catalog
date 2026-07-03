use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::dimension::Dimension;
use crate::key::GraphKey;
use crate::text::Digest;

/// One lowered unit's fingerprint as it travels on the wire.
///
/// The shared record a producer (a compiler such as fe) and the catalog agree
/// on: the graph it names, the policy it was hashed under, the schema version
/// that fixes the encoding, the per-dimension digests, and how many nodes
/// folded in. It carries no hashing, only the result: `policy_id` and the
/// digests are opaque here (the engine computes them), and this crate guarantees
/// only the shape and the encoding so two producers emit the same bytes.
///
/// This is the same content as the catalog's on-disk `Record::Digest` and fe's
/// `ShapeGraphHashFact` plus owner metadata; freezing it here gives both sides
/// one form to serialize.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FingerprintRecord {
    /// The lowered unit this fingerprint addresses.
    pub graph_key: GraphKey,
    /// The policy id the digests were computed under (a bare digest never
    /// travels without it: I1/I7).
    pub policy_id: Digest,
    /// The schema version that fixes the encoding these digests were produced
    /// with. A consumer must reject a record whose version it does not know.
    pub schema_version: u32,
    /// The per-dimension digests. Absent dimensions were not requested; a
    /// present-but-empty dimension is a different, versioned thing (see
    /// `Dimension::TraceEvents`).
    pub digests: BTreeMap<Dimension, Digest>,
    /// How many nodes folded into the graph digest, for weighting and sanity.
    pub node_count: u64,
}

impl FingerprintRecord {
    pub fn new(
        graph_key: GraphKey,
        policy_id: Digest,
        schema_version: u32,
        digests: BTreeMap<Dimension, Digest>,
        node_count: u64,
    ) -> Self {
        Self {
            graph_key,
            policy_id,
            schema_version,
            digests,
            node_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::EntityKey;

    fn record() -> FingerprintRecord {
        let graph_key = GraphKey::new(
            EntityKey::new("yul.object", "pkg:token", "Token").unwrap(),
            "runtime",
        )
        .unwrap();
        let digests = BTreeMap::from([
            (Dimension::Structure, Digest::from_bytes([0x11; 32])),
            (Dimension::Names, Digest::from_bytes([0x22; 32])),
        ]);
        FingerprintRecord::new(graph_key, Digest::from_bytes([0xaa; 32]), 2, digests, 7)
    }

    #[test]
    fn serde_round_trip() {
        let r = record();
        let json = serde_json::to_string(&r).unwrap();
        let back: FingerprintRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn absent_dimension_differs_from_present() {
        let mut a = record();
        let b = record();
        a.digests.remove(&Dimension::Names);
        assert_ne!(a, b, "dropping a dimension must change the record");
    }
}
