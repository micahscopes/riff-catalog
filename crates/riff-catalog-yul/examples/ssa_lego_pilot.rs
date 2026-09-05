use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;

use riff_catalog_core::{
    CyclePolicy, Digest, DigestRequest, Dimension, DimensionDigests, EntityKey, Graph, GraphHashes,
    GraphKey, HashPolicy, NodeKey, Value, ViewMode, digest_graph,
};
use riff_catalog_solc::{SolcRunner, yul_input};
use riff_catalog_yul::lower::LoweredUnit;
use riff_catalog_yul::ssa::{YUL_SSA_LEVEL, lower_yul_cfg};

const SOURCE: &str = r#"object "LegoPilot" {
  code {
    function straight(x) -> r {
      let a := add(x, 7)
      let b := mul(a, 3)
      r := xor(b, 0x55)
    }

    function wrapped(x) -> r {
      let i := 0
      for { } lt(i, x) { i := add(i, 1) } {
        let a := add(x, 7)
        let b := mul(a, 3)
        r := xor(b, 0x55)
      }
    }

    function guarded(x) -> r {
      let i := 0
      for { } lt(i, x) { i := add(i, 1) } {
        if iszero(eq(i, 13)) {
          let a := add(x, 7)
          let b := mul(a, 3)
          r := xor(b, 0x55)
        }
      }
    }

    function carried(x) -> r {
      r := x
      let i := 0
      for { } lt(i, x) { i := add(i, 1) } {
        let a := add(r, 7)
        let b := mul(a, 3)
        r := xor(b, 0x55)
      }
    }

    function mutated(x) -> r {
      let a := add(x, 8)
      let b := mul(a, 3)
      r := xor(b, 0x55)
    }

    let one := add(straight(9), wrapped(9))
    let two := add(guarded(9), carried(9))
    let three := mutated(9)
    mstore(0, add(add(one, two), three))
    return(0, 32)
  }
}"#;

const STRAIGHT_MAIN: &str = r#"object "Straight" {
  code {
    let x := calldataload(0)
    let a := add(x, 7)
    let b := mul(a, 3)
    let r := xor(b, 0x55)
    mstore(0, r)
    return(0, 32)
  }
}"#;

const WRAPPED_MAIN: &str = r#"object "Wrapped" {
  code {
    let x := calldataload(0)
    let n := calldataload(32)
    let r := 0
    let i := 0
    for { } lt(i, n) { i := add(i, 1) } {
      let a := add(x, 7)
      let b := mul(a, 3)
      r := xor(b, 0x55)
    }
    mstore(0, r)
    return(0, 32)
  }
}"#;

type Address = [Digest; 5];

#[derive(Clone)]
struct Instruction {
    op: String,
    local: Address,
    tree: Address,
    component_size: usize,
}

fn address(digests: &DimensionDigests) -> Address {
    Dimension::ALL.map(|dimension| *digests.get(dimension).expect("all dimensions requested"))
}

fn hash(unit: &LoweredUnit) -> GraphHashes {
    let policy = HashPolicy::new(
        YUL_SSA_LEVEL,
        ViewMode::AnonymousShape,
        CyclePolicy::CondenseScc,
    )
    .unwrap();
    digest_graph(
        &DigestRequest::all_dimensions(unit.graph_key.clone(), policy),
        &unit.graph,
    )
    .unwrap()
    .hashes
}

fn op(unit: &LoweredUnit, key: &NodeKey) -> String {
    unit.graph.nodes[key]
        .fields
        .iter()
        .find(|field| field.name.as_str() == "op")
        .and_then(|field| match &field.value {
            Value::Text(value) => Some(value.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "call".to_string())
}

fn instructions(unit: &LoweredUnit, hashes: &GraphHashes) -> Vec<Instruction> {
    unit.graph
        .nodes
        .iter()
        .filter(|(_, node)| node.kind.as_str() == "yulssa.insn")
        .map(|(key, _)| {
            let node_hashes = &hashes.nodes[key];
            let component_size = hashes
                .components
                .iter()
                .find(|component| component.members.contains(key))
                .map_or(0, |component| component.members.len());
            Instruction {
                op: op(unit, key),
                local: address(&node_hashes.local),
                tree: address(&node_hashes.tree),
                component_size,
            }
        })
        .collect()
}

fn multiset_matches(
    reference: &[Instruction],
    candidate: &[Instruction],
    select: impl Fn(&Instruction) -> Address,
) -> usize {
    let mut available = candidate
        .iter()
        .fold(BTreeMap::new(), |mut counts, instruction| {
            *counts.entry(select(instruction)).or_insert(0usize) += 1;
            counts
        });
    reference
        .iter()
        .filter(|instruction| {
            let count = available.entry(select(instruction)).or_default();
            if *count == 0 {
                false
            } else {
                *count -= 1;
                true
            }
        })
        .count()
}

fn compile_object(
    solc: &SolcRunner,
    source_name: &str,
    object_name: &str,
    source: &str,
) -> Result<LoweredUnit, Box<dyn Error>> {
    let output = solc.compile(&yul_input(source_name, source, true))?;
    output.check_errors()?;
    let cfg = output.yul_cfg_json(source_name, object_name)?;
    let lowered = lower_yul_cfg(cfg, &format!("pilot:optimized:{object_name}"))?;
    lowered
        .objects
        .into_iter()
        .find(|unit| unit.name == object_name)
        .ok_or_else(|| format!("missing object {object_name}").into())
}

fn payload_instructions(unit: &LoweredUnit) -> Vec<NodeKey> {
    for (block_key, node) in &unit.graph.nodes {
        if node.kind.as_str() != "yulssa.block" {
            continue;
        }
        let mut block_instructions = unit
            .graph
            .children
            .iter()
            .filter(|child| child.parent == *block_key && child.label.as_str() == "insn")
            .map(|child| (child.ordinal, child.child.clone()))
            .collect::<Vec<_>>();
        block_instructions.sort_by_key(|(ordinal, _)| *ordinal);
        for window in block_instructions.windows(3) {
            let operations = window
                .iter()
                .map(|(_, key)| op(unit, key))
                .collect::<Vec<_>>();
            if operations == ["add", "mul", "xor"] {
                return window.iter().map(|(_, key)| key.clone()).collect();
            }
        }
    }
    panic!("payload sequence not found in {}", unit.name);
}

fn payload_graph(unit: &LoweredUnit, owner: &str) -> Graph {
    // This is the Lego seam under test. Keep the ordered instruction forest
    // and data edges whose endpoints are both inside the selected region.
    // Data edges crossing the boundary become implicit input/output ports, so
    // the same payload can be recognized under a different CFG or phi shell.
    let payload = payload_instructions(unit);
    let mut live = payload.iter().cloned().collect::<BTreeSet<_>>();
    let mut queue = payload.iter().cloned().collect::<VecDeque<_>>();
    while let Some(parent) = queue.pop_front() {
        for child in unit
            .graph
            .children
            .iter()
            .filter(|child| child.parent == parent)
        {
            if live.insert(child.child.clone()) {
                queue.push_back(child.child.clone());
            }
        }
    }

    let root_entity = EntityKey::new("yulssa.payload", owner, "root").unwrap();
    let root = NodeKey::entity(root_entity.clone());
    let mut graph = Graph::new(GraphKey::new(root_entity, "ordered-payload").unwrap());
    graph.add_node(root.clone(), "yulssa.payload").unwrap();
    for key in &live {
        graph
            .nodes
            .insert(key.clone(), unit.graph.nodes[key].clone());
    }
    for (ordinal, instruction) in payload.iter().enumerate() {
        graph
            .add_child(&root, "insn", ordinal as u32, instruction)
            .unwrap();
    }
    graph.children.extend(
        unit.graph
            .children
            .iter()
            .filter(|child| live.contains(&child.parent) && live.contains(&child.child))
            .cloned(),
    );
    graph.edges.extend(
        unit.graph
            .edges
            .iter()
            .filter(|edge| live.contains(&edge.source) && live.contains(&edge.target))
            .cloned(),
    );
    graph.validate().unwrap();
    graph
}

fn payload_address(unit: &LoweredUnit, owner: &str) -> Address {
    let graph = payload_graph(unit, owner);
    let policy = HashPolicy::new(
        "view:yulssa.ordered-payload/1",
        ViewMode::AnonymousShape,
        CyclePolicy::CondenseScc,
    )
    .unwrap();
    let hashes = digest_graph(
        &DigestRequest::all_dimensions(graph.graph_key.clone(), policy),
        &graph,
    )
    .unwrap()
    .hashes;
    address(&hashes.graph)
}

fn main() -> Result<(), Box<dyn Error>> {
    let solc = SolcRunner::locate(None);
    let version = solc.version()?;
    let output = solc.compile(&yul_input("pairs.yul", SOURCE, false))?;
    output.check_errors()?;
    let cfg = output.yul_cfg_json("pairs.yul", "LegoPilot")?;
    let lowered = lower_yul_cfg(cfg, "pilot:lego")?;

    let units: BTreeMap<_, _> = lowered
        .functions
        .iter()
        .map(|unit| (unit.name.as_str(), unit))
        .collect();
    let reference_unit = units["straight"];
    let reference_hashes = hash(reference_unit);
    let reference = instructions(reference_unit, &reference_hashes);

    println!("solc {version}, optimizer off, anonymous yul-ssa-cfg/1");
    println!("reference payload: add(x, 7) -> mul(_, 3) -> xor(_, 0x55)");
    println!();
    let reference_payload = payload_address(reference_unit, "pilot:payload:straight");
    println!(
        "variant    blocks  max-scc  local     subtree   whole-body  exact ops       payload-scc  graph"
    );

    for name in ["straight", "wrapped", "guarded", "carried", "mutated"] {
        let unit = units[name];
        let hashes = hash(unit);
        let candidate = instructions(unit, &hashes);
        let local = multiset_matches(&reference, &candidate, |instruction| instruction.local);
        let tree = multiset_matches(&reference, &candidate, |instruction| instruction.tree);
        let blocks = unit
            .graph
            .nodes
            .values()
            .filter(|node| node.kind.as_str() == "yulssa.block")
            .count();
        let max_scc = hashes
            .components
            .iter()
            .map(|component| component.members.len())
            .max()
            .unwrap_or(0);
        let payload_scc = candidate
            .iter()
            .filter(|instruction| {
                reference
                    .iter()
                    .any(|reference| reference.tree == instruction.tree)
            })
            .map(|instruction| instruction.component_size)
            .max()
            .unwrap_or(0);
        let exact_ops = reference
            .iter()
            .map(|reference| {
                let matched = candidate
                    .iter()
                    .any(|instruction| instruction.tree == reference.tree);
                format!("{}:{}", reference.op, if matched { "yes" } else { "no" })
            })
            .collect::<Vec<_>>()
            .join(" ");
        let body_same =
            payload_address(unit, &format!("pilot:payload:{name}")) == reference_payload;
        let graph_same = hashes.graph.values == reference_hashes.graph.values;
        println!(
            "{name:<10} {blocks:>6}  {max_scc:>7}  {local:>2}/{total:<2}     {tree:>2}/{total:<2}     {:<10}  {exact_ops:<27} {payload_scc:>3}         {}",
            if body_same { "same" } else { "different" },
            if graph_same { "same" } else { "different" },
            total = reference.len(),
        );
        if name == "carried" {
            for reference_instruction in &reference {
                if candidate
                    .iter()
                    .any(|instruction| instruction.tree == reference_instruction.tree)
                {
                    continue;
                }
                for candidate_instruction in candidate
                    .iter()
                    .filter(|instruction| instruction.op == reference_instruction.op)
                {
                    let changed = Dimension::ALL
                        .into_iter()
                        .enumerate()
                        .filter(|(index, _)| {
                            reference_instruction.tree[*index] != candidate_instruction.tree[*index]
                        })
                        .map(|(_, dimension)| dimension.as_str())
                        .collect::<Vec<_>>()
                        .join(",");
                    println!(
                        "  carried {} candidate subtree={} changed={changed}",
                        candidate_instruction.op,
                        candidate_instruction.tree[0].display_short(),
                    );
                }
            }
        }
    }

    println!();
    println!("reference instruction addresses:");
    for instruction in &reference {
        println!(
            "  {:<4} local={} subtree={}",
            instruction.op,
            instruction.local[0].display_short(),
            instruction.tree[0].display_short(),
        );
    }

    let optimized_straight = compile_object(&solc, "straight-main.yul", "Straight", STRAIGHT_MAIN)?;
    let optimized_wrapped = compile_object(&solc, "wrapped-main.yul", "Wrapped", WRAPPED_MAIN)?;
    let optimized_straight_hashes = hash(&optimized_straight);
    let optimized_wrapped_hashes = hash(&optimized_wrapped);
    let optimized_reference = instructions(&optimized_straight, &optimized_straight_hashes)
        .into_iter()
        .filter(|instruction| ["add", "mul", "xor"].contains(&instruction.op.as_str()))
        .collect::<Vec<_>>();
    let optimized_candidate = instructions(&optimized_wrapped, &optimized_wrapped_hashes);
    let optimized_local =
        multiset_matches(&optimized_reference, &optimized_candidate, |instruction| {
            instruction.local
        });
    let optimized_tree =
        multiset_matches(&optimized_reference, &optimized_candidate, |instruction| {
            instruction.tree
        });
    let optimized_payload_scc = optimized_candidate
        .iter()
        .filter(|instruction| {
            optimized_reference
                .iter()
                .any(|reference| reference.tree == instruction.tree)
        })
        .map(|instruction| instruction.component_size)
        .max()
        .unwrap_or(0);
    let optimized_body_same = payload_address(&optimized_straight, "pilot:payload:optimized-a")
        == payload_address(&optimized_wrapped, "pilot:payload:optimized-b");

    println!();
    println!("optimized top-level objects, compiled separately:");
    println!(
        "  payload local {optimized_local}/{total}, subtree {optimized_tree}/{total}, whole body {}, payload SCC {optimized_payload_scc}, graph {}",
        if optimized_body_same {
            "same"
        } else {
            "different"
        },
        if optimized_straight_hashes.graph.values == optimized_wrapped_hashes.graph.values {
            "same"
        } else {
            "different"
        },
        total = optimized_reference.len(),
    );

    Ok(())
}
