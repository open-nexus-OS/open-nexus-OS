// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A field of an instruction word: bit pieces over bytes, most significant piece first — the
//! form in which the instruction description states every field. Reads take what the bytes
//! hold; writes refuse a value wider than the field (no silent truncation, so a backend that
//! picks a register or an offset out of range is told by name).

use super::RogueError;

/// One piece of a field: bits `hi..=lo` of byte `byte`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece {
    pub byte: u8,
    pub hi: u8,
    pub lo: u8,
}

/// A piece; `hi >= lo`, both below 8.
pub const fn pc(byte: u8, hi: u8, lo: u8) -> Piece {
    Piece { byte, hi, lo }
}

impl Piece {
    const fn width(self) -> u32 {
        (self.hi - self.lo + 1) as u32
    }
}

/// A field: its pieces, most significant first.
pub type Field = &'static [Piece];

/// The field's width in bits.
pub fn width(f: Field) -> u32 {
    f.iter().map(|p| p.width()).sum()
}

/// The bytes the field touches: one past its highest byte.
pub fn span(f: Field) -> usize {
    f.iter().map(|p| p.byte as usize + 1).max().unwrap_or(0)
}

/// The field's value; bytes past the slice read as zero (callers check the length first).
pub fn get(bytes: &[u8], f: Field) -> u32 {
    let mut v = 0u32;
    for p in f {
        let w = p.width();
        let b = u32::from(bytes.get(p.byte as usize).copied().unwrap_or(0));
        v = (v << w) | ((b >> p.lo) & ((1u32 << w) - 1));
    }
    v
}

/// Writes `value` into the field; refuses a value wider than the field or a piece past `bytes`.
pub fn put(bytes: &mut [u8], f: Field, value: u32, what: &'static str) -> Result<(), RogueError> {
    let w = width(f);
    if w < 32 && value >> w != 0 {
        return Err(RogueError::TooWide { what, value });
    }
    let mut v = value;
    for p in f.iter().rev() {
        let pw = p.width();
        let mask = ((1u32 << pw) - 1) as u8;
        let b = bytes.get_mut(p.byte as usize).ok_or(RogueError::TooWide { what, value })?;
        *b = (*b & !(mask << p.lo)) | (((v as u8) & mask) << p.lo);
        v = if pw < 32 { v >> pw } else { 0 };
    }
    Ok(())
}

/// A boolean field (one bit).
pub fn flag(bytes: &[u8], f: Field) -> bool {
    get(bytes, f) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPLIT: Field = &[pc(2, 2, 0), pc(1, 1, 0), pc(0, 5, 0)];

    #[test]
    fn a_split_field_reads_and_writes_its_pieces_most_significant_first() {
        let mut b = [0u8; 3];
        put(&mut b, SPLIT, 0b101_1011_0011, "split").unwrap();
        assert_eq!(b, [0b0011_0011, 0b0000_0010, 0b0000_0101]);
        assert_eq!(get(&b, SPLIT), 0b101_1011_0011);
        assert_eq!((width(SPLIT), span(SPLIT)), (11, 3));
    }

    #[test]
    fn test_reject_a_value_wider_than_its_field() {
        let mut b = [0u8; 3];
        assert_eq!(
            put(&mut b, SPLIT, 1 << 11, "split"),
            Err(RogueError::TooWide { what: "split", value: 1 << 11 })
        );
        assert_eq!(b, [0; 3], "nothing written");
    }
}
