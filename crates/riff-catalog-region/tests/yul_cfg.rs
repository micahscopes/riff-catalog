use riff_catalog_region::{
    exact::{Budget, Outcome, canonicalize},
    yul_cfg::{materialize, pure_window},
};
use serde_json::{Value, json};

fn function() -> Value {
    json!({"type":"Function","entry":"Block0","arguments":["v0"],"numReturns":1,"blocks":[
        {"id":"Block0","instructions":[{"op":"split","in":["v0"],"out":["v1","v2"]}],"exit":{"type":"Jump","targets":["Block1"]}},
        {"id":"Block1","entries":["Block0","Block1"],"instructions":[
            {"op":"PhiFunction","in":["v1","v4"],"out":["v3"]},
            {"op":"add","in":["v3","0x01"],"out":["v4"]}],
         "exit":{"type":"ConditionalJump","cond":"v0","targets":["Block2","Block1"]}},
        {"id":"Block2","instructions":[],"exit":{"type":"FunctionReturn","returnValues":["v3"]}}
    ]})
}

fn canon(function: &Value, blocks: &[String]) -> riff_catalog_region::exact::Graph {
    let region = materialize(function, blocks).unwrap();
    match canonicalize(&region.graph, Budget::default()) {
        Outcome::Exact {
            canonical, mapping, ..
        } => {
            assert!(riff_catalog_region::exact::verify(
                &region.graph,
                &canonical,
                &mapping
            ));
            canonical
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn result_ordinals_and_phi_pairings_are_not_erased() {
    let a = function();
    let baseline = canon(&a, &[]);
    let mut b = a.clone();
    b["blocks"][1]["instructions"][0]["in"][0] = json!("v2");
    assert_ne!(baseline, canon(&b, &[]));
    b = a.clone();
    b["blocks"][1]["entries"].as_array_mut().unwrap().reverse();
    b["blocks"][1]["instructions"][0]["in"]
        .as_array_mut()
        .unwrap()
        .reverse();
    assert_eq!(baseline, canon(&b, &[]));
    b["blocks"][1]["instructions"][0]["in"]
        .as_array_mut()
        .unwrap()
        .reverse();
    assert_ne!(baseline, canon(&b, &[]));
}

#[test]
fn cyclic_selection_ignores_outside_body_not_boundary() {
    let a = function();
    let selection = vec!["Block1".into()];
    assert!(materialize(&a, &selection).unwrap().has_control_cycle);
    let baseline = canon(&a, &selection);
    let mut b = a.clone();
    b["blocks"][2]["instructions"] = json!([{"op":"mstore","in":["0x00","v0"],"out":[]}]);
    assert_eq!(baseline, canon(&b, &selection));
    assert_ne!(canon(&a, &[]), canon(&b, &[]));
    b["blocks"][2]["instructions"][0]["in"][1] = json!("v4");
    assert_ne!(baseline, canon(&b, &selection)); // another selected result now crosses the boundary
}

#[test]
fn transport_ids_and_block_order_are_occurrences() {
    let a = function();
    let mut b = a.clone();
    fn rename(value: &mut Value) {
        match value {
            Value::String(s) if s.starts_with("Block") || s.starts_with('v') => {
                *s = format!("renamed:{s}");
            }
            Value::Array(a) => {
                for v in a {
                    rename(v);
                }
            }
            Value::Object(o) => {
                for v in o.values_mut() {
                    rename(v);
                }
            }
            _ => {}
        }
    }
    rename(&mut b);
    b["blocks"].as_array_mut().unwrap().reverse();
    assert_eq!(canon(&a, &[]), canon(&b, &[]));
}

#[test]
fn malformed_phi_and_unknown_fields_fail_closed() {
    let mut a = function();
    a["blocks"][1]["entries"] = json!(["Block0"]);
    assert!(materialize(&a, &[]).is_err());
    a = function();
    a["blocks"][1]["instructions"][0]["futureField"] = json!(true);
    assert!(materialize(&a, &[]).is_err());
}

#[test]
fn irreducible_multi_entry_selection_and_budget_outcomes() {
    let f = json!({"type":"Function","entry":"E","arguments":["v0"],"numReturns":0,"blocks":[
        {"id":"E","instructions":[],"exit":{"type":"ConditionalJump","cond":"v0","targets":["A","B"]}},
        {"id":"A","instructions":[],"exit":{"type":"Jump","targets":["B"]}},
        {"id":"B","instructions":[],"exit":{"type":"ConditionalJump","cond":"v0","targets":["A","X"]}},
        {"id":"X","instructions":[],"exit":{"type":"FunctionReturn","returnValues":[]}}
    ]});
    let selected = vec!["A".into(), "B".into()];
    let a = materialize(&f, &selected).unwrap();
    assert!(a.has_control_cycle);
    let comparison =
        riff_catalog_region::yul_cfg::compare(a.clone(), a.clone(), Budget::default()).unwrap();
    assert_eq!(comparison.equivalent, Some(true));
    assert!(comparison.witness.is_some());
    let exhausted = riff_catalog_region::yul_cfg::compare(
        a.clone(),
        a,
        Budget {
            states: 0,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(exhausted.equivalent, None);
    assert_eq!(exhausted.left_content_id, None);
    assert_eq!(exhausted.witness, None);
}

#[test]
fn pure_window_cannot_misclassify_forward_or_unknown_references_as_ports() {
    let mut f = function();
    f["blocks"][1]["instructions"] = json!([
        {"op":"add","in":["v9","0x01"],"out":["v8"]},
        {"op":"mul","in":["v8","0x02"],"out":["v9"]}
    ]);
    assert!(pure_window(&f, "Block1", 0, 2, vec![1]).is_err());
    f["blocks"][1]["instructions"][0]["in"][0] = json!("missing");
    assert!(pure_window(&f, "Block1", 0, 2, vec![1]).is_err());
}

#[test]
#[ignore = "requires modified solc with experimental yulCFGJson"]
fn real_cyclic_capture_and_independent_payload() {
    let runner = riff_catalog_solc::SolcRunner::locate(None);
    let source = include_str!("../fixtures/regions.yul");
    let mut input = riff_catalog_solc::yul_input("regions.yul", source, false);
    input["settings"]["experimental"] = json!(true);
    let output = runner.compile(&input).unwrap();
    output.check_errors().unwrap();
    let cfg = output.yul_cfg_json("regions.yul", "RegionPilot").unwrap();
    let functions = &cfg["RegionPilot"]["functions"];
    let wrapped = &functions["wrapped"];
    let selection = vec!["Block1".into(), "Block2".into()];
    assert!(materialize(wrapped, &selection).unwrap().has_control_cycle);
    assert_ne!(
        canon(wrapped, &selection),
        canon(&functions["changed_header"], &selection)
    );
    // Explicit IR location selection, no opcode-pattern finder.
    let body = pure_window(wrapped, "Block2", 0, 3, vec![2])
        .unwrap()
        .normalize()
        .unwrap()
        .0;
    let changed = pure_window(&functions["changed_header"], "Block2", 0, 3, vec![2])
        .unwrap()
        .normalize()
        .unwrap()
        .0;
    let straight = pure_window(&functions["straight"], "Block0", 0, 3, vec![2])
        .unwrap()
        .normalize()
        .unwrap()
        .0;
    assert_eq!(body, changed);
    assert_eq!(body, straight);
}
