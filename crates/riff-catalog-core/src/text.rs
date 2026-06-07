use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::CatalogError;

/// Separator used when joining key parts into canonical strings. Excluded from
/// every validated text so canonical joins are unambiguous.
pub(crate) const UNIT_SEP: char = '\u{1f}';

/// A validated non-empty string: never empty, never contains [`UNIT_SEP`].
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Name(String);

impl Name {
    pub fn new(value: impl Into<String>, field: &'static str) -> Result<Self, CatalogError> {
        let value = value.into();
        if value.is_empty() {
            return Err(CatalogError::EmptyText { field });
        }
        if value.contains(UNIT_SEP) {
            return Err(CatalogError::InvalidText { field });
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A blake3-256 digest. Stored as raw bytes; serialized as 64 lowercase hex.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest([u8; 32]);

impl Digest {
    pub(crate) fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn from_hex(hex: &str) -> Result<Self, CatalogError> {
        if hex.len() != 64 {
            return Err(CatalogError::InvalidDigest);
        }
        let mut bytes = [0u8; 32];
        for (i, chunk) in hex.as_bytes().chunks_exact(2).enumerate() {
            let hi = hex_val(chunk[0]).ok_or(CatalogError::InvalidDigest)?;
            let lo = hex_val(chunk[1]).ok_or(CatalogError::InvalidDigest)?;
            bytes[i] = (hi << 4) | lo;
        }
        Ok(Self(bytes))
    }

    pub fn to_hex(&self) -> String {
        let mut out = String::with_capacity(64);
        for byte in self.0 {
            out.push(char::from_digit((byte >> 4) as u32, 16).unwrap());
            out.push(char::from_digit((byte & 0xf) as u32, 16).unwrap());
        }
        out
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// First 16 hex chars, for human-facing tables.
    pub fn display_short(&self) -> String {
        self.to_hex()[..16].to_string()
    }
}

/// Lowercase-only: digests are canonical text, uppercase input is rejected.
fn hex_val(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({})", self.to_hex())
    }
}

impl Serialize for Digest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let hex = String::deserialize(deserializer)?;
        Digest::from_hex(&hex).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_rejects_empty_and_separator() {
        assert_eq!(
            Name::new("", "field"),
            Err(CatalogError::EmptyText { field: "field" })
        );
        assert_eq!(
            Name::new("a\u{1f}b", "field"),
            Err(CatalogError::InvalidText { field: "field" })
        );
        assert_eq!(Name::new("ok", "field").unwrap().as_str(), "ok");
    }

    #[test]
    fn digest_hex_round_trip() {
        let digest = Digest::from_bytes([0xab; 32]);
        let hex = digest.to_hex();
        assert_eq!(hex.len(), 64);
        assert_eq!(Digest::from_hex(&hex).unwrap(), digest);
        // uppercase rejected: canonical digests are lowercase
        assert!(Digest::from_hex(&hex.to_uppercase()).is_err());
        assert!(Digest::from_hex("ab").is_err());
    }

    #[test]
    fn digest_serde_is_hex_string() {
        let digest = Digest::from_bytes([1; 32]);
        let json = serde_json::to_string(&digest).unwrap();
        assert_eq!(json, format!("\"{}\"", digest.to_hex()));
        let back: Digest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, digest);
    }
}
