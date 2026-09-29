//! Per-function byte comparison between a Fe build and solc builds, over a
//! hand-made pairing. Both sides count bytes by source function: Fe by
//! primary source body (`fe-trace-stages` expansion), solc by innermost
//! source function (`solc-functions`). Inlined code counts toward the
//! function it came from on both sides.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::fe_stages::FeStagesReport;
use crate::solc_functions::SolcFunctions;

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
    pub fe_bytes: u64,
    pub solc_bytes: BTreeMap<String, u64>,
    /// Fe's largest trace category and its bytes.
    pub fe_top_category: Option<(String, u64)>,
}

pub fn compare_functions(
    fe: &FeStagesReport,
    solc: &BTreeMap<String, SolcFunctions>,
    pairs: &[FunctionPair],
) -> Vec<FunctionComparison> {
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
    let mut out: Vec<FunctionComparison> = pairs
        .iter()
        .map(|p| {
            let mut cats: BTreeMap<String, u64> = BTreeMap::new();
            let mut fe_bytes = 0;
            for name in &p.fe {
                if let Some(b) = bodies.get(name.as_str()) {
                    fe_bytes += b.bytes_primary;
                    for (c, v) in &b.primary_by_category {
                        *cats.entry(c.clone()).or_default() += v;
                    }
                }
            }
            let solc_bytes = owners
                .iter()
                .map(|(build, table)| {
                    let names = p.solc.get(*build).map(Vec::as_slice).unwrap_or(&[]);
                    let bytes = names
                        .iter()
                        .map(|n| table.get(n.as_str()).copied().unwrap_or(0))
                        .sum();
                    (build.to_string(), bytes)
                })
                .collect();
            FunctionComparison {
                label: p.label.clone(),
                confidence: p.confidence.clone(),
                fe_bytes,
                solc_bytes,
                fe_top_category: cats.into_iter().max_by_key(|(_, v)| *v),
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.fe_bytes
            .cmp(&a.fe_bytes)
            .then_with(|| a.label.cmp(&b.label))
    });
    out
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
    let fe_paired: u64 = rows.iter().map(|r| r.fe_bytes).sum();
    FunctionComparison {
        label: "(everything outside the pairs: unpaired functions, dispatch, no-source and generated code, data)".into(),
        confidence: "-".into(),
        fe_bytes: fe_total.saturating_sub(fe_paired),
        solc_bytes: solc
            .iter()
            .map(|(b, f)| {
                let paired: u64 = rows.iter().map(|r| r.solc_bytes.get(b).copied().unwrap_or(0)).sum();
                (b.clone(), (f.runtime_bytes as u64).saturating_sub(paired))
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
    let mut totals: BTreeMap<&str, u64> = BTreeMap::new();
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
