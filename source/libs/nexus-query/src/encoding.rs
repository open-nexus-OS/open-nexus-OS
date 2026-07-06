// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Order-preserving key encoding + deterministic row codec — the
//! correctness core of the query engine (byte order == value order, proven
//! by exhaustive pair tests incl. sign, prefix, and NUL cases).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 5 tests (order properties, tuple composition, row roundtrip)
//!
//! Invariant (property-tested): for values `a`, `b` of the same type,
//! `a < b  ⇔  encode(a) < encode(b)` under plain byte-wise comparison, and
//! encodings are **self-terminating** so tuple concatenation preserves
//! lexicographic tuple order (no prefix ambiguity).
//!
//! Formats:
//! - `Int`/`Fx` (i64): sign-bit flip → big-endian u64 (order-preserving);
//! - `Bool`: one byte 0/1;
//! - `Str`: UTF-8 bytes with `0x00` escaped as `0x00 0xFF`, terminated by
//!   `0x00 0x00` (the classic escape-terminate scheme — a string that is a
//!   prefix of another sorts first, embedded NULs stay ordered).

use alloc::{string::String, vec::Vec};

/// A typed column value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QVal {
    Bool(bool),
    Int(i64),
    /// Raw Q32.32 fixed point.
    Fx(i64),
    Str(String),
}

/// Column type tags (schema + validation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QType {
    Bool,
    Int,
    Fx,
    Str,
}

impl QVal {
    #[must_use]
    pub fn kind(&self) -> QType {
        match self {
            QVal::Bool(_) => QType::Bool,
            QVal::Int(_) => QType::Int,
            QVal::Fx(_) => QType::Fx,
            QVal::Str(_) => QType::Str,
        }
    }
}

/// Appends the order-preserving encoding of `value` to `out`.
pub fn encode_key(value: &QVal, out: &mut Vec<u8>) {
    match value {
        QVal::Bool(b) => out.push(u8::from(*b)),
        QVal::Int(i) | QVal::Fx(i) => {
            let flipped = (*i as u64) ^ (1u64 << 63);
            out.extend_from_slice(&flipped.to_be_bytes());
        }
        QVal::Str(s) => {
            for &byte in s.as_bytes() {
                if byte == 0x00 {
                    out.extend_from_slice(&[0x00, 0xFF]);
                } else {
                    out.push(byte);
                }
            }
            out.extend_from_slice(&[0x00, 0x00]);
        }
    }
}

/// Deterministic row (value) encoding: tag + payload, length-prefixed strings.
/// NOT order-preserving — rows are payloads, keys carry the order.
pub fn encode_row(values: &[QVal], out: &mut Vec<u8>) {
    out.extend_from_slice(&(values.len() as u32).to_le_bytes());
    for value in values {
        match value {
            QVal::Bool(b) => {
                out.push(0);
                out.push(u8::from(*b));
            }
            QVal::Int(i) => {
                out.push(1);
                out.extend_from_slice(&i.to_le_bytes());
            }
            QVal::Fx(f) => {
                out.push(2);
                out.extend_from_slice(&f.to_le_bytes());
            }
            QVal::Str(s) => {
                out.push(3);
                out.extend_from_slice(&(s.len() as u32).to_le_bytes());
                out.extend_from_slice(s.as_bytes());
            }
        }
    }
}

/// Decodes a row encoded by [`encode_row`]. `None` on corruption.
#[must_use]
pub fn decode_row(bytes: &[u8]) -> Option<Vec<QVal>> {
    let mut cursor = 0usize;
    let take = |cursor: &mut usize, n: usize| -> Option<&[u8]> {
        let slice = bytes.get(*cursor..*cursor + n)?;
        *cursor += n;
        Some(slice)
    };
    let count = u32::from_le_bytes(take(&mut cursor, 4)?.try_into().ok()?) as usize;
    if count > 4096 {
        return None; // bounded — a hostile length can't allocate the moon
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let tag = *take(&mut cursor, 1)?.first()?;
        values.push(match tag {
            0 => QVal::Bool(*take(&mut cursor, 1)?.first()? != 0),
            1 => QVal::Int(i64::from_le_bytes(take(&mut cursor, 8)?.try_into().ok()?)),
            2 => QVal::Fx(i64::from_le_bytes(take(&mut cursor, 8)?.try_into().ok()?)),
            3 => {
                let len =
                    u32::from_le_bytes(take(&mut cursor, 4)?.try_into().ok()?) as usize;
                let raw = take(&mut cursor, len)?;
                QVal::Str(String::from_utf8(raw.to_vec()).ok()?)
            }
            _ => return None,
        });
    }
    if cursor == bytes.len() {
        Some(values)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(v: &QVal) -> Vec<u8> {
        let mut out = Vec::new();
        encode_key(v, &mut out);
        out
    }

    /// The core property: value order == encoded byte order.
    #[test]
    fn int_encoding_preserves_order_across_sign() {
        let samples: [i64; 12] = [
            i64::MIN,
            i64::MIN + 1,
            -1_000_000,
            -42,
            -1,
            0,
            1,
            7,
            1_000_000,
            i64::MAX - 1,
            i64::MAX,
            123_456_789,
        ];
        for &a in &samples {
            for &b in &samples {
                assert_eq!(
                    a.cmp(&b),
                    enc(&QVal::Int(a)).cmp(&enc(&QVal::Int(b))),
                    "order broken for {a} vs {b}"
                );
            }
        }
    }

    #[test]
    fn fx_encoding_preserves_order() {
        let samples: [i64; 6] =
            [i64::MIN, -(1 << 32), -(1 << 31), 0, 1 << 31, i64::MAX];
        for &a in &samples {
            for &b in &samples {
                assert_eq!(a.cmp(&b), enc(&QVal::Fx(a)).cmp(&enc(&QVal::Fx(b))));
            }
        }
    }

    #[test]
    fn str_encoding_preserves_order_including_prefix_and_nul() {
        let samples =
            ["", "a", "a\0", "a\0b", "aa", "ab", "b", "ba", "z", "za", "\0", "\0\0"];
        for a in samples {
            for b in samples {
                assert_eq!(
                    a.cmp(b),
                    enc(&QVal::Str(a.into())).cmp(&enc(&QVal::Str(b.into()))),
                    "order broken for {a:?} vs {b:?}"
                );
            }
        }
    }

    /// Self-termination: tuple concatenation preserves tuple order.
    #[test]
    fn tuple_concatenation_preserves_lexicographic_order() {
        let tuples: [(&str, i64); 6] =
            [("a", 5), ("a", 6), ("a\0", 0), ("aa", -3), ("ab", i64::MIN), ("b", 0)];
        let enc2 = |(s, i): (&str, i64)| {
            let mut out = Vec::new();
            encode_key(&QVal::Str(s.into()), &mut out);
            encode_key(&QVal::Int(i), &mut out);
            out
        };
        for &a in &tuples {
            for &b in &tuples {
                assert_eq!(a.cmp(&b), enc2(a).cmp(&enc2(b)), "tuple order broken {a:?} {b:?}");
            }
        }
    }

    #[test]
    fn rows_roundtrip_and_reject_corruption() {
        let row = alloc::vec![
            QVal::Int(-7),
            QVal::Str("hello\0world".into()),
            QVal::Bool(true),
            QVal::Fx(1 << 31),
        ];
        let mut bytes = Vec::new();
        encode_row(&row, &mut bytes);
        assert_eq!(decode_row(&bytes), Some(row));
        // Truncation + trailing garbage are both rejected.
        assert_eq!(decode_row(&bytes[..bytes.len() - 1]), None);
        let mut long = bytes.clone();
        long.push(0);
        assert_eq!(decode_row(&long), None);
    }
}
