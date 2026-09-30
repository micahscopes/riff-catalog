//! Sonatina functions compared by facet: which functions of one IR module
//! are the same code at a facet that forgets types (or types and constants),
//! for example one generic Fe function specialized for several layouts.
//!
//! Each function becomes its own graph
//! ([`riff_catalog_sonatina::project_functions`]) at level `sonatina-ir/1`.
//! Names never enter an address (anonymous shape, and names are their own
//! dimension).

use std::collections::BTreeMap;

use anyhow::Result;
use riff_catalog_core::{
    CyclePolicy, DigestRequest, Dimension, Facet, HashPolicy, ViewMode, digest_graph,
};
use riff_catalog_sonatina::{SONATINA_IR_LEVEL, parse_and_lower_module, project_functions};
use serde::{Deserialize, Serialize};

/// One class: functions with one address at a facet.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionClass {
    pub address: String,
    /// (function name, emitted bytes when the regions name it).
    pub functions: Vec<(String, Option<u64>)>,
    pub emitted_bytes: u64,
    /// Emitted bytes minus the largest copy: what one shared version would
    /// save before any cost of making it generic. An upper bound.
    pub upper_bound_saving: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionFacetCensus {
    pub facet: String,
    pub functions: usize,
    /// Classes of two or more functions.
    pub classes: Vec<FunctionClass>,
    pub upper_bound_saving: u64,
}

/// Group a module's functions at three facets: Structure + Types +
/// Constants (exact), Structure + Constants (types blind), and Structure
/// (types and constants blind). `bytes` gives emitted bytes per function.
pub fn sonatina_function_facets(
    source: &str,
    bytes: &BTreeMap<String, u64>,
) -> Result<Vec<FunctionFacetCensus>> {
    let graphs = project_functions(&parse_and_lower_module("sonatina", source)?)?;
    let policy = HashPolicy::new(
        SONATINA_IR_LEVEL,
        ViewMode::AnonymousShape,
        CyclePolicy::NonRecursiveGraphEdges,
    )?;
    let facets = [
        (
            "exact (structure+types+constants)",
            vec![Dimension::Structure, Dimension::Types, Dimension::Constants],
        ),
        (
            "types blind (structure+constants)",
            vec![Dimension::Structure, Dimension::Constants],
        ),
        (
            "types and constants blind (structure)",
            vec![Dimension::Structure],
        ),
    ];
    let mut out = Vec::new();
    for (label, dims) in facets {
        let facet = Facet::new(policy.policy_id(), dims.iter().copied())?;
        let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (name, graph) in &graphs {
            let request = DigestRequest::new(
                graph.graph_key.clone(),
                policy.clone(),
                dims.iter().copied(),
            )?;
            let address = digest_graph(&request, graph)?
                .hashes
                .facet_address(&facet)?
                .address_digest()
                .to_hex();
            groups.entry(address).or_default().push(name.clone());
        }
        let mut classes: Vec<FunctionClass> = groups
            .into_iter()
            .filter(|(_, v)| v.len() > 1)
            .map(|(address, names)| {
                let functions: Vec<(String, Option<u64>)> = names
                    .iter()
                    .map(|n| (n.clone(), bytes.get(n).copied()))
                    .collect();
                let known: Vec<u64> = functions.iter().filter_map(|f| f.1).collect();
                let emitted: u64 = known.iter().sum();
                let largest = known.iter().copied().max().unwrap_or(0);
                FunctionClass {
                    address,
                    functions,
                    emitted_bytes: emitted,
                    upper_bound_saving: emitted - largest,
                }
            })
            .collect();
        classes.sort_by(|a, b| {
            b.upper_bound_saving
                .cmp(&a.upper_bound_saving)
                .then_with(|| a.address.cmp(&b.address))
        });
        out.push(FunctionFacetCensus {
            facet: label.into(),
            functions: graphs.len(),
            upper_bound_saving: classes.iter().map(|c| c.upper_bound_saving).sum(),
            classes,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A global's only identity in the lowering is a Names field, and names
    /// never enter an address: functions reading different globals must
    /// still differ at the facets that keep constants.
    #[test]
    fn functions_reading_different_globals_differ_at_the_exact_facet() {
        let ir = r#"
target = "evm-ethereum-london"

global private i256 $G = 1;
global private i256 $H = 2;

func public %a() -> i256 {
    block0:
        v1.i256 = ptr_to_int $G i256;
        return v1;
}

func public %b() -> i256 {
    block0:
        v1.i256 = ptr_to_int $H i256;
        return v1;
}

func public %c() -> i256 {
    block0:
        v1.i256 = ptr_to_int $G i256;
        return v1;
}
"#;
        let census = sonatina_function_facets(ir, &BTreeMap::new()).unwrap();
        let classes = |i: usize| -> Vec<Vec<String>> {
            census[i]
                .classes
                .iter()
                .map(|c| c.functions.iter().map(|f| f.0.clone()).collect())
                .collect()
        };
        assert_eq!(classes(0), vec![vec!["a".to_string(), "c".to_string()]]);
        assert_eq!(classes(2).len(), 1);
        assert_eq!(classes(2)[0].len(), 3);
    }

    #[test]
    fn types_blind_groups_functions_that_differ_only_in_types() {
        let ir = r#"
target = "evm-ethereum-osaka"

func public %a(v0.i256) -> i256 {
    block0:
        v1.i256 = add v0 1.i256;
        return v1;
}

func public %b(v0.i64) -> i64 {
    block0:
        v1.i64 = add v0 1.i64;
        return v1;
}

func public %c(v0.i256) -> i256 {
    block0:
        v1.i256 = add v0 2.i256;
        return v1;
}
"#;
        let bytes: BTreeMap<String, u64> =
            [("a".into(), 10), ("b".into(), 12), ("c".into(), 10)].into();
        let census = sonatina_function_facets(ir, &bytes).unwrap();
        assert!(census[0].classes.is_empty(), "{:?}", census[0]);
        assert_eq!(census[1].classes.len(), 1);
        assert_eq!(census[1].classes[0].functions.len(), 2);
        assert_eq!(census[1].upper_bound_saving, 10);
        assert_eq!(census[2].classes[0].functions.len(), 3);
    }
}
