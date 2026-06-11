//! `bucket`, `overlap`, `diff`, `root` — the corpus query commands.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, bail};
use riff_catalog_claims::{ClaimSet, FacetClosure};
use riff_catalog_core::{Digest, Dimension, Facet};

use crate::corpus::{Corpus, DigestRow, matches_selector};
use crate::facet::{address_of, facet_for_row, parse_facet_dimensions};
use crate::table::Table;

pub struct QueryArgs {
    pub unit: String,
    pub mode: String,
    pub facet: String,
    pub use_claims: bool,
    /// Assumption roots (hex) the querier accepts: conditional claims merge
    /// only when their root is listed here. Non-empty implies `use_claims`.
    pub assume: Vec<String>,
    pub require: Option<String>,
}

impl QueryArgs {
    fn wants_claims(&self) -> bool {
        self.use_claims || !self.assume.is_empty()
    }
}

/// Build the claims closure for one facet, honoring `--assume` roots.
fn claims_closure(corpus: &Corpus, facet: &Facet, assume: &[String]) -> Result<FacetClosure> {
    let assumed_roots = assume
        .iter()
        .map(|hex| Ok(Digest::from_hex(hex)?))
        .collect::<Result<BTreeSet<Digest>>>()?;
    let mut set = ClaimSet::new();
    for claim in corpus.claims()? {
        set.insert(claim);
    }
    Ok(set.closure_for_facet_assuming(facet, &assumed_roots))
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

    let mut closure = if args.wants_claims() {
        let facet = facet_for_row(&rows[0], &dimensions)?;
        Some(claims_closure(corpus, &facet, &args.assume)?)
    } else {
        None
    };

    let permitted = permitted_addresses(corpus, args)?;

    let mut classes: BTreeMap<Digest, Vec<&DigestRow>> = BTreeMap::new();
    let mut excluded = 0usize;
    let mut considered = 0usize;
    for row in &rows {
        let address = address_of(row, &dimensions)?;
        if let Some(permitted) = &permitted {
            if !permitted.contains(&address) {
                excluded += 1;
                continue;
            }
        }
        considered += 1;
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

    // Denominator counts only gated-in rows: one attested row in a large
    // corpus is 0% dedup, not ~100% (external review pass 2, P2).
    let dedup_classes = classes.len();
    let percent = if considered == 0 {
        0.0
    } else {
        100.0 * (1.0 - dedup_classes as f64 / considered as f64)
    };
    if json {
        println!("{}", table.to_json());
    } else {
        table.print();
        println!(
            "\n{considered} {unit} graphs -> {dedup_classes} classes at facet {facet} ({mode}); \
             dedup {percent:.1}%{gate}",
            unit = args.unit,
            facet = args.facet,
            mode = args.mode,
            gate = match (&args.require, excluded) {
                (Some(property), n) => format!("; {n} rows excluded (lack `{property}`)"),
                _ => String::new(),
            },
        );
    }
    Ok(())
}

/// The attestation gate: addresses carrying `--require`'s property. Shared
/// by bucket AND overlap — overlap silently bypassing the gate was external
/// review pass 2's P1.
fn permitted_addresses(corpus: &Corpus, args: &QueryArgs) -> Result<Option<BTreeSet<Digest>>> {
    match &args.require {
        Some(property) => {
            let mut set = riff_catalog_claims::AttestationSet::new();
            for attestation in corpus.attestations()? {
                set.insert(attestation);
            }
            Ok(Some(set.subjects_with(property)))
        }
        None => Ok(None),
    }
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

    let mut closure = if args.wants_claims() {
        let Some(first) = rows.first() else {
            bail!("empty corpus");
        };
        let facet = facet_for_row(first, &dimensions)?;
        Some(claims_closure(corpus, &facet, &args.assume)?)
    } else {
        None
    };

    let permitted = permitted_addresses(corpus, args)?;

    let mut left_classes: BTreeMap<Digest, Vec<&DigestRow>> = BTreeMap::new();
    let mut right_classes: BTreeMap<Digest, Vec<&DigestRow>> = BTreeMap::new();
    let mut excluded = 0usize;
    for row in &rows {
        let on_left = matches_selector(row, left);
        let on_right = matches_selector(row, right);
        if !on_left && !on_right {
            continue;
        }
        if let Some(permitted) = &permitted {
            if !permitted.contains(&address_of(row, &dimensions)?) {
                excluded += 1;
                continue;
            }
        }
        let key = class_key(row, &dimensions, closure.as_mut())?;
        if on_left {
            left_classes.entry(key).or_default().push(row);
        }
        if on_right {
            right_classes.entry(key).or_default().push(row);
        }
    }
    if excluded > 0 {
        eprintln!(
            "note: {excluded} rows excluded by --require {}",
            args.require.as_deref().unwrap_or_default()
        );
    }

    let left_keys: BTreeSet<&Digest> = left_classes.keys().collect();
    let right_keys: BTreeSet<&Digest> = right_classes.keys().collect();
    let shared: Vec<&&Digest> = left_keys.intersection(&right_keys).collect();

    let mut table = Table::new(&["class", &format!("A: {left}"), &format!("B: {right}")]);
    for digest in &shared {
        // Projector-friendly: cap member lists, count the rest.
        let names = |classes: &BTreeMap<Digest, Vec<&DigestRow>>| {
            let mut names: Vec<&str> = classes[**digest].iter().map(|r| r.name.as_str()).collect();
            names.sort();
            names.dedup();
            let total = names.len();
            let mut joined = String::new();
            for (index, name) in names.iter().enumerate() {
                let next_len = joined.len() + name.len() + 2;
                if index > 0 && next_len > 56 {
                    joined.push_str(&format!(" (+{})", total - index));
                    break;
                }
                if index > 0 {
                    joined.push_str(", ");
                }
                joined.push_str(name);
            }
            joined
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
        // Collisions after normalization (helpers in multiple objects,
        // duplicate ingests) stay VISIBLE as distinct keyed rows instead of
        // silently keeping the last one (external review pass 2, P2).
        let pick = |selector: &str| -> BTreeMap<String, &DigestRow> {
            let mut grouped: BTreeMap<String, Vec<&DigestRow>> = BTreeMap::new();
            for row in rows
                .iter()
                .filter(|row| matches_selector(row, selector))
                .filter(|row| name.is_none_or(|n| row.name.contains(n)))
            {
                grouped.entry(normalized(&row.name)).or_default().push(row);
            }
            let mut picked = BTreeMap::new();
            for (norm, group) in grouped {
                if let [only] = group.as_slice() {
                    picked.insert(norm, *only);
                } else {
                    for (index, row) in group.into_iter().enumerate() {
                        let key = if index == 0 {
                            row.name.clone()
                        } else {
                            format!("{}#{}", row.name, index + 1)
                        };
                        picked.insert(key, row);
                    }
                }
            }
            picked
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

/// The corpus root (invariant I17): a canonical, order-independent merkle
/// root over the corpus's distinct facet addresses. The leaf for each digest
/// row is its address at the row's full available dimension set — owners,
/// names, units, and ingestion order never enter (identity over logical
/// structure); twin rows collapse to one leaf. `--check` recomputes and
/// fails loudly on mismatch, in the conformance spirit: a corpus that
/// doesn't match its published root is a finding, not a footnote.
pub fn root(
    corpus: &Corpus,
    unit: Option<&str>,
    mode: Option<&str>,
    check: Option<&str>,
    json: bool,
) -> Result<()> {
    let rows = corpus.digest_rows_filtered(unit, mode)?;
    let mut addresses: BTreeSet<Digest> = BTreeSet::new();
    for row in &rows {
        let dimensions: BTreeSet<Dimension> = row.digests.keys().copied().collect();
        addresses.insert(address_of(row, &dimensions)?);
    }
    let leaf_count = addresses.len();
    let root = riff_catalog_core::set_root(addresses);

    if json {
        println!(
            "{}",
            serde_json::json!({
                "root": root.to_hex(),
                "addresses": leaf_count,
                "rows": rows.len(),
                "unit": unit,
                "mode": mode,
            })
        );
    } else {
        println!(
            "corpus root {} ({} distinct addresses over {} rows; unit: {}, mode: {})",
            root.to_hex(),
            leaf_count,
            rows.len(),
            unit.unwrap_or("all"),
            mode.unwrap_or("all"),
        );
    }

    if let Some(expected) = check {
        let expected = Digest::from_hex(expected)?;
        if expected != root {
            bail!(
                "ROOT MISMATCH: corpus is {}, expected {}",
                root.to_hex(),
                expected.to_hex()
            );
        }
        if !json {
            println!("root check OK");
        }
    }
    Ok(())
}
