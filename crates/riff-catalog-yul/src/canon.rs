//! Literal canonicalization shared by the lowering (invariant I10: the
//! lowering — including this normalization — is part of the encoding
//! contract, identified by the level string "yul-ast/1").
//!
//! solc preserves number spellings verbatim in both `ir` text and `irAst`
//! JSON (verified: `0x40` and `64` both occur in the corpus), so without
//! this, equal constants would produce distinct Constants digests.

use crate::ast::{Literal, LiteralKind};
use crate::error::YulLowerError;

/// Canonical number form: minimal lowercase hex with `0x` prefix ("0x0" for
/// zero). Accepts decimal and 0x-hex spellings of arbitrary width (Yul
/// numbers are u256): simple base-10 → bytes accumulation, no bigint dep.
pub fn canon_number(spelling: &str) -> Result<String, YulLowerError> {
    let invalid = || YulLowerError::InvalidNumber(spelling.to_string());
    let digits: Vec<u8> = if let Some(hex) = spelling
        .strip_prefix("0x")
        .or_else(|| spelling.strip_prefix("0X"))
    {
        if hex.is_empty() {
            return Err(invalid());
        }
        let mut nibbles = Vec::with_capacity(hex.len());
        for ch in hex.chars() {
            nibbles.push(ch.to_digit(16).ok_or_else(invalid)? as u8);
        }
        nibbles
    } else {
        if spelling.is_empty() || !spelling.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid());
        }
        // base-10 -> base-16 nibble accumulation
        let mut nibbles: Vec<u8> = vec![0];
        for byte in spelling.bytes() {
            let digit = byte - b'0';
            let mut carry = u32::from(digit);
            for nibble in nibbles.iter_mut().rev() {
                let value = u32::from(*nibble) * 10 + carry;
                *nibble = (value & 0xf) as u8;
                carry = value >> 4;
            }
            while carry > 0 {
                nibbles.insert(0, (carry & 0xf) as u8);
                carry >>= 4;
            }
        }
        nibbles
    };

    let trimmed: Vec<u8> = digits
        .iter()
        .copied()
        .skip_while(|&nibble| nibble == 0)
        .collect();
    if trimmed.is_empty() {
        return Ok("0x0".to_string());
    }
    let mut out = String::with_capacity(2 + trimmed.len());
    out.push_str("0x");
    for nibble in trimmed {
        out.push(char::from_digit(u32::from(nibble), 16).expect("nibble"));
    }
    Ok(out)
}

/// Canonical literal (value, optional type) for the Constants/Types
/// dimensions. String literals are already canonical 0x-hex in the AST
/// (normalized at both front doors); bools pass through.
pub fn canon_literal(literal: &Literal) -> Result<(String, Option<&str>), YulLowerError> {
    let value = match literal.kind {
        LiteralKind::Number => canon_number(&literal.value)?,
        LiteralKind::String => {
            if !literal.value.starts_with("0x") {
                return Err(YulLowerError::InvalidStringHex(literal.value.clone()));
            }
            literal.value.clone()
        }
        LiteralKind::Bool => literal.value.clone(),
    };
    Ok((value, literal.ty.as_deref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_and_decimal_unify() {
        assert_eq!(canon_number("0x40").unwrap(), "0x40");
        assert_eq!(canon_number("64").unwrap(), "0x40");
        assert_eq!(canon_number("0").unwrap(), "0x0");
        assert_eq!(canon_number("0x0").unwrap(), "0x0");
        assert_eq!(canon_number("0x00040").unwrap(), "0x40");
        assert_eq!(canon_number("0xABCDEF").unwrap(), "0xabcdef");
    }

    #[test]
    fn handles_u256_scale_decimals() {
        // 2^255 = 578960446186580977117854925043439539266349923328202820197287920039565648199168 / no — use a known pair:
        // 10^18 in hex
        assert_eq!(
            canon_number("1000000000000000000").unwrap(),
            "0xde0b6b3a7640000"
        );
        // 77-digit decimal (max-uint256-ish) survives without overflow
        let max = "115792089237316195423570985008687907853269984665640564039457584007913129639935";
        assert_eq!(canon_number(max).unwrap(), format!("0x{}", "f".repeat(64)));
    }

    #[test]
    fn rejects_junk() {
        assert!(canon_number("").is_err());
        assert!(canon_number("0x").is_err());
        assert!(canon_number("12a").is_err());
        assert!(canon_number("1e18").is_err());
    }
}
