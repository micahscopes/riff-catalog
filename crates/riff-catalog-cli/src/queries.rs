//! `bucket`, `overlap`, `diff` — the corpus query commands.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use riff_catalog_claims::ClaimSet;
use riff_catalog_core::{Digest, Dimension};

use crate::corpus::{Corpus, DigestRow, matches_selector};
use crate::facet::{address_of, facet_for_row, parse_facet_dimensions};
use crate::table::Table;

pub struct QueryArgs {
    pub unit: String,
    pub mode: String,
    pub facet: String,
    pub use_claims: bool,
    pub require: Option<String>,
}

fn class_key(
    row: &DigestRow,
    dimensions: &BTreeSet<Dimension>,
    closure: Option<&mut riff_catalog_claims::FacetClosure>,
) -> Result<Digest> {
    let address = address_of(row, dimensions)?;
    Ok(match closure {
        Some(closure) => closure.representative(&address),
        None => address,
    })
}

pub fn bucket(
    corpus: &Corpus,
    args: &QueryArgs,
    min_size: usize,
    top: usize,
    json: bool,
) -> Result<()> {
    let rows = corpus.digest_rows(&args.unit, &args.mode)?;
    if rows.is_empty() {
        bail!(
            "no digest rows for unit `{}` mode `{}` — ingest something first",
            args.unit,
            args.mode
        );
    }
    let dimensions = parse_facet_dimensions(&args.facet)?;

    let mut closure = if args.use_claims {
        let facet = facet_for_row(&rows[0], &dimensions)?;
        let mut set = ClaimSet::new();
        for claim in corpus.claims()? {
            set.insert(claim);
        }
        Some(set.closure_for_facet(&facet))
    } else {
        None
    };

    // property gate (attestations): drop rows whose address lacks it
    let permitted: Option<BTreeSet<Digest>> = match &args.require {
        Some(property) => {
            let mut set = riff_catalog_claims::AttestationSet::new();
            for attestation in corpus.attestations()? {
                set.insert(attestation);
            }
            Some(set.subjects_with(property))
        }
        None => None,
    };

    let mut classes: BTreeMap<Digest, Vec<&DigestRow>> = BTreeMap::new();
    let mut excluded = 0usize;
    for row in &rows {
        let address = address_of(row, &dimensions)?;
        if let Some(permitted) = &permitted {
            if !permitted.contains(&address) {
                excluded += 1;
                continue;
            }
        }
        let key = match closure.as_mut() {
            Some(closure) => closure.representative(&address),
            None => address,
        };
        classes.entry(key).or_default().push(row);
    }

    let mut sorted: Vec<(&Digest, &Vec<&DigestRow>)> = classes.iter().collect();
    sorted.sort_by_key(|(_, members)| std::cmp::Reverse(members.len()));

    let mut table = Table::new(&["class", "size", "artifacts", "members"]);
    for (digest, members) in sorted.iter().filter(|(_, m)| m.len() >= min_size).take(top) {
        let artifacts: BTreeSet<&str> =
            members.iter().map(|row| row.artifact_id.as_str()).collect();
        let mut names: Vec<&str> = members.iter().map(|row| row.name.as_str()).collect();
        names.sort();
        names.dedup();
        let sample = names.iter().take(4).cloned().collect::<Vec<_>>().join(", ");
        let suffix = if names.len() > 4 {
            format!(" (+{})", names.len() - 4)
        } else {
            String::new()
        };
        table.row(vec![
            digest.display_short(),
            members.len().to_string(),
            artifacts.len().to_string(),
            format!("{sample}{suffix}"),
        ]);
    }

    let total = rows.len();
    let dedup_classes = classes.len();
    if json {
        println!("{}", table.to_json());
    } else {
        table.print();
        println!(
            "\n{total} {unit} graphs -> {dedup_classes} classes at facet {facet} ({mode}); \
             dedup {percent:.1}%{gate}",
            unit = args.unit,
            facet = args.facet,
            mode = args.mode,
            percent = 100.0 * (1.0 - dedup_classes as f64 / total as f64),
            gate = match (&args.require, excluded) {
                (Some(property), n) => format!("; {n} rows excluded (lack `{property}`)"),
                _ => String::new(),
            },
        );
    }
    Ok(())
}

pub fn overlap(
    corpus: &Corpus,
    args: &QueryArgs,
    left: &str,
    right: &str,
    json: bool,
) -> Result<()> {
    let rows = corpus.digest_rows(&args.unit, &args.mode)?;
    let dimensions = parse_facet_dimensions(&args.facet)?;

    let mut closure = if args.use_claims {
        let Some(first) = rows.first() else {
            bail!("empty corpus");
        };
        let facet = facet_for_row(first, &dimensions)?;
        let mut set = ClaimSet::new();
        for claim in corpus.claims()? {
            set.insert(claim);
        }
        Some(set.closure_for_facet(&facet))
    } else {
        None
    };

    let mut left_classes: BTreeMap<Digest, Vec<&DigestRow>> = BTreeMap::new();
    let mut right_classes: BTreeMap<Digest, Vec<&DigestRow>> = BTreeMap::new();
    for row in &rows {
        let on_left = matches_selector(row, left);
        let on_right = matches_selector(row, right);
        if !on_left && !on_right {
            continue;
        }
        let key = class_key(row, &dimensions, closure.as_mut())?;
        if on_left {
            left_classes.entry(key).or_default().push(row);
        }
        if on_right {
            right_classes.entry(key).or_default().push(row);
        }
    }

    let left_keys: BTreeSet<&Digest> = left_classes.keys().collect();
    let right_keys: BTreeSet<&Digest> = right_classes.keys().collect();
    let shared: Vec<&&Digest> = left_keys.intersection(&right_keys).collect();

    let mut table = Table::new(&["class", &format!("A: {left}"), &format!("B: {right}")]);
    for digest in &shared {
        let names = |classes: &BTreeMap<Digest, Vec<&DigestRow>>| {
            let mut names: Vec<&str> = classes[**digest].iter().map(|r| r.name.as_str()).collect();
            names.sort();
            names.dedup();
            names.join(", ")
        };
        table.row(vec![
            digest.display_short(),
            names(&left_classes),
            names(&right_classes),
        ]);
    }

    if json {
        println!("{}", table.to_json());
    } else {
        table.print();
        let union = left_keys.union(&right_keys).count();
        println!(
            "\nA: {} classes, B: {} classes, shared: {}, Jaccard {:.3}",
            left_keys.len(),
            right_keys.len(),
            shared.len(),
            if union == 0 {
                0.0
            } else {
                shared.len() as f64 / union as f64
            }
        );
    }
    Ok(())
}

pub fn diff(
    corpus: &Corpus,
    unit: &str,
    left: &str,
    right: &str,
    name: Option<&str>,
    json: bool,
) -> Result<()> {
    // Per-dimension survival matrix across both view modes.
    let mut table = Table::new(&["name", "mode", "dimension", "A", "B", "same"]);
    for mode in ["identity", "shape"] {
        let rows = corpus.digest_rows(unit, mode)?;
        // Name is a substring filter, and the comparison key strips solc's
        // volatile `_<id>` suffixes so `fun_transfer_123` (unoptimized IR)
        // lines up with `fun_transfer` (optimized).
        let normalized = |raw: &str| -> String {
            match raw.rsplit_once('_') {
                Some((stem, suffix))
                    if !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()) =>
                {
                    stem.to_string()
                }
                _ => raw.to_string(),
            }
        };
        let pick = |selector: &str| -> BTreeMap<String, &DigestRow> {
            rows.iter()
                .filter(|row| matches_selector(row, selector))
                .filter(|row| name.is_none_or(|n| row.name.contains(n)))
                .map(|row| (normalized(&row.name), row))
                .collect()
        };
        let left_rows = pick(left);
        let right_rows = pick(right);
        let names: BTreeSet<&String> = left_rows.keys().chain(right_rows.keys()).collect();
        for unit_name in names {
            match (left_rows.get(unit_name), right_rows.get(unit_name)) {
                (Some(left_row), Some(right_row)) => {
                    for dimension in Dimension::ALL {
                        let a = left_row.digests.get(&dimension);
                        let b = right_row.digests.get(&dimension);
                        let same = a == b;
                        table.row(vec![
                            unit_name.clone(),
                            mode.to_string(),
                            dimension.as_str().to_string(),
                            a.map(|d| d.display_short()).unwrap_or_default(),
                            b.map(|d| d.display_short()).unwrap_or_default(),
                            if same { "=".into() } else { "DIFF".into() },
                        ]);
                    }
                }
                (Some(_), None) => table.row(vec![
                    unit_name.clone(),
                    mode.to_string(),
                    "-".into(),
                    "present".into(),
                    "absent".into(),
                    "DIFF".into(),
                ]),
                (None, Some(_)) => table.row(vec![
                    unit_name.clone(),
                    mode.to_string(),
                    "-".into(),
                    "absent".into(),
                    "present".into(),
                    "DIFF".into(),
                ]),
                (None, None) => unreachable!("name came from one of the maps"),
            }
        }
    }
    if table.is_empty() {
        bail!("no matching rows for `{left}` / `{right}` at unit `{unit}`");
    }
    if json {
        println!("{}", table.to_json());
    } else {
        table.print();
    }
    Ok(())
}
