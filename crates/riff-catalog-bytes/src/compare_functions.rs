//! Per-function byte comparison between a Fe build and solc builds, over a
//! hand-made pairing. Both sides count bytes by source function: Fe by
//! primary source body (`fe-trace-stages` expansion), solc by innermost
//! source function (`solc-functions`). Inlined code counts toward the
//! function it came from on both sides.
//!
//! The pairing is checked against both reports: every Fe body and solc
//! function it names must be in them, and every build it names must be
//! given, so a typo is an error rather than a row of zeros.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::fe_stages::FeStagesReport;
use crate::solc_functions::SolcFunctions;

pub const FUNCTION_COMPARISON_SCHEMA: &str = "riffcat-function-comparison/1";

/// The `compare-functions` report: the solc builds in column order, then one
/// row per pair (and the residual row when the Fe total was given).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionComparisonReport {
    pub schema: String,
    pub builds: Vec<String>,
    pub rows: Vec<FunctionComparison>,
}

/// One pairing: Fe source bodies and solc functions that implement the same
/// thing. `confidence` is the pairer's (high, medium, low).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionPair {
    pub label: String,
    pub fe: Vec<String>,
    /// solc build name to function names.
    pub solc: BTreeMap<String, Vec<String>>,
    pub confidence: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionComparison {
    pub label: String,
    pub confidence: String,
    /// Signed, because the residual row is negative when pairs overlap.
    pub fe_bytes: i64,
    pub solc_bytes: BTreeMap<String, i64>,
    /// Fe's largest trace category and its bytes.
    pub fe_top_category: Option<(String, u64)>,
}

pub fn compare_functions(
    fe: &FeStagesReport,
    solc: &BTreeMap<String, SolcFunctions>,
    pairs: &[FunctionPair],
) -> Result<Vec<FunctionComparison>> {
    let bodies: BTreeMap<&str, _> = fe
        .expansion_by_body
        .iter()
        .map(|b| (b.body.as_str(), b))
        .collect();
    let owners: BTreeMap<&str, BTreeMap<&str, u64>> = solc
        .iter()
        .map(|(build, f)| {
            (
                build.as_str(),
                f.by_owner.iter().map(|(n, b)| (n.as_str(), *b)).collect(),
            )
        })
        .collect();
    let mut out = Vec::new();
    for p in pairs {
        if let Some(build) = p.solc.keys().find(|b| !owners.contains_key(b.as_str())) {
            bail!(
                "pair `{}` names solc build `{build}`, which was not given",
                p.label
            );
        }
        let mut cats: BTreeMap<String, u64> = BTreeMap::new();
        let mut fe_bytes = 0u64;
        for name in &p.fe {
            let Some(b) = bodies.get(name.as_str()) else {
                bail!(
                    "pair `{}` names Fe body `{name}`, which the stage report does not list (no emitted bytes reach it)",
                    p.label
                );
            };
            fe_bytes += b.bytes_primary;
            for (c, v) in &b.primary_by_category {
                *cats.entry(c.clone()).or_default() += v;
            }
        }
        let mut solc_bytes = BTreeMap::new();
        for (build, table) in &owners {
            let names = p.solc.get(*build).map(Vec::as_slice).unwrap_or(&[]);
            let mut bytes = 0u64;
            for n in names {
                let Some(b) = table.get(n.as_str()) else {
                    bail!(
                        "pair `{}` names `{n}`, which solc build `{build}` does not list",
                        p.label
                    );
                };
                bytes += b;
            }
            solc_bytes.insert(build.to_string(), bytes as i64);
        }
        out.push(FunctionComparison {
            label: p.label.clone(),
            confidence: p.confidence.clone(),
            fe_bytes: fe_bytes as i64,
            solc_bytes,
            fe_top_category: cats.into_iter().max_by_key(|(_, v)| *v),
        });
    }
    out.sort_by(|a, b| {
        b.fe_bytes
            .cmp(&a.fe_bytes)
            .then_with(|| a.label.cmp(&b.label))
    });
    Ok(out)
}

/// Rows for the bytes no pair holds, so each column adds up to its whole
/// artifact: Fe bytes outside the paired bodies, and each solc build's bytes
/// outside its paired functions. Several Fe bodies or solc functions can be
/// claimed by two pairs; those count once per pair, so the residual can be
/// negative when pairs overlap.
pub fn residual_row(
    rows: &[FunctionComparison],
    fe_total: u64,
    solc: &BTreeMap<String, SolcFunctions>,
) -> FunctionComparison {
    let fe_paired: i64 = rows.iter().map(|r| r.fe_bytes).sum();
    FunctionComparison {
        label: "(everything outside the pairs: unpaired functions, dispatch, no-source and generated code, data)".into(),
        confidence: "-".into(),
        fe_bytes: fe_total as i64 - fe_paired,
        solc_bytes: solc
            .iter()
            .map(|(b, f)| {
                let paired: i64 = rows
                    .iter()
                    .map(|r| r.solc_bytes.get(b).copied().unwrap_or(0))
                    .sum();
                (b.clone(), f.runtime_bytes as i64 - paired)
            })
            .collect(),
        fe_top_category: None,
    }
}

pub fn render_function_comparison(rows: &[FunctionComparison], builds: &[String]) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = write!(out, "{:>7}", "fe");
    for b in builds {
        let _ = write!(out, " {:>9}", b.chars().take(9).collect::<String>());
    }
    let _ = writeln!(
        out,
        "  fe/{}  conf    top fe category  function",
        builds.last().map_or("", |b| b)
    );
    let mut totals: BTreeMap<&str, i64> = BTreeMap::new();
    let mut fe_total = 0;
    for r in rows {
        fe_total += r.fe_bytes;
        let _ = write!(out, "{:>7}", r.fe_bytes);
        for b in builds {
            let v = r.solc_bytes.get(b).copied().unwrap_or(0);
            *totals.entry(b.as_str()).or_default() += v;
            let _ = write!(out, " {v:>9}");
        }
        let last = builds
            .last()
            .and_then(|b| r.solc_bytes.get(b))
            .copied()
            .unwrap_or(0);
        let ratio = if last > 0 {
            format!("{:>6.2}", r.fe_bytes as f64 / last as f64)
        } else {
            "     -".into()
        };
        let cat = r
            .fe_top_category
            .as_ref()
            .map_or("-".to_string(), |(c, v)| format!("{c} {v}"));
        let _ = writeln!(
            out,
            "  {ratio}  {:<6}  {cat:<15}  {}",
            r.confidence, r.label
        );
    }
    let _ = write!(out, "{fe_total:>7}");
    for b in builds {
        let _ = write!(out, " {:>9}", totals.get(b.as_str()).copied().unwrap_or(0));
    }
    let _ = writeln!(out, "  (paired totals)");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fe_stages::BodyExpansion;

    fn fe(bodies: &[(&str, u64)]) -> FeStagesReport {
        FeStagesReport {
            schema: crate::fe_stages::FE_STAGES_SCHEMA.into(),
            contract: "C".into(),
            stage_graph_nodes: 0,
            selections: Vec::new(),
            expansion_by_body: bodies
                .iter()
                .map(|(b, n)| BodyExpansion {
                    body: b.to_string(),
                    bytes_primary: *n,
                    ..BodyExpansion::default()
                })
                .collect(),
            chain_class_count: 0,
            chain_classes: Vec::new(),
            top_constructs: Vec::new(),
            category_by_function: Vec::new(),
        }
    }

    fn solc(owners: &[(&str, u64)], total: usize) -> BTreeMap<String, SolcFunctions> {
        let table: SolcFunctions = serde_json::from_value(serde_json::json!({
            "schema": crate::solc_functions::SOLC_FUNCTIONS_SCHEMA,
            "source": "a.sol", "contract": "C", "runtime_bytes": total, "code_end": total,
            "by_owner": owners.iter().map(|(n, b)| (n.to_string(), *b)).collect::<Vec<_>>(),
        }))
        .unwrap();
        [("s".to_string(), table)].into()
    }

    fn pairs(value: serde_json::Value) -> Vec<FunctionPair> {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn unknown_names_in_the_pairing_are_errors() {
        let fe = fe(&[("decode_order", 100)]);
        let solc = solc(&[("C.f", 50)], 50);
        for bad in [
            serde_json::json!([{"label": "typo", "fe": ["decode_ordr"], "solc": {"s": ["C.f"]}, "confidence": "high"}]),
            serde_json::json!([{"label": "typo", "fe": ["decode_order"], "solc": {"s": ["C.ff"]}, "confidence": "high"}]),
            serde_json::json!([{"label": "build", "fe": ["decode_order"], "solc": {"t": ["C.f"]}, "confidence": "high"}]),
        ] {
            let result = compare_functions(&fe, &solc, &pairs(bad.clone()));
            assert!(format!("{result:?}").starts_with("Err"), "{bad} accepted");
        }
        let ok = pairs(serde_json::json!([
            {"label": "ok", "fe": ["decode_order"], "solc": {"s": ["C.f"]}, "confidence": "high"}
        ]));
        assert!(format!("{:?}", compare_functions(&fe, &solc, &ok)).starts_with("Ok"));
    }

    #[test]
    fn the_residual_can_be_negative_so_columns_add_up() {
        let fe = fe(&[("a", 60)]);
        let solc = solc(&[("C.f", 60)], 100);
        // Two pairs claim the same body and function: 120 of 100 bytes.
        let pairs = pairs(serde_json::json!([
            {"label": "p1", "fe": ["a"], "solc": {"s": ["C.f"]}, "confidence": "high"},
            {"label": "p2", "fe": ["a"], "solc": {"s": ["C.f"]}, "confidence": "high"}
        ]));
        let rows = compare_functions(&fe, &solc, &pairs).unwrap();
        let residual = residual_row(&rows, 100, &solc);
        let fe_sum: i64 = rows.iter().map(|r| r.fe_bytes).sum::<i64>() + residual.fe_bytes;
        let solc_sum: i64 =
            rows.iter().map(|r| r.solc_bytes["s"]).sum::<i64>() + residual.solc_bytes["s"];
        assert_eq!((fe_sum, solc_sum), (100, 100));
        assert_eq!(residual.fe_bytes, -20);
    }
}
