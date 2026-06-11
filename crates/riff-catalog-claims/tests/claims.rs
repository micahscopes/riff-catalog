//! M5b/M6 gates: claim identity, closure semantics, facet scoping, the
//! wrong-witness contract, conditional claims, and attestation gating.

use std::collections::BTreeSet;

use riff_catalog_claims::*;
use riff_catalog_core::*;

fn digest(byte: u8) -> Digest {
    Digest::from_hex(&format!("{:02x}", byte).repeat(32)).unwrap()
}

fn facet() -> Facet {
    Facet::structure_only(digest(0xee))
}

fn address(facet: &Facet, byte: u8) -> FacetAddress {
    FacetAddress::new(facet.clone(), [(Dimension::Structure, digest(byte))].into()).unwrap()
}

fn witness() -> Witness {
    Witness::new(
        "constructor-correspondence",
        [
            ("0".to_string(), "re".to_string()),
            ("1".to_string(), "im".to_string()),
        ],
    )
    .unwrap()
}

#[test]
fn claim_requires_consistent_facets() {
    let facet_a = facet();
    let facet_b = Facet::names_blind(digest(0xee));
    let left = address(&facet_a, 1);
    let right = FacetAddress::new(
        facet_b.clone(),
        Dimension::ALL
            .into_iter()
            .filter(|d| *d != Dimension::Names)
            .map(|d| (d, digest(2)))
            .collect(),
    )
    .unwrap();
    assert!(matches!(
        Claim::new(facet_a, left, right, witness(), None, None),
        Err(ClaimsError::FacetMismatch)
    ));
}

#[test]
fn claim_id_stable_and_note_excluded() {
    let facet = facet();
    let make = |note: Option<&str>| {
        Claim::new(
            facet.clone(),
            address(&facet, 1),
            address(&facet, 2),
            witness(),
            note.map(String::from),
            None,
        )
        .unwrap()
    };
    assert_eq!(make(None).claim_id(), make(Some("a note")).claim_id());

    // ...but the witness payload ORDER is identity: scrambling it is a
    // different claim.
    let scrambled = Claim::new(
        facet.clone(),
        address(&facet, 1),
        address(&facet, 2),
        Witness::new(
            "constructor-correspondence",
            [
                ("1".to_string(), "im".to_string()),
                ("0".to_string(), "re".to_string()),
            ],
        )
        .unwrap(),
        None,
        None,
    )
    .unwrap();
    assert_ne!(make(None).claim_id(), scrambled.claim_id());
}

#[test]
fn claim_set_dedups_by_id() {
    let facet = facet();
    let claim = Claim::new(
        facet.clone(),
        address(&facet, 1),
        address(&facet, 2),
        witness(),
        None,
        None,
    )
    .unwrap();
    let mut set = ClaimSet::new();
    assert!(set.insert(claim.clone()));
    assert!(!set.insert(claim));
    assert_eq!(set.len(), 1);
}

#[test]
fn closure_is_transitive_with_deterministic_representative() {
    let facet = facet();
    let mut set = ClaimSet::new();
    for (l, r) in [(1u8, 2u8), (2, 3)] {
        set.insert(
            Claim::new(
                facet.clone(),
                address(&facet, l),
                address(&facet, r),
                witness(),
                None,
                None,
            )
            .unwrap(),
        );
    }
    let mut closure = set.closure_for_facet(&facet);
    let a1 = address(&facet, 1).address_digest();
    let a3 = address(&facet, 3).address_digest();
    assert!(closure.same_class(&a1, &a3));
    assert_eq!(closure.class_members(&a1).len(), 3);
    // representative = min digest in class, same from any member
    assert_eq!(closure.representative(&a1), closure.representative(&a3));
    // both claims support the class
    assert_eq!(closure.supporting_claims(&a3).len(), 2);
}

/// I12: a claim at facet F is invisible to the closure at facet G.
#[test]
fn closure_scopes_to_exact_facet() {
    let facet_full = Facet::full(digest(0xee));
    let mut set = ClaimSet::new();
    set.insert(
        Claim::new(
            facet_full.clone(),
            FacetAddress::new(
                facet_full.clone(),
                Dimension::ALL.into_iter().map(|d| (d, digest(1))).collect(),
            )
            .unwrap(),
            FacetAddress::new(
                facet_full.clone(),
                Dimension::ALL.into_iter().map(|d| (d, digest(2))).collect(),
            )
            .unwrap(),
            witness(),
            None,
            None,
        )
        .unwrap(),
    );
    let other = facet();
    let mut closure = set.closure_for_facet(&other);
    let a = address(&other, 1).address_digest();
    let b = address(&other, 2).address_digest();
    assert!(!closure.same_class(&a, &b));
}

/// I11: the closure never validates witnesses — a wrong witness merges, and
/// the merge is attributable.
#[test]
fn wrong_witness_still_merges_attributably() {
    let facet = facet();
    let wrong = Witness::new(
        "constructor-correspondence",
        // im,re instead of re,im: a wrong-but-valid-looking witness
        [
            ("0".to_string(), "im".to_string()),
            ("1".to_string(), "re".to_string()),
        ],
    )
    .unwrap();
    let claim = Claim::new(
        facet.clone(),
        address(&facet, 1),
        address(&facet, 2),
        wrong,
        None,
        None,
    )
    .unwrap();
    let claim_id = claim.claim_id();
    let mut set = ClaimSet::new();
    set.insert(claim);

    let mut closure = set.closure_for_facet(&facet);
    let a = address(&facet, 1).address_digest();
    let b = address(&facet, 2).address_digest();
    assert!(
        closure.same_class(&a, &b),
        "merge happens regardless of witness content"
    );
    assert_eq!(
        closure.supporting_claims(&a),
        vec![claim_id],
        "and is attributable"
    );
}

#[test]
fn claim_set_serde_round_trip() {
    let facet = facet();
    let mut set = ClaimSet::new();
    set.insert(
        Claim::new(
            facet.clone(),
            address(&facet, 1),
            address(&facet, 2),
            witness(),
            Some("demo".into()),
            None,
        )
        .unwrap(),
    );
    let json = serde_json::to_string(&set).unwrap();
    let back: ClaimSet = serde_json::from_str(&json).unwrap();
    assert_eq!(back, set);
}

/// I18: a conditional claim is a different claim — and it never merges
/// unless its assumption root is explicitly accepted.
#[test]
fn conditional_claim_is_distinct_and_gated() {
    let facet = facet();
    let root = set_root([digest(0x40), digest(0x41)]);
    let make = |assumptions: Option<Digest>| {
        Claim::new(
            facet.clone(),
            address(&facet, 1),
            address(&facet, 2),
            witness(),
            None,
            assumptions,
        )
        .unwrap()
    };

    // Identity: conditional ≠ unconditional, and the root is identity.
    assert_ne!(make(None).claim_id(), make(Some(root)).claim_id());
    assert_ne!(
        make(Some(root)).claim_id(),
        make(Some(set_root([digest(0x42)]))).claim_id()
    );

    let mut set = ClaimSet::new();
    set.insert(make(Some(root)));
    let a = address(&facet, 1).address_digest();
    let b = address(&facet, 2).address_digest();

    // Default closure: the conditional claim is invisible.
    let mut unassumed = set.closure_for_facet(&facet);
    assert!(!unassumed.same_class(&a, &b));

    // Wrong root accepted: still invisible.
    let wrong: BTreeSet<Digest> = [set_root([digest(0x42)])].into();
    let mut wrong_assumed = set.closure_for_facet_assuming(&facet, &wrong);
    assert!(!wrong_assumed.same_class(&a, &b));

    // The claim's own root accepted: merges, attributably.
    let right: BTreeSet<Digest> = [root].into();
    let mut assumed = set.closure_for_facet_assuming(&facet, &right);
    assert!(assumed.same_class(&a, &b));
    assert_eq!(assumed.supporting_claims(&a).len(), 1);
}

/// Schema v1 records (no `assumptions` field) still load, as unconditional.
#[test]
fn v1_claim_json_loads_as_unconditional() {
    let facet = facet();
    let modern = Claim::new(
        facet.clone(),
        address(&facet, 1),
        address(&facet, 2),
        witness(),
        None,
        None,
    )
    .unwrap();
    // A v1 line never serialized an `assumptions` key; strip it if present
    // and pin the version to 1 to reconstruct one.
    let mut value: serde_json::Value = serde_json::to_value(&modern).unwrap();
    value.as_object_mut().unwrap().remove("assumptions");
    value["schema_version"] = 1.into();
    let v1: Claim = serde_json::from_value(value).unwrap();
    assert_eq!(v1.assumptions, None);
    assert_eq!(v1.schema_version, 1);

    // And an unconditional claim's serialization carries no `assumptions`
    // key at all — v1 readers round-trip v2-unconditional lines untouched.
    let json = serde_json::to_string(&modern).unwrap();
    assert!(!json.contains("assumptions"));
}

/// I13 gating direction: unattested artifacts are correctly excluded.
#[test]
fn attestation_gates_by_property() {
    let facet = facet();
    let verified = address(&facet, 1);
    let unverified = address(&facet, 2);

    let mut set = AttestationSet::new();
    set.insert(
        Attestation::new(
            verified.clone(),
            "verified-total",
            Witness::new(
                "lean-proof",
                [("theorem".to_string(), "total_f".to_string())],
            )
            .unwrap(),
            None,
        )
        .unwrap(),
    );

    assert!(set.permits("verified-total", &verified.address_digest()));
    assert!(!set.permits("verified-total", &unverified.address_digest()));
    assert!(!set.permits("memory-safe", &verified.address_digest()));
}

#[test]
fn attestation_id_excludes_note() {
    let facet = facet();
    let make = |note: Option<&str>| {
        Attestation::new(
            address(&facet, 1),
            "tested",
            Witness::new("test-vector", []).unwrap(),
            note.map(String::from),
        )
        .unwrap()
    };
    assert_eq!(
        make(None).attestation_id(),
        make(Some("ran in CI")).attestation_id()
    );
}
