use riff_catalog_solidity::selection::{Document, View};
use serde_json::{Value, json};

fn fixture(x: i64, y: i64, name: &str) -> Value {
    json!({"nodeType":"SourceUnit","id":100,"src":"0:100:0","nodes":[
        {"nodeType":"VariableDeclaration","id":1,"src":"0:1:0","name":name},
        {"nodeType":"VariableDeclaration","id":2,"src":"2:1:0","name":"y"},
        {"nodeType":"BinaryOperation","id":3,"src":"4:5:0","operator":"+",
         "leftExpression":{"nodeType":"Identifier","id":4,"src":"4:1:0","name":name,"referencedDeclaration":x},
         "rightExpression":{"nodeType":"Identifier","id":5,"src":"8:1:0","name":name,"referencedDeclaration":y}}
    ]})
}

#[test]
fn resolved_ports_preserve_aliasing_and_erase_only_requested_names() {
    let a = fixture(1, 1, "x");
    let b = fixture(1, 1, "renamed");
    let c = fixture(1, 2, "x");
    let normal = |v: &Value, view| Document::new(v).unwrap().normalize(3, view).unwrap().normal;
    assert_eq!(normal(&a, View::Bindings), normal(&b, View::Bindings));
    assert_ne!(normal(&a, View::Names), normal(&b, View::Names));
    assert_ne!(normal(&a, View::Bindings), normal(&c, View::Bindings));
}

#[test]
fn transport_renumbering_does_not_change_local_or_boundary_references() {
    let a = fixture(1, 2, "x");
    let mut b = a.clone();
    fn renumber(v: &mut Value) {
        match v {
            Value::Object(map) => {
                for (key, value) in map {
                    if matches!(key.as_str(), "id" | "referencedDeclaration") {
                        *value = json!(value.as_i64().unwrap() + 1000);
                    } else {
                        renumber(value);
                    }
                }
            }
            Value::Array(a) => {
                for v in a {
                    renumber(v);
                }
            }
            _ => {}
        }
    }
    renumber(&mut b);
    for (left, right) in [(100, 1100), (3, 1003)] {
        assert_eq!(
            Document::new(&a)
                .unwrap()
                .normalize(left, View::Bindings)
                .unwrap()
                .normal,
            Document::new(&b)
                .unwrap()
                .normalize(right, View::Bindings)
                .unwrap()
                .normal
        );
    }
}

#[test]
fn malformed_and_unknown_inputs_are_not_nonmatches() {
    let mut a = fixture(1, 999, "x");
    assert!(
        Document::new(&a)
            .unwrap()
            .normalize(3, View::Bindings)
            .is_err()
    );
    a = fixture(1, 1, "x");
    a["nodes"][2]["futureSemanticField"] = json!(true);
    assert!(
        Document::new(&a)
            .unwrap()
            .normalize(3, View::Bindings)
            .is_err()
    );
    a["nodes"][1]["id"] = json!(1);
    assert!(Document::new(&a).is_err());
}

#[test]
fn selection_respects_enclosure_and_nonempty_ranges() {
    let a = fixture(1, 1, "x");
    let doc = Document::new(&a).unwrap();
    assert_eq!(doc.select_range(4, 9).unwrap(), 3);
    assert!(doc.select_range(4, 4).is_err());
    assert!(doc.select_range(101, 102).is_err());
}

#[test]
#[ignore = "requires a local solc; run explicitly with RIFFCAT_SOLC"]
fn real_compiler_source_retrieval_and_shadowing() {
    use riff_catalog_solidity::selection::{SourceArtifact, search};
    let runner = riff_catalog_solc::SolcRunner::locate(None);
    let sources = [
        "contract C { function f(uint x, uint y) public pure returns(uint) { return x + x; } }",
        "contract Renamed { function g(uint a, uint b) public pure returns(uint) { return a + a; } }",
        "contract Different { function f(uint x, uint y) public pure returns(uint) { return x + y; } }",
        "contract Shadow { function f(uint x) public pure returns(uint) { { uint x = 1; x++; } return x; } }",
    ];
    let artifacts: Vec<_> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let output = runner
                .compile(
                    &json!({"language":"Solidity","sources":{"input.sol":{"content":source}},
            "settings":{"outputSelection":{"*":{"": ["ast"]}}}}),
                )
                .unwrap();
            output.check_errors().unwrap();
            SourceArtifact {
                path: format!("{index}.sol"),
                source: source.to_string(),
                ast: output.source_ast("input.sol").unwrap().clone(),
            }
        })
        .collect();
    let start = sources[0].find("function").unwrap();
    let end = sources[0].rfind('}').unwrap() - 1;
    let result = search(&artifacts, 0, start, end, View::Bindings).unwrap();
    assert_eq!(result.matches.len(), 1);
    assert_eq!(result.matches[0].path, "1.sol");
    assert!(result.unsupported.is_empty(), "{:?}", result.unsupported);
    assert_eq!(result.excluded_self, 1);
    assert!(
        search(&artifacts, 0, start, end, View::Names)
            .unwrap()
            .matches
            .is_empty()
    );
    // The two same-spelled x declarations resolve to separate local positions.
    let doc = Document::new(&artifacts[3].ast).unwrap();
    let id = doc
        .candidates()
        .find(|(_, n)| n["nodeType"] == "FunctionDefinition")
        .unwrap()
        .0;
    let selected = doc.normalize(id, View::Bindings).unwrap();
    let locals: std::collections::BTreeSet<_> = selected
        .normal
        .nodes
        .iter()
        .filter_map(|n| {
            n.pointer("/fields/referencedDeclaration/local")
                .and_then(Value::as_u64)
        })
        .collect();
    assert_eq!(locals.len(), 2);
}
