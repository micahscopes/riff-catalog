//! Encode music as riff-catalog graphs.
//!
//! The engine is domain-general: a graph of nodes carrying dimension-tagged
//! fields, folded into per-facet addresses. Music is just a second domain. The
//! only real work is the ENCODING (which musical aspect lands in which
//! dimension); after that the facets are dimension subsets and "same at this
//! facet" falls straight out of the existing machinery.
//!
//! Encoding for a melodic riff:
//!   - interval from the previous note -> Structure   (so the names-blind /
//!     harmonic-relationships facet is transposition invariant)
//!   - absolute pitch                  -> Names        (the note's "name")
//!   - duration                        -> Constants    (rhythm)
//!   - note order                      -> child ordinals (the Merkle skeleton)
//!
//! A pitch-class set is unordered, so it is a NORMALIZATION, not a projection:
//! canonicalize (mod 12, dedup, sort), encode the canonical sequence with pitch
//! class -> Names, and the Names facet over that graph is the set's identity.

pub mod chord;
pub mod set_theory;

use riff_catalog_core::{
    CatalogError, CyclePolicy, Dimension, DigestRequest, EntityKey, Facet, Graph, GraphKey,
    HashPolicy, NodeKey, ViewMode, digest_graph,
};

/// One note: MIDI pitch (60 = middle C) and a duration in arbitrary ticks.
#[derive(Clone, Copy, Debug)]
pub struct Note {
    pub pitch: i32,
    pub dur: u32,
}

/// The lowering level: two producers at this level must encode identically.
pub const LEVEL: &str = "riff.v1";

/// The three demo facets, each a dimension subset.
pub const HARMONIC_RELATIONSHIPS: [Dimension; 1] = [Dimension::Structure]; // intervals
pub const RHYTHM: [Dimension; 1] = [Dimension::Constants]; // durations
pub const PITCH_CLASS_SET: [Dimension; 1] = [Dimension::Names]; // on the normalized set graph

fn owner(name: &str) -> Result<EntityKey, CatalogError> {
    EntityKey::new("music", "demo", name)
}

/// Encode a melodic riff. interval -> Structure, pitch -> Names, dur -> Constants.
pub fn encode_riff(name: &str, notes: &[Note]) -> Result<(GraphKey, Graph), CatalogError> {
    let owner = owner(name)?;
    let key = GraphKey::new(owner.clone(), "riff")?;
    let mut g = Graph::new(key.clone());
    let root = NodeKey::entity(owner.clone());
    g.add_node(root.clone(), "riff")?;
    let mut prev: Option<i32> = None;
    for (i, note) in notes.iter().enumerate() {
        let n = NodeKey::derived(owner.clone(), format!("n{i}"))?;
        g.add_node(n.clone(), "note")?;
        let interval = match prev {
            Some(p) => i64::from(note.pitch - p),
            None => 0,
        };
        g.add_field(&n, Dimension::Structure, "interval", interval)?;
        g.add_field(&n, Dimension::Names, "pitch", i64::from(note.pitch))?;
        g.add_field(&n, Dimension::Constants, "dur", u64::from(note.dur))?;
        g.add_child(&root, "note", i as u32, &n)?;
        prev = Some(note.pitch);
    }
    g.validate()?;
    Ok((key, g))
}

/// Encode a pitch-class set: normalize (mod 12, dedup, sort), then encode the
/// canonical sequence with pitch class -> Names.
pub fn encode_pitch_class_set(
    name: &str,
    pitches: &[i32],
) -> Result<(GraphKey, Graph), CatalogError> {
    let mut pcs: Vec<u64> = pitches.iter().map(|p| p.rem_euclid(12) as u64).collect();
    pcs.sort_unstable();
    pcs.dedup();
    let owner = owner(name)?;
    let key = GraphKey::new(owner.clone(), "pcset")?;
    let mut g = Graph::new(key.clone());
    let root = NodeKey::entity(owner.clone());
    g.add_node(root.clone(), "pcset")?;
    for (i, pc) in pcs.iter().enumerate() {
        let n = NodeKey::derived(owner.clone(), format!("pc{i}"))?;
        g.add_node(n.clone(), "pc")?;
        g.add_field(&n, Dimension::Names, "pc", *pc)?;
        g.add_child(&root, "pc", i as u32, &n)?;
    }
    g.validate()?;
    Ok((key, g))
}

/// The "equal at this facet" address, as hex, computed in the anonymous-shape
/// view (so the riff's own name never enters the digest).
pub fn facet_hex(key: &GraphKey, graph: &Graph, dims: &[Dimension]) -> Result<String, CatalogError> {
    let policy = HashPolicy::new(LEVEL, ViewMode::AnonymousShape, CyclePolicy::CondenseScc)?;
    let req = DigestRequest::all_dimensions(key.clone(), policy);
    let hashes = digest_graph(&req, graph)?.hashes;
    let facet = Facet::new(hashes.policy_id, dims.iter().copied())?;
    Ok(hashes.facet_address(&facet)?.address_digest().to_hex())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn riff(spec: &[(i32, u32)]) -> Vec<Note> {
        spec.iter()
            .map(|&(pitch, dur)| Note { pitch, dur })
            .collect()
    }

    fn addr(name: &str, notes: &[Note], dims: &[Dimension]) -> String {
        let (k, g) = encode_riff(name, notes).unwrap();
        facet_hex(&k, &g, dims).unwrap()
    }

    // A short motif, and the same motif moved up a perfect fifth (all pitches +7).
    const MOTIF: [(i32, u32); 5] = [(60, 2), (62, 1), (64, 1), (67, 2), (64, 2)];

    fn transposed(k: i32) -> Vec<Note> {
        MOTIF
            .iter()
            .map(|&(p, d)| Note { pitch: p + k, dur: d })
            .collect()
    }

    #[test]
    fn transposition_matches_at_harmonic_relationships_but_not_full() {
        let a = riff(&MOTIF);
        let b = transposed(7);
        // same intervals -> same harmonic-relationships shape
        assert_eq!(
            addr("a", &a, &HARMONIC_RELATIONSHIPS),
            addr("b", &b, &HARMONIC_RELATIONSHIPS),
            "a transposed riff should share the harmonic-relationships shape"
        );
        // different absolute pitches -> still distinguishable at the full facet
        let full = Dimension::ALL.to_vec();
        assert_ne!(
            addr("a", &a, &full),
            addr("b", &b, &full),
            "the full facet should still tell them apart (the pitches differ)"
        );
    }

    #[test]
    fn same_rhythm_different_pitches_matches_at_rhythm() {
        let a = riff(&MOTIF);
        // same duration sequence as MOTIF, deliberately different pitches/intervals
        let c = riff(&[(72, 2), (60, 1), (65, 1), (61, 2), (70, 2)]);
        assert_eq!(
            addr("a", &a, &RHYTHM),
            addr("c", &c, &RHYTHM),
            "same duration sequence should share the rhythm facet"
        );
        assert_ne!(
            addr("a", &a, &HARMONIC_RELATIONSHIPS),
            addr("c", &c, &HARMONIC_RELATIONSHIPS),
            "different intervals should differ at harmonic relationships"
        );
    }

    #[test]
    fn same_pitch_class_set_regardless_of_order_octave_rhythm() {
        // C E G, then the same three classes reordered and re-octaved (G, E+oct, C-oct, E)
        let (k1, g1) = encode_pitch_class_set("triad-1", &[60, 64, 67]).unwrap();
        let (k2, g2) = encode_pitch_class_set("triad-2", &[67, 76, 48, 64]).unwrap();
        assert_eq!(
            facet_hex(&k1, &g1, &PITCH_CLASS_SET).unwrap(),
            facet_hex(&k2, &g2, &PITCH_CLASS_SET).unwrap(),
            "same pitch classes (mod 12) should share the pitch-class-set facet"
        );
        // C E G# (augmented) is a different set
        let (k3, g3) = encode_pitch_class_set("triad-3", &[60, 64, 68]).unwrap();
        assert_ne!(
            facet_hex(&k1, &g1, &PITCH_CLASS_SET).unwrap(),
            facet_hex(&k3, &g3, &PITCH_CLASS_SET).unwrap(),
            "a different pitch-class set should differ"
        );
    }

    #[test]
    fn storybook_riffs_group_as_the_chapter_claims() {
        // The five riffs in the storybook's riff-dial. Each facet must collapse a
        // different set, or the chapter would be lying about what the dial does.
        use std::collections::BTreeSet;
        // The opening of Schubert's An die Musik and its variants, as in the
        // storybook's riff-dial. Keep these in sync with RIFFS in app.js.
        let riffs: [(&str, &[(i32, u32)]); 5] = [
            ("An die Musik", &[(69, 2), (69, 1), (71, 1), (69, 2), (66, 1), (64, 1), (66, 2), (62, 2)]),
            ("up a fifth", &[(76, 2), (76, 1), (78, 1), (76, 2), (73, 1), (71, 1), (73, 2), (69, 2)]),
            ("same notes, re-voiced", &[(62, 1), (78, 1), (66, 1), (81, 1), (64, 1), (71, 2)]),
            ("same rhythm, new notes", &[(72, 2), (67, 1), (71, 1), (67, 2), (65, 1), (69, 1), (67, 2), (72, 2)]),
            ("a different riff", &[(60, 1), (60, 1), (67, 1), (67, 1), (69, 2)]),
        ];
        let shapes = |dims: &[Dimension], pcs: bool| {
            riffs
                .iter()
                .map(|(name, spec)| {
                    if pcs {
                        let pitches: Vec<i32> = spec.iter().map(|&(p, _)| p).collect();
                        let (k, g) = encode_pitch_class_set(name, &pitches).unwrap();
                        facet_hex(&k, &g, dims).unwrap()
                    } else {
                        let (k, g) = encode_riff(name, &riff(spec)).unwrap();
                        facet_hex(&k, &g, dims).unwrap()
                    }
                })
                .collect::<BTreeSet<_>>()
                .len()
        };
        let full = Dimension::ALL.to_vec();
        assert_eq!(shapes(&full, false), 5, "full: all five are distinct");
        assert_eq!(shapes(&HARMONIC_RELATIONSHIPS, false), 4, "intervals: An die Musik = up a fifth");
        assert_eq!(shapes(&RHYTHM, false), 3, "rhythm: An die Musik = up a fifth = same rhythm");
        assert_eq!(shapes(&PITCH_CLASS_SET, true), 4, "note set: An die Musik = re-voiced");
    }
}
