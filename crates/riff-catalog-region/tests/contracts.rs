use std::collections::{BTreeMap, BTreeSet};

use riff_catalog_region::{CONTRACT, Operand, Operation, Region, address_graph, wl_observation};

fn region(names: [&str; 3], literal: &str, result: usize, first_op: &str) -> Region {
    Region {
        operations: vec![
            Operation {
                op: first_op.into(),
                operands: vec![
                    Operand::External(names[0].into()),
                    Operand::Literal(literal.into()),
                ],
            },
            Operation {
                op: "mul".into(),
                operands: vec![Operand::Result(0), Operand::External(names[1].into())],
            },
            Operation {
                op: "xor".into(),
                operands: vec![Operand::Result(result), Operand::External(names[2].into())],
            },
        ],
        outputs: vec![2],
    }
}

/// Exact oracle for this ordered grammar: enumerate every bijective naming of
/// the external values and take the least serialized operation/operand tape.
/// This does not call the production normalizer, graph emitter, or hash engine.
fn exact_form(region: &Region, literals: bool) -> String {
    let names: Vec<_> = region
        .operations
        .iter()
        .flat_map(|operation| &operation.operands)
        .filter_map(|operand| match operand {
            Operand::External(key) => Some(key),
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    fn visit(
        order: &mut [usize],
        at: usize,
        names: &[&String],
        region: &Region,
        literals: bool,
        best: &mut Option<String>,
    ) {
        if at < order.len() {
            for i in at..order.len() {
                order.swap(at, i);
                visit(order, at + 1, names, region, literals, best);
                order.swap(at, i);
            }
            return;
        }
        let mapped: BTreeMap<_, _> = names.iter().zip(order.iter()).collect();
        let operations: Vec<_> = region
            .operations
            .iter()
            .map(|op| {
                let operands: Vec<_> = op
                    .operands
                    .iter()
                    .map(|operand| match operand {
                        Operand::External(name) => format!("input:{}", mapped[&name]),
                        Operand::Result(index) => format!("result:{index}"),
                        Operand::Literal(value) => {
                            format!("literal:{}", if literals { value.as_str() } else { "_" })
                        }
                    })
                    .collect();
                (op.op.as_str(), operands)
            })
            .collect();
        let encoded = serde_json::to_string(&(operations, &region.outputs)).unwrap();
        if best.as_ref().is_none_or(|current| encoded < *current) {
            *best = Some(encoded);
        }
    }
    let mut best = None;
    visit(
        &mut (0..names.len()).collect::<Vec<_>>(),
        0,
        &names,
        region,
        literals,
        &mut best,
    );
    best.unwrap()
}

#[test]
fn address_classes_agree_with_exhaustive_port_renaming_oracle() {
    for literals in [false, true] {
        let mut by_hash = BTreeMap::new();
        let mut by_exact = BTreeMap::new();
        let mut cases = 0;
        for a in ["a", "b", "c"] {
            for b in ["a", "b", "c"] {
                for c in ["a", "b", "c"] {
                    for literal in ["7", "8"] {
                        for result in [0, 1] {
                            for op in ["add", "sub"] {
                                let raw = region([a, b, c], literal, result, op);
                                let exact = exact_form(&raw, literals);
                                let hash = raw.normalize().unwrap().0.address(literals).unwrap();
                                if let Some(previous) = by_hash.insert(hash, exact.clone()) {
                                    assert_eq!(
                                        previous, exact,
                                        "false match: {raw:?}, literals={literals}"
                                    );
                                }
                                if let Some(previous) = by_exact.insert(exact, hash) {
                                    assert_eq!(previous, hash, "false split: {raw:?}");
                                }
                                cases += 1;
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(cases, 216);
        assert_eq!(by_exact.len(), if literals { 40 } else { 20 });
    }
}

#[test]
fn repeated_and_distinct_inputs_and_equal_count_alias_patterns_stay_distinct() {
    let hash = |names| {
        region(names, "7", 1, "add")
            .normalize()
            .unwrap()
            .0
            .address(true)
            .unwrap()
    };
    assert_ne!(hash(["x", "x", "x"]), hash(["x", "x", "y"]));
    assert_ne!(hash(["x", "x", "y"]), hash(["x", "y", "x"]));
    assert_eq!(hash(["x", "x", "y"]), hash(["renamed", "renamed", "other"]));
}

#[test]
fn literal_facet_and_output_boundary_obey_the_contract() {
    let a = region(["x", "x", "x"], "7", 1, "add")
        .normalize()
        .unwrap()
        .0;
    let mut b = region(["x", "x", "x"], "8", 1, "add")
        .normalize()
        .unwrap()
        .0;
    assert_ne!(a.address(true).unwrap(), b.address(true).unwrap());
    assert_eq!(a.address(false).unwrap(), b.address(false).unwrap());
    b.outputs = vec![1, 2];
    assert_ne!(a.address(false).unwrap(), b.address(false).unwrap());
}

#[test]
fn invalid_regions_are_rejected() {
    let mut raw = region(["x", "x", "x"], "7", 1, "add");
    raw.operations[0].operands[0] = Operand::Result(2);
    assert!(raw.normalize().is_err());
    raw.operations[0].operands[0] = Operand::External("x".into());
    raw.operations[0].op = "sstore".into();
    assert!(raw.normalize().is_err());
}

#[test]
fn transport_order_and_producer_keys_do_not_define_region_identity() {
    use riff_catalog_core::{EntityKey, Graph, GraphKey, NodeKey};
    let normal = region(["x", "y", "x"], "7", 1, "add")
        .normalize()
        .unwrap()
        .0;
    let original = normal.graph().unwrap();
    let owner = EntityKey::new("different.producer", "another.stage", "unit").unwrap();
    let map: BTreeMap<_, _> = original
        .nodes
        .keys()
        .rev()
        .enumerate()
        .map(|(i, key)| {
            (
                key.clone(),
                NodeKey::derived(owner.clone(), format!("fresh:{i}")).unwrap(),
            )
        })
        .collect();
    let mut renamed = original.clone();
    renamed.graph_key = GraphKey::new(owner, "different serialization").unwrap();
    renamed.nodes = original
        .nodes
        .iter()
        .map(|(key, node)| {
            let mut node = node.clone();
            node.key = map[key].clone();
            (node.key.clone(), node)
        })
        .collect();
    for child in &mut renamed.children {
        child.parent = map[&child.parent].clone();
        child.child = map[&child.child].clone();
    }
    for edge in &mut renamed.edges {
        edge.source = map[&edge.source].clone();
        edge.target = map[&edge.target].clone();
    }
    renamed.children.reverse();
    renamed.edges.reverse();
    let roundtrip: Graph =
        serde_json::from_str(&serde_json::to_string_pretty(&renamed).unwrap()).unwrap();
    assert_eq!(
        address_graph(&original, CONTRACT, true).unwrap(),
        address_graph(&roundtrip, CONTRACT, true).unwrap()
    );
}

#[test]
fn wl_equivalence_is_a_useful_but_coarser_observation() {
    let report = wl_observation::run().unwrap();
    assert_eq!(report["same_coarse_address"], true);
    assert_eq!(report["exact_isomorphic"], false);
    let edges = wl_observation::prism();
    let renamed: Vec<_> = edges.iter().map(|&(a, b)| (5 - a, 5 - b)).collect();
    assert_eq!(
        wl_observation::exact_form(&edges, 6),
        wl_observation::exact_form(&renamed, 6)
    );
}
