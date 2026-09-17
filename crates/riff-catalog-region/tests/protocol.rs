use riff_catalog_region::{
    NormalOperand,
    protocol::{Request, compare},
};
use serde_json::{Value, json};

fn request() -> Value {
    let region = json!({"operations":[
        {"op":"sub","operands":[{"External":"x"},{"External":"y"}]},
        {"op":"add","operands":[{"Result":0},{"External":"x"}]}
    ],"outputs":[1]});
    json!({"schema":"riffcat-ordered-region-request/1","id":"example",
        "view":"literal-tokens","left":region,"right":region})
}

fn equal(value: Value) -> bool {
    compare(serde_json::from_value(value).unwrap())
        .unwrap()
        .equivalent
}

#[test]
fn alpha_renaming_preserves_repeated_bindings_but_rewiring_does_not() {
    let mut q = request();
    q["right"]["operations"][0]["operands"][0] = json!({"External":"a"});
    q["right"]["operations"][0]["operands"][1] = json!({"External":"b"});
    q["right"]["operations"][1]["operands"][1] = json!({"External":"a"});
    assert!(equal(q.clone()));
    q["right"]["operations"][1]["operands"][1] = json!({"External":"b"});
    assert!(!equal(q));
}

#[test]
fn operand_order_and_output_boundary_are_retained() {
    let mut q = request();
    q["right"]["operations"][0]["operands"] = json!([{"External":"y"},{"External":"x"}]);
    assert!(!equal(q));
    let mut q = request();
    q["right"]["outputs"] = json!([0, 1]);
    assert!(!equal(q));
}

#[test]
fn literal_tokens_are_explicitly_not_numeric_normalization() {
    let mut q = request();
    q["left"]["operations"][0]["operands"][1] = json!({"Literal":"7"});
    q["right"]["operations"][0]["operands"][1] = json!({"Literal":"0x7"});
    assert!(!equal(q.clone()));
    q["view"] = json!("ignore-literals");
    assert!(equal(q));
}

#[test]
fn unknown_fields_and_unsupported_schemas_are_not_inequality() {
    let mut q = request();
    q["right"]["hidden_wiring"] = json!([0, 1]);
    assert!(serde_json::from_value::<Request>(q).is_err());
    let mut q = request();
    q["schema"] = json!("other/1");
    assert!(compare(serde_json::from_value(q).unwrap()).is_err());
    let mut q = request();
    q["right"]["operations"][0]["op"] = json!("sstore");
    assert!(compare(serde_json::from_value(q).unwrap()).is_err());
}

#[test]
fn malformed_normalized_records_return_errors_not_panics() {
    let mut normalized = compare(serde_json::from_value(request()).unwrap())
        .unwrap()
        .left;
    normalized.operations[0].operands[0] = NormalOperand::Input(usize::MAX);
    assert!(normalized.graph().is_err());
    normalized.operations[0].operands[0] = NormalOperand::Result(usize::MAX);
    assert!(normalized.graph().is_err());
}

#[test]
fn operation_limit_rejects_before_comparison() {
    let mut q: Request = serde_json::from_value(request()).unwrap();
    q.left.operations = vec![q.left.operations[0].clone(); 4097];
    assert!(compare(q).is_err());
}
