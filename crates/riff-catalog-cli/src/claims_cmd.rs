//! `claim` and `attest`: witnessed assertions over corpus rows.
//!
//! Claims are inputs, not discoveries (invariant I11): nothing here checks
//! that a witness makes sense — `bucket --claims` will merge whatever you
//! assert, attributably. `diff` on a claimed-equal pair will still show the
//! structural disagreement; that contradiction surfacing is the point.

use anyhow::{Result, bail};
use riff_catalog_claims::{Attestation, Claim, Witness};

use crate::corpus::{Corpus, DigestRow, Record, matches_selector};
use crate::facet::{facet_address_of, parse_facet_dimensions};

pub struct AssertArgs {
    pub unit: String,
    pub mode: String,
    pub facet: String,
    pub witness_kind: String,
    pub witness: Vec<(String, String)>,
    pub note: Option<String>,
}

fn resolve_one<'r>(rows: &'r [DigestRow], selector: &str, what: &str) -> Result<&'r DigestRow> {
    let matches: Vec<&DigestRow> = rows
        .iter()
        .filter(|row| matches_selector(row, selector))
        .collect();
    match matches.len() {
        0 => bail!("{what} selector `{selector}` matches nothing"),
        1 => Ok(matches[0]),
        n => {
            let sample: Vec<String> = matches
                .iter()
                .take(5)
                .map(|row| format!("{} ({})", row.name, row.owner))
                .collect();
            bail!(
                "{what} selector `{selector}` is ambiguous ({n} matches): {}",
                sample.join("; ")
            )
        }
    }
}

pub fn claim_add(corpus: &Corpus, args: &AssertArgs, left: &str, right: &str) -> Result<()> {
    let rows = corpus.digest_rows(&args.unit, &args.mode)?;
    let dimensions = parse_facet_dimensions(&args.facet)?;
    let left_row = resolve_one(&rows, left, "--left")?;
    let right_row = resolve_one(&rows, right, "--right")?;

    let left_address = facet_address_of(left_row, &dimensions)?;
    let right_address = facet_address_of(right_row, &dimensions)?;
    let facet = left_address.facet.clone();

    let witness = Witness::new(args.witness_kind.as_str(), args.witness.clone())?;
    let claim = Claim::new(
        facet,
        left_address,
        right_address,
        witness,
        args.note.clone(),
    )?;
    let claim_id = claim.claim_id();

    corpus.append(
        "claims",
        &[Record::Claim {
            claim,
            left_display: format!("{} ({})", left_row.name, left_row.owner),
            right_display: format!("{} ({})", right_row.name, right_row.owner),
        }],
    )?;
    println!(
        "claimed: {} ≅ {} at facet {} ({}) — claim {}",
        left_row.name,
        right_row.name,
        args.facet,
        args.mode,
        claim_id.display_short()
    );
    Ok(())
}

pub fn attest_add(corpus: &Corpus, args: &AssertArgs, subject: &str, property: &str) -> Result<()> {
    let rows = corpus.digest_rows(&args.unit, &args.mode)?;
    let dimensions = parse_facet_dimensions(&args.facet)?;
    let subject_row = resolve_one(&rows, subject, "--subject")?;
    let address = facet_address_of(subject_row, &dimensions)?;

    let witness = Witness::new(args.witness_kind.as_str(), args.witness.clone())?;
    let attestation = Attestation::new(address, property, witness, args.note.clone())?;
    let attestation_id = attestation.attestation_id();

    corpus.append(
        "attestations",
        &[Record::Attestation {
            attestation,
            subject_display: format!("{} ({})", subject_row.name, subject_row.owner),
        }],
    )?;
    println!(
        "attested: {} carries `{property}` at facet {} ({}) — attestation {}",
        subject_row.name,
        args.facet,
        args.mode,
        attestation_id.display_short()
    );
    Ok(())
}

pub fn list(corpus: &Corpus, json: bool) -> Result<()> {
    // Only claim/attestation records are listed; skip the large graph payloads.
    let records = corpus.load_non_graph_records()?;
    let mut any = false;
    for record in &records {
        match record {
            Record::Claim {
                claim,
                left_display,
                right_display,
            } => {
                any = true;
                if json {
                    println!("{}", serde_json::to_string(record)?);
                } else {
                    println!(
                        "claim {}: {} ≅ {} [witness: {}{}]{}",
                        claim.claim_id().display_short(),
                        left_display,
                        right_display,
                        claim.witness.kind,
                        if claim.witness.payload.is_empty() {
                            String::new()
                        } else {
                            format!(
                                " {}",
                                claim
                                    .witness
                                    .payload
                                    .iter()
                                    .map(|(k, v)| format!("{k}={v}"))
                                    .collect::<Vec<_>>()
                                    .join(",")
                            )
                        },
                        claim
                            .note
                            .as_deref()
                            .map(|note| format!(" — {note}"))
                            .unwrap_or_default()
                    );
                }
            }
            Record::Attestation {
                attestation,
                subject_display,
            } => {
                any = true;
                if json {
                    println!("{}", serde_json::to_string(record)?);
                } else {
                    println!(
                        "attestation {}: {} carries `{}` [witness: {}]{}",
                        attestation.attestation_id().display_short(),
                        subject_display,
                        attestation.property,
                        attestation.witness.kind,
                        attestation
                            .note
                            .as_deref()
                            .map(|note| format!(" — {note}"))
                            .unwrap_or_default()
                    );
                }
            }
            _ => {}
        }
    }
    if !any && !json {
        println!("no claims or attestations recorded");
    }
    Ok(())
}
