//! Tagged meta-record builder for downstream crates (claims) that need
//! canonical digests of their own record types without access to the raw
//! encoder. The header (magic, schema version, tag) is enforced here, so the
//! domain-separation discipline survives the crate boundary.

use crate::SCHEMA_VERSION;
use crate::encode;
use crate::text::Digest;

pub struct MetaRecord {
    bytes: Vec<u8>,
}

impl MetaRecord {
    /// Start a record. `tag` must be a stable, namespaced record tag
    /// (e.g. "riffcat.claim"); changing a tag is a schema change.
    pub fn new(tag: &str) -> Self {
        let mut bytes = Vec::new();
        encode::push_str(&mut bytes, encode::MAGIC);
        encode::push_u32(&mut bytes, SCHEMA_VERSION);
        encode::push_str(&mut bytes, tag);
        Self { bytes }
    }

    pub fn push_str(&mut self, value: &str) -> &mut Self {
        encode::push_str(&mut self.bytes, value);
        self
    }

    pub fn push_u32(&mut self, value: u32) -> &mut Self {
        encode::push_u32(&mut self.bytes, value);
        self
    }

    pub fn push_digest(&mut self, digest: &Digest) -> &mut Self {
        encode::push_digest(&mut self.bytes, digest);
        self
    }

    pub fn finish(self) -> Digest {
        encode::digest_bytes(&self.bytes)
    }
}
