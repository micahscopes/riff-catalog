//! The one line scanner every reader of a trace bundle uses.
//!
//! Each non-blank line is parsed as JSON; its `record` and fact `type` are
//! read as fields, never matched as text, so key order and whitespace do
//! not matter. A `metadata` record must carry an integer `schema_version`
//! in [`SUPPORTED_TRACE_SCHEMA_VERSIONS`]; a missing, non-integer or unknown
//! version is an error, because an unknown schema may change the meaning of
//! known fact kinds. Records other than `metadata` and `fact` are skipped.

use std::borrow::Cow;
use std::io::BufRead;
use std::ops::RangeInclusive;

use serde::Deserialize;

/// Fe trace-bundle schema versions this crate reads. Version 2 only added the
/// `attribution_gap` fact kind; the kinds read here are unchanged.
pub const SUPPORTED_TRACE_SCHEMA_VERSIONS: RangeInclusive<u64> = 1..=2;

/// Why a trace bundle could not be scanned.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("reading the trace bundle: {0}")]
    Io(#[from] std::io::Error),
    #[error("trace bundle line {line}: invalid JSON: {source}")]
    Json {
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error("trace bundle line {line}: {message}")]
    Line { line: usize, message: String },
    #[error(
        "trace bundle line {line}: unsupported trace schema version {found} (supported: {} to {})",
        SUPPORTED_TRACE_SCHEMA_VERSIONS.start(),
        SUPPORTED_TRACE_SCHEMA_VERSIONS.end()
    )]
    UnsupportedSchema { line: usize, found: u64 },
    #[error("trace bundle has no metadata record, so its schema version is unknown")]
    NoMetadata,
}

/// What the metadata record said.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TraceMetadata {
    pub schema_version: Option<u64>,
    pub input_path: Option<String>,
}

#[derive(Deserialize)]
struct Head<'a> {
    #[serde(borrow)]
    record: Option<Cow<'a, str>>,
    #[serde(rename = "type", borrow, default)]
    kind: Option<Cow<'a, str>>,
    #[serde(default)]
    schema_version: Option<serde_json::Value>,
    #[serde(borrow, default)]
    input_path: Option<Cow<'a, str>>,
}

/// Scan a bundle. For every `fact` whose `type` passes `wanted`, `on_fact`
/// gets the 1-based line number, the fact type and the line's text to parse
/// as it needs. With `require_metadata`, a bundle without a metadata record
/// is an error.
pub fn scan_trace<E: From<ScanError>>(
    reader: impl BufRead,
    require_metadata: bool,
    wanted: impl Fn(&str) -> bool,
    mut on_fact: impl FnMut(usize, &str, &str) -> Result<(), E>,
) -> Result<TraceMetadata, E> {
    let mut metadata = None;
    for (index, raw) in reader.lines().enumerate() {
        let line = index + 1;
        let raw = raw.map_err(ScanError::from)?;
        let text = raw.trim();
        if text.is_empty() {
            continue;
        }
        let head: Head =
            serde_json::from_str(text).map_err(|source| ScanError::Json { line, source })?;
        let Some(record) = head.record else {
            return Err(ScanError::Line {
                line,
                message: "no `record` field".into(),
            }
            .into());
        };
        match record.as_ref() {
            "metadata" => {
                let found = match head.schema_version {
                    None => {
                        return Err(ScanError::Line {
                            line,
                            message: "metadata record has no `schema_version`".into(),
                        }
                        .into());
                    }
                    Some(v) => v.as_u64().ok_or_else(|| ScanError::Line {
                        line,
                        message: format!("`schema_version` {v} is not a non-negative integer"),
                    })?,
                };
                if !SUPPORTED_TRACE_SCHEMA_VERSIONS.contains(&found) {
                    return Err(ScanError::UnsupportedSchema { line, found }.into());
                }
                metadata = Some(TraceMetadata {
                    schema_version: Some(found),
                    input_path: head.input_path.map(Cow::into_owned),
                });
            }
            "fact" => {
                let Some(kind) = head.kind else {
                    return Err(ScanError::Line {
                        line,
                        message: "fact record has no `type`".into(),
                    }
                    .into());
                };
                if wanted(&kind) {
                    on_fact(line, &kind, text)?;
                }
            }
            _ => {}
        }
    }
    match metadata {
        Some(m) => Ok(m),
        None if require_metadata => Err(ScanError::NoMetadata.into()),
        None => Ok(TraceMetadata::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(text: &str) -> Result<(TraceMetadata, Vec<String>), ScanError> {
        let mut seen = Vec::new();
        let meta = scan_trace(
            text.as_bytes(),
            true,
            |k| k == "wanted",
            |_, k, _| -> Result<(), ScanError> {
                seen.push(k.to_string());
                Ok(())
            },
        )?;
        Ok((meta, seen))
    }

    #[test]
    fn versions_one_and_two_are_read_in_any_key_order_and_spacing() {
        for meta in [
            r#"{"record":"metadata","schema_version":1}"#,
            r#"{"schema_version":2,"record":"metadata","input_path":"a.fe"}"#,
            r#"{ "record": "metadata", "schema_version": 2 }"#,
        ] {
            let text = format!(
                "{meta}\n{{ \"type\" : \"wanted\", \"record\" : \"fact\" }}\n{{\"record\":\"fact\",\"type\":\"other\"}}\n"
            );
            let (m, seen) = scan(&text).unwrap();
            assert!(m.schema_version.is_some());
            assert_eq!(seen, vec!["wanted"]);
        }
    }

    #[test]
    fn missing_string_and_unknown_versions_are_rejected() {
        for (meta, expected) in [
            (r#"{"record":"metadata"}"#, "no `schema_version`"),
            (
                r#"{"record":"metadata","schema_version":"2"}"#,
                "not a non-negative integer",
            ),
            (
                r#"{"schema_version":99,"record":"metadata"}"#,
                "unsupported trace schema version 99",
            ),
            (
                r#"{ "record": "metadata", "schema_version": 3 }"#,
                "unsupported trace schema version 3",
            ),
        ] {
            let err = scan(meta).unwrap_err().to_string();
            assert!(err.contains(expected), "{meta}: {err}");
        }
        let err = scan(r#"{"record":"fact","type":"wanted"}"#).unwrap_err();
        assert!(matches!(err, ScanError::NoMetadata), "{err}");
        let err = scan("{\"record\":\"metadata\",\"schema_version\":2}\n{\"type\":\"x\"}")
            .unwrap_err()
            .to_string();
        assert!(err.contains("line 2") && err.contains("record"), "{err}");
    }
}
