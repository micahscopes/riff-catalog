//! The demo's forte-catalog chapter, pinned to the published catalog.
//!
//! The storybook (demo/app/app.js, `forte-catalog`) renders, for each chord it
//! displays, a Forte-style label DERIVED from engine output: the published base
//! name looked up by the engine's prime form (the one baked fact), then
//!   letter = "A" if the Tn-type equals the prime form, "B" otherwise,
//!   no letter if the set is inversionally symmetric
//!            (Tn-type of the set == Tn-type of its inversion).
//! This test walks the exact chords the demo displays (parsed from the same
//! notation strings), replays that derivation in Rust, and asserts every
//! resulting label, prime form, Tn-type, and interval vector against the
//! published values (Forte 1973 / Rahn; A/B letters per the standard
//! convention, e.g. Wikipedia's List of set classes). It also covers the
//! demo's invert (I) operation: the labels the cards show after the toggle.
//!
//! If this test fails, the demo is showing a wrong catalog fact.

use riff_catalog_music::chord::chord_to_pitch_classes;
use riff_catalog_music::set_theory::{interval_vector, prime_form, transposition_normal_form};

/// The demo's FORTE_NAMES map, byte-for-byte: prime-form string -> published
/// base name. Keys must match what the engine emits (asserted below).
const FORTE_NAMES: &[(&str, &str)] = &[
    ("0 3 7", "3-11"),
    ("0 4 8", "3-12"),
    ("0 3 6", "3-10"),
    ("0 1 5 8", "4-20"),
    ("0 3 5 8", "4-26"),
    ("0 2 5 8", "4-27"),
];

fn pcs_string(pcs: &[i32]) -> String {
    pcs.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(" ")
}

fn invert(pcs: &[i32]) -> Vec<i32> {
    let mut inv: Vec<i32> = pcs.iter().map(|&x| (12 - x).rem_euclid(12)).collect();
    inv.sort_unstable();
    inv.dedup();
    inv
}

/// The demo's `forteLabel`, replayed in Rust over the engine's own functions.
fn forte_label(pcs: &[i32]) -> String {
    let pf = pcs_string(&prime_form(pcs));
    let base = FORTE_NAMES
        .iter()
        .find(|(k, _)| *k == pf)
        .map(|(_, v)| *v)
        .unwrap_or_else(|| panic!("prime form {pf:?} has no entry in FORTE_NAMES"));
    let tnf = transposition_normal_form(pcs);
    let sym = tnf == transposition_normal_form(&invert(pcs));
    let letter = if sym {
        ""
    } else if tnf == prime_form(pcs) {
        "A"
    } else {
        "B"
    };
    format!("{base}{letter}")
}

/// Every chord the forte-catalog chapter displays (the demo's FORTE_CHORDS, in
/// its notation), with the published facts the cards must show: the full
/// A/B-lettered id, the prime form, the Tn-type, and the interval vector.
#[test]
fn the_demo_catalog_matches_the_published_catalog() {
    #[rustfmt::skip]
    let expect: &[(&str, &str, &[i32], &[i32], [u8; 6])] = &[
        // notation, id,      prime form,    Tn-type,        interval vector
        ("C",     "3-11B", &[0, 3, 7],    &[0, 4, 7],     [0, 0, 1, 1, 1, 0]),
        ("G",     "3-11B", &[0, 3, 7],    &[0, 4, 7],     [0, 0, 1, 1, 1, 0]),
        ("Am",    "3-11A", &[0, 3, 7],    &[0, 3, 7],     [0, 0, 1, 1, 1, 0]),
        ("Caug",  "3-12",  &[0, 4, 8],    &[0, 4, 8],     [0, 0, 0, 3, 0, 0]),
        ("Bdim",  "3-10",  &[0, 3, 6],    &[0, 3, 6],     [0, 0, 2, 0, 0, 1]),
        ("Cmaj7", "4-20",  &[0, 1, 5, 8], &[0, 1, 5, 8],  [1, 0, 1, 2, 2, 0]),
        ("G7",    "4-27B", &[0, 2, 5, 8], &[0, 3, 6, 8],  [0, 1, 2, 1, 1, 1]),
        ("Am7",   "4-26",  &[0, 3, 5, 8], &[0, 3, 5, 8],  [0, 1, 2, 1, 2, 0]),
    ];
    for (notation, id, pf, tnf, iv) in expect {
        let pcs = chord_to_pitch_classes(notation).expect(notation);
        assert_eq!(&forte_label(&pcs), id, "{notation}: Forte id");
        assert_eq!(&prime_form(&pcs), pf, "{notation}: prime form");
        assert_eq!(&transposition_normal_form(&pcs), tnf, "{notation}: Tn-type");
        assert_eq!(&interval_vector(&pcs), iv, "{notation}: interval vector");
    }
    // The one collapse the page shows at the set-class stop: the two major
    // triads share a Tn-type (transposition folded), while the minor triad,
    // its mirror, does not join them until the prime-form fold.
    let tnf = |c: &str| transposition_normal_form(&chord_to_pitch_classes(c).expect(c));
    assert_eq!(tnf("C"), tnf("G"), "C = G at the Tn-type");
    assert_ne!(tnf("C"), tnf("Am"), "Am stays apart at the Tn-type (the A/B point)");
}

/// The demo's invert (I) toggle: A and B swap, symmetric sets stay themselves.
#[test]
fn the_invert_operation_swaps_a_and_b_and_fixes_the_symmetric_sets() {
    let expect: &[(&str, &str)] = &[
        ("C", "3-11A"),     // major inverts onto the minor type
        ("G", "3-11A"),
        ("Am", "3-11B"),    // and minor onto the major type
        ("Caug", "3-12"),   // the symmetric four invert to themselves
        ("Bdim", "3-10"),
        ("Cmaj7", "4-20"),
        ("Am7", "4-26"),
        ("G7", "4-27A"),    // dominant seventh inverts onto the half-diminished type
    ];
    for (notation, id) in expect {
        let pcs = chord_to_pitch_classes(notation).expect(notation);
        assert_eq!(&forte_label(&invert(&pcs)), id, "inv({notation}): Forte id");
        // The defining property of the fold: prime form survives inversion.
        assert_eq!(
            prime_form(&pcs),
            prime_form(&invert(&pcs)),
            "inv({notation}): same prime form"
        );
    }
}

/// The FORTE_NAMES keys are exactly the prime-form strings the engine emits for
/// the displayed chords: no unused entry, no chord whose card would silently
/// lose its Forte number to a key mismatch.
#[test]
fn forte_names_keys_are_exactly_the_engine_prime_forms_of_the_demo_chords() {
    let demo_chords = ["C", "G", "Am", "Caug", "Bdim", "Cmaj7", "G7", "Am7"];
    let mut emitted: Vec<String> = demo_chords
        .iter()
        .map(|c| pcs_string(&prime_form(&chord_to_pitch_classes(c).expect(c))))
        .collect();
    emitted.sort();
    emitted.dedup();
    let mut keys: Vec<String> = FORTE_NAMES.iter().map(|(k, _)| (*k).to_string()).collect();
    keys.sort();
    assert_eq!(emitted, keys, "FORTE_NAMES must key exactly the displayed prime forms");
}

/// The chord-fp chapter (the-riff, chapter 2): "Cdo" is the solfege spelling of
/// the C major triad, and the chords the prose says collapse at set class
/// really do (C = Cdo = Am = F at the prime form; Caug and Bdim stand alone).
#[test]
fn chord_fp_chapter_grouping_matches_its_prose() {
    let pf = |c: &str| prime_form(&chord_to_pitch_classes(c).expect(c));
    assert_eq!(chord_to_pitch_classes("Cdo").unwrap(), vec![0, 4, 7], "Cdo is C major");
    assert_eq!(pf("C"), vec![0, 3, 7]);
    assert_eq!(pf("Cdo"), pf("C"), "two spellings, one class");
    assert_eq!(pf("Am"), pf("C"));
    assert_eq!(pf("F"), pf("C"));
    assert_ne!(pf("Caug"), pf("C"));
    assert_ne!(pf("Bdim"), pf("C"));
    assert_ne!(pf("Caug"), pf("Bdim"));
    // The badge pair the chapter renders: Tn-type and the prime form it folds to.
    let c = chord_to_pitch_classes("C").unwrap();
    assert_eq!(transposition_normal_form(&c), vec![0, 4, 7], "C: Tn [0 4 7]");
    assert_eq!(prime_form(&c), vec![0, 3, 7], "C: prime [0 3 7]");
}
