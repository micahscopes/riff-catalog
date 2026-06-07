//! The canonical byte encoding. `pub(crate)` on purpose: every digest in the
//! system is produced through [`digest_record`] / [`digest_meta`], so the
//! domain-separation discipline (magic, schema version, full policy context,
//! record tag) cannot be bypassed from outside this crate.

use crate::SCHEMA_VERSION;
use crate::dimension::Dimension;
use crate::error::CatalogError;
use crate::key::NodeKey;
use crate::policy::HashPolicy;
use crate::text::Digest;
use crate::value::Value;

pub(crate) const MAGIC: &str = "riffcat";

pub(crate) fn push_str(bytes: &mut Vec<u8>, value: &str) {
    push_u64(bytes, value.len() as u64);
    bytes.extend_from_slice(value.as_bytes());
}

pub(crate) fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn push_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn push_i64(bytes: &mut Vec<u8>, value: i64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

/// Raw 32 bytes, no length prefix (fixed width).
pub(crate) fn push_digest(bytes: &mut Vec<u8>, digest: &Digest) {
    bytes.extend_from_slice(digest.as_bytes());
}

pub(crate) fn push_node_key(bytes: &mut Vec<u8>, key: &NodeKey) {
    push_str(bytes, &key.canonical_key());
}

pub(crate) fn push_value(bytes: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Text(value) => {
            push_str(bytes, "text");
            push_str(bytes, value);
        }
        Value::Bool(value) => {
            push_str(bytes, "bool");
            bytes.push(u8::from(*value));
        }
        Value::U64(value) => {
            push_str(bytes, "u64");
            push_u64(bytes, *value);
        }
        Value::I64(value) => {
            push_str(bytes, "i64");
            push_i64(bytes, *value);
        }
        Value::Bytes(value) => {
            push_str(bytes, "bytes");
            push_u64(bytes, value.len() as u64);
            bytes.extend_from_slice(value);
        }
    }
}

/// The workhorse for every "sorted multiset of records" in the format: sorts
/// the encoded records lexicographically by their raw bytes, then pushes a u32
/// count followed by the concatenation. One helper, one ordering rule — no
/// key-derived sorting can sneak into anonymous payloads (invariant I5).
pub(crate) fn push_sorted_records(bytes: &mut Vec<u8>, mut records: Vec<Vec<u8>>) {
    records.sort_unstable();
    push_u32(bytes, records.len() as u32);
    for record in records {
        bytes.extend_from_slice(&record);
    }
}

/// A dimension-scoped record: the header commits to the full policy context
/// plus the dimension and a record tag (invariant I1).
pub(crate) fn digest_record(
    policy: &HashPolicy,
    dimension: Dimension,
    record_tag: &str,
    write_payload: impl FnOnce(&mut Vec<u8>),
) -> Result<Digest, CatalogError> {
    policy.check_supported()?;
    let mut bytes = Vec::new();
    push_str(&mut bytes, MAGIC);
    push_u32(&mut bytes, policy.schema_version);
    push_str(&mut bytes, policy.algorithm.as_str());
    push_str(&mut bytes, policy.level.as_str());
    push_str(&mut bytes, dimension.as_str());
    push_str(&mut bytes, policy.view_mode.as_str());
    push_str(&mut bytes, policy.cycle_policy.as_str());
    push_str(&mut bytes, record_tag);
    write_payload(&mut bytes);
    Ok(digest_bytes(&bytes))
}

/// A policy-independent meta record (policy ids, facet ids, claim ids):
/// magic + schema version + tag + payload.
pub(crate) fn digest_meta(tag: &str, write_payload: impl FnOnce(&mut Vec<u8>)) -> Digest {
    let mut bytes = Vec::new();
    push_str(&mut bytes, MAGIC);
    push_u32(&mut bytes, SCHEMA_VERSION);
    push_str(&mut bytes, tag);
    write_payload(&mut bytes);
    digest_bytes(&bytes)
}

pub(crate) fn digest_bytes(bytes: &[u8]) -> Digest {
    Digest::from_bytes(*blake3::hash(bytes).as_bytes())
}
