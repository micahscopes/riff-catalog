//! Order-independent merkle roots over digest sets (invariant I17): the
//! commitment a corpus can publish and a conditional claim can reference.
//! This is the whole zk-socket — the schema accommodation that lets an
//! aggregate claim ("this shape is in corpus R") be verified against a
//! root someday, with zero prover code here, ever.
//!
//! Construction (canonical; conformance-relevant, like everything hashed):
//! 1. Leaves form a *set*: duplicates collapse and insertion order is
//!    forgotten. A root commits to what is in the corpus, not to how many
//!    times or in which order it was ingested.
//! 2. Sort ascending by raw digest bytes.
//! 3. Wrap each leaf as `meta("riffcat.root.leaf", digest)` so a leaf can
//!    never be confused with an interior node (second-preimage discipline).
//! 4. Reduce pairwise, left to right: `meta("riffcat.root.node", left,
//!    right)`. An unpaired trailing node is promoted to the next level
//!    unchanged — never duplicated (duplication would make `{a}` and
//!    `{a, a}` distinguishable from a proof's point of view while the set
//!    semantics say they are the same).
//! 5. The empty set has the distinguished root `meta("riffcat.root.empty")`.

use std::collections::BTreeSet;

use crate::encode;
use crate::text::Digest;

/// The canonical order-independent merkle root of a set of digests.
pub fn set_root(leaves: impl IntoIterator<Item = Digest>) -> Digest {
    let leaves: BTreeSet<Digest> = leaves.into_iter().collect();
    if leaves.is_empty() {
        return encode::digest_meta("riffcat.root.empty", |_| {});
    }
    let mut level: Vec<Digest> = leaves
        .into_iter()
        .map(|leaf| {
            encode::digest_meta("riffcat.root.leaf", |bytes| {
                encode::push_digest(bytes, &leaf);
            })
        })
        .collect();
    while level.len() > 1 {
        level = level
            .chunks(2)
            .map(|pair| match pair {
                [left, right] => encode::digest_meta("riffcat.root.node", |bytes| {
                    encode::push_digest(bytes, left);
                    encode::push_digest(bytes, right);
                }),
                [promoted] => *promoted,
                _ => unreachable!("chunks(2) yields 1- or 2-element slices"),
            })
            .collect();
    }
    level[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(byte: u8) -> Digest {
        Digest::from_bytes([byte; 32])
    }

    #[test]
    fn root_is_order_independent() {
        let forward = set_root([d(1), d(2), d(3), d(4), d(5)]);
        let shuffled = set_root([d(4), d(1), d(5), d(3), d(2)]);
        assert_eq!(forward, shuffled);
    }

    #[test]
    fn duplicates_collapse() {
        assert_eq!(set_root([d(7), d(7), d(8)]), set_root([d(8), d(7)]));
        assert_eq!(set_root([d(7), d(7)]), set_root([d(7)]));
    }

    #[test]
    fn distinct_sets_get_distinct_roots() {
        assert_ne!(set_root([]), set_root([d(0)]));
        assert_ne!(set_root([d(1)]), set_root([d(2)]));
        assert_ne!(set_root([d(1), d(2)]), set_root([d(1), d(3)]));
        // Subset vs superset.
        assert_ne!(set_root([d(1), d(2)]), set_root([d(1), d(2), d(3)]));
    }

    #[test]
    fn leaf_digest_is_not_its_own_root() {
        // The leaf wrap means a raw digest never equals the singleton root
        // built over it — roots and member digests live in separate domains.
        assert_ne!(set_root([d(9)]), d(9));
    }

    #[test]
    fn odd_count_promotion_is_well_defined() {
        // Regression shape: 3 leaves = node(node(l0, l1), promoted l2).
        // Recomputing must be bit-stable, and dropping the promoted member
        // must change the root.
        let three = set_root([d(1), d(2), d(3)]);
        assert_eq!(three, set_root([d(3), d(2), d(1)]));
        assert_ne!(three, set_root([d(1), d(2)]));
    }
}
