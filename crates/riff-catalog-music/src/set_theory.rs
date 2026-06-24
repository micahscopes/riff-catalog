//! Pitch-class set theory: prime form and interval vector (Allen Forte / Rahn).
//! This mirrors what micahscopes/polyphonotopes-rs computes, kept here as a
//! small self-contained reference so a test can show that riffcat's note-set
//! facet, when fed prime-form-canonicalized input, coincides with Forte's
//! set-class equivalence: the engine rediscovering the catalog.
//!
//! Prime form is the canonical representative of a set under transposition AND
//! inversion (Tn/TnI). It is decidable finite combinatorics: no hash, no engine.

/// The prime form of a pitch-class set: the canonical representative under
/// transposition AND inversion (Tn/TnI), following Rahn. Take the most compact
/// normal order of the set and of its inversion, transpose each to begin on
/// zero, and keep the more left-packed of the two. Decidable finite
/// combinatorics: no hash, no engine.
///
/// Note this is NOT the lexicographically least rotation over all rotations of
/// the set and its inversion: that shortcut disagrees with the published
/// catalog for sets where the most compact rotation is not the lex-least one.
/// The minor seventh 4-26 {0,4,7,9} is the canonical example: its prime form is
/// [0,3,5,8] (span 8), while the lex-least rotation is the looser [0,2,5,9].
pub fn prime_form(pcs: &[i32]) -> Vec<i32> {
    let mut set: Vec<i32> = pcs.iter().map(|p| p.rem_euclid(12)).collect();
    set.sort_unstable();
    set.dedup();
    if set.is_empty() {
        return vec![];
    }
    let mut inv: Vec<i32> = set.iter().map(|&x| (12 - x).rem_euclid(12)).collect();
    inv.sort_unstable();
    inv.dedup();

    let nf_set = normal_order_zeroed(&set);
    let nf_inv = normal_order_zeroed(&inv);
    // The prime form is the more left-packed of the two normal orders. They
    // always share the same span, so this lexicographic pick is the standard
    // "most packed to the left" rule.
    if nf_inv < nf_set { nf_inv } else { nf_set }
}

/// The most compact normal order of a set, transposed to begin on zero (Rahn):
/// among the rotations, minimize the span (the last element), then break ties by
/// comparing the remaining elements inward from the right, preferring the form
/// packed most tightly to the left.
fn normal_order_zeroed(set: &[i32]) -> Vec<i32> {
    let n = set.len();
    let mut best: Option<Vec<i32>> = None;
    for i in 0..n {
        let base = set[i];
        let cand: Vec<i32> = (0..n)
            .map(|k| (set[(i + k) % n] - base).rem_euclid(12))
            .collect();
        best = Some(match best {
            None => cand,
            Some(b) => {
                if more_compact(&cand, &b) { cand } else { b }
            }
        });
    }
    best.unwrap()
}

/// True if `a` is strictly more compact than `b` (both transposed to start on
/// zero, equal length). Compare the span (last element) first, then inward from
/// the right, preferring the smaller value at the first difference.
fn more_compact(a: &[i32], b: &[i32]) -> bool {
    for k in (1..a.len()).rev() {
        if a[k] != b[k] {
            return a[k] < b[k];
        }
    }
    false
}

/// The interval vector: counts of each interval class (1..=6) among all pairs.
pub fn interval_vector(pcs: &[i32]) -> [u8; 6] {
    let mut set: Vec<i32> = pcs.iter().map(|p| p.rem_euclid(12)).collect();
    set.sort_unstable();
    set.dedup();
    let mut v = [0u8; 6];
    for i in 0..set.len() {
        for j in (i + 1)..set.len() {
            let d = (set[j] - set[i]).rem_euclid(12);
            let ic = d.min(12 - d); // interval class 1..=6
            if (1..=6).contains(&ic) {
                v[(ic - 1) as usize] += 1;
            }
        }
    }
    v
}

/// Transpose a 12-bit pitch-class mask up by k semitones (bit i -> bit (i+k)%12).
fn transpose_mask(mask: u32, k: u32) -> u32 {
    let mut r = 0u32;
    for i in 0..12u32 {
        if mask & (1u32 << i) != 0 {
            r |= 1u32 << ((i + k) % 12);
        }
    }
    r
}

/// Transposition normal form: the canonical transposition representative of a
/// pitch-class set (the minimal-rotation bitmask), as sorted pitch classes. Two
/// transposition-equivalent sets share it. This is the rung between the literal
/// note set and the prime form. It induces the same partition as
/// polyphonotopes-math's normalFormBits (cross-checked against its tonal catalog).
pub fn transposition_normal_form(pcs: &[i32]) -> Vec<i32> {
    let mut mask = 0u32;
    for p in pcs {
        mask |= 1u32 << (p.rem_euclid(12) as u32);
    }
    if mask == 0 {
        return vec![];
    }
    let min = (0..12u32).map(|k| transpose_mask(mask, k)).min().unwrap();
    (0..12i32).filter(|i| min & (1u32 << i) != 0).collect()
}

#[cfg(test)]
mod tests {
    use super::{interval_vector, prime_form, transposition_normal_form};
    use crate::{PITCH_CLASS_SET, encode_pitch_class_set, facet_hex};

    #[test]
    fn prime_forms_match_the_published_catalog() {
        assert_eq!(prime_form(&[0, 4, 7]), vec![0, 3, 7], "major triad = 3-11");
        assert_eq!(prime_form(&[0, 3, 7]), vec![0, 3, 7], "minor triad = 3-11 too");
        assert_eq!(prime_form(&[9, 0, 4]), vec![0, 3, 7], "A minor is also 3-11");
        assert_eq!(prime_form(&[0, 4, 8]), vec![0, 4, 8], "augmented = 3-12");
        assert_eq!(prime_form(&[0, 3, 6]), vec![0, 3, 6], "diminished = 3-10");
        assert_eq!(prime_form(&[0, 4, 7, 10]), vec![0, 2, 5, 8], "dominant 7th = 4-27");
        assert_eq!(prime_form(&[0, 4, 7, 11]), vec![0, 1, 5, 8], "major 7th = 4-20");
        // 4-26, the minor seventh, is the case the lex-least shortcut got wrong:
        // the published prime form is the compact [0,3,5,8], not [0,2,5,9].
        assert_eq!(prime_form(&[0, 4, 7, 9]), vec![0, 3, 5, 8], "minor 7th = 4-26 (compact)");
        // half-diminished and dominant sevenths are the two faces of 4-27, so
        // they fold to the same prime form (the half-diminished side, [0,2,5,8]).
        assert_eq!(prime_form(&[0, 3, 6, 8]), vec![0, 2, 5, 8], "dominant 7th Tn-type folds to 4-27");
        assert_eq!(prime_form(&[0, 2, 5, 8]), vec![0, 2, 5, 8], "half-diminished is the 4-27 prime");
    }

    /// A set and its inversion always share one prime form (the defining
    /// property of TnI canonicalization), including the 4-26 case.
    #[test]
    fn prime_form_is_inversion_invariant() {
        let invert = |pcs: &[i32]| -> Vec<i32> { pcs.iter().map(|x| (12 - x).rem_euclid(12)).collect() };
        for set in [
            vec![0, 4, 7],
            vec![0, 4, 7, 9],
            vec![0, 4, 7, 10],
            vec![0, 1, 4, 6, 9],
        ] {
            assert_eq!(prime_form(&set), prime_form(&invert(&set)), "prime form survives inversion: {set:?}");
        }
    }

    #[test]
    fn interval_vectors_match() {
        assert_eq!(interval_vector(&[0, 4, 7]), [0, 0, 1, 1, 1, 0], "major triad <001110>");
        assert_eq!(interval_vector(&[0, 4, 8]), [0, 0, 0, 3, 0, 0], "augmented <000300>");
    }

    /// riffcat's note-set facet, fed prime-form-canonicalized input, partitions
    /// chords exactly the way Forte's set classes do: major and minor triads
    /// collapse to one class (3-11), augmented and diminished stay distinct.
    #[test]
    fn note_set_facet_coincides_with_forte_set_class() {
        let addr = |pcs: &[i32]| {
            let (k, g) = encode_pitch_class_set("sc", &prime_form(pcs)).unwrap();
            facet_hex(&k, &g, &PITCH_CLASS_SET).unwrap()
        };
        let c_major = addr(&[0, 4, 7]);
        let a_minor = addr(&[9, 0, 4]); // A C E
        let f_major = addr(&[5, 9, 0]); // F A C
        let augmented = addr(&[0, 4, 8]);
        let diminished = addr(&[0, 3, 6]);
        assert_eq!(c_major, a_minor, "major and minor triads are one set class (Forte 3-11)");
        assert_eq!(c_major, f_major, "all major triads share the class");
        assert_ne!(c_major, augmented, "augmented (3-12) is its own class");
        assert_ne!(c_major, diminished, "diminished (3-10) is its own class");
        assert_ne!(augmented, diminished, "3-12 and 3-10 differ");
    }

    #[test]
    fn transposition_normal_form_partitions_like_polyphonotopes_math() {
        // (bits, normalFormBits) straight from polyphonotopes-math/data/tonal-pcs.json.
        // normalFormBits is transposition-invariant, so our minimal-rotation normal
        // form must induce the same partition: same normalFormBits iff same normal form.
        let catalog: &[(u32, u32)] = &[
            (661, 661),
            (2741, 2774),
            (1453, 2774),
            (669, 2382),
            (1257, 2382),
            (2733, 2742),
            (2477, 3436),
            (129, 2112),
            (161, 2128),
            (1041, 2208),
        ];
        let pcs_of = |bits: u32| -> Vec<i32> { (0..12i32).filter(|i| bits & (1u32 << i) != 0).collect() };
        for &(b1, nf1) in catalog {
            for &(b2, nf2) in catalog {
                let same_theirs = nf1 == nf2;
                let same_ours =
                    transposition_normal_form(&pcs_of(b1)) == transposition_normal_form(&pcs_of(b2));
                assert_eq!(same_ours, same_theirs, "bits {b1} vs {b2}");
            }
        }
    }
}
