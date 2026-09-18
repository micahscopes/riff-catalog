use riff_catalog_region::{
    exact::Budget,
    yul_cfg::{compare, materialize},
};
use serde_json::Value;

fn functions() -> Value {
    serde_json::from_str::<Value>(include_str!("../fixtures/extended-cfg.json")).unwrap()["ExtendedRegions"]["functions"].clone()
}

#[test]
fn actual_nested_loop_survives_transport_reordering() {
    let f = functions();
    let original = &f["nested"];
    let mut reordered = original.clone();
    reordered["blocks"].as_array_mut().unwrap().reverse();
    let left = materialize(original, &[]).unwrap();
    let right = materialize(&reordered, &[]).unwrap();
    assert!(left.has_control_cycle);
    let result = compare(left, right, Budget::default()).unwrap();
    assert_eq!(result.equivalent, Some(true));
    assert!(result.witness.is_some());
}

#[test]
fn actual_two_exit_loop_keeps_both_boundary_targets() {
    let f = functions();
    let original = &f["exits"];
    let selected = vec!["Block1".into(), "Block2".into(), "Block6".into()];
    let left = materialize(original, &selected).unwrap();
    assert!(left.has_control_cycle);
    let outside: Vec<_> = left
        .occurrences
        .iter()
        .enumerate()
        .filter(|(_, o)| o["outside"] == true)
        .map(|(i, _)| i)
        .collect();
    let exit_targets: std::collections::BTreeSet<_> = left
        .graph
        .edges
        .iter()
        .filter(|e| e.role.starts_with("control:") && outside.contains(&e.to))
        .map(|e| e.to)
        .collect();
    assert_eq!(exit_targets.len(), 2);
    let mut changed = original.clone();
    let block = changed["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|b| b["id"] == "Block2")
        .unwrap();
    block["exit"]["targets"].as_array_mut().unwrap().reverse();
    let result = compare(
        left,
        materialize(&changed, &selected).unwrap(),
        Budget::default(),
    )
    .unwrap();
    assert_eq!(result.equivalent, Some(false));
}

#[test]
fn effect_order_is_structural_even_when_operations_commute() {
    let f = functions();
    let a = materialize(&f["effects"], &[]).unwrap();
    let b = materialize(&f["effects_swapped"], &[]).unwrap();
    assert_eq!(
        compare(a, b, Budget::default()).unwrap().equivalent,
        Some(false)
    );
    // This contract does not reorder stores or establish behavior equivalence.
}
