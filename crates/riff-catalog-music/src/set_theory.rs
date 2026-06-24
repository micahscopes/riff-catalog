//! Pitch-class set theory: prime form and interval vector (Allen Forte / Rahn).
//! This mirrors what micahscopes/polyphonotopes-rs computes, kept here as a
//! small self-contained reference so a test can show that riffcat's note-set
//! facet, when fed prime-form-canonicalized input, coincides with Forte's
//! set-class equivalence: the engine rediscovering the catalog.
//!
//! Prime form is the canonical representative of a set under transposition AND
//! inversion (Tn/TnI). It is decidable finite combinatorics: no hash, no engine.

/// The prime form of a pitch-class set: the lexicographically least
/// transposed-to-zero rotation of the set or its inversion (Rahn). Decidable.
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

    let mut best: Option<Vec<i32>> = None;
    for form in [&set, &inv] {
        let n = form.len();
        for i in 0..n {
            let base = form[i];
            let cand: Vec<i32> = (0..n)
                .map(|k| (form[(i + k) % n] - base).rem_euclid(12))
                .collect();
            best = Some(match best {
                None => cand,
                Some(b) if cand < b => cand,
                Some(b) => b,
            });
        }
    }
    best.unwrap()
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

#[cfg(test)]
mod tests {
    use super::{interval_vector, prime_form};
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
}
