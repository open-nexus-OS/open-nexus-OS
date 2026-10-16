// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The end-of-tile program (TASK-0215 5c-3 P1): where a render pass's tiles go. The pixel back
//! end's two state words — the target's address, beside it the stride and the format — are
//! immediates of the end-of-tile program the fragment job's pixel event kicks: two moves of an
//! immediate, then the emit (the render bundle's program at code-heap offset `0x7e80`, its pixel
//! event data at PDS-heap offset `0x2980`, read 2026-10-16). A pass into another target names a
//! copy of the program with that target's address — placed once per target, never rewritten —
//! through the pixel event data naming the copy; nothing else of the pass changes. Every target
//! keeps the captured stride and format (a plane's: 1920 pixels of four bytes).

/// The program's bytes: two moves (three words each) and the emit (four words).
pub const PROGRAM_BYTES: usize = 40;
/// The pixel event's data: the program's code-heap offset, then its control word.
pub const DATA_BYTES: usize = 8;
/// A move of a 32-bit immediate into a register, as the open driver encodes the two loads.
const MOV_IMMEDIATE: u32 = 0x1340_9286;
/// The two moves' byte offsets and their immediates': state word 1 (the address's bits 34–39
/// in its bits 0–5, beside the stride and the format), state word 0 (the address's bits 2–33).
const MOVES: [usize; 2] = [0, 12];
const WORD1_AT: usize = 4;
const WORD0_AT: usize = 16;
/// The address bits state word 1 carries.
const WORD1_ADDRESS: u32 = 0x3f;
/// A target's alignment (a plane row's start is a multiple of its 7 680 bytes).
const TARGET_ALIGN: u64 = 64;
/// A program's alignment in the code heap.
const PROGRAM_ALIGN: u32 = 16;

/// Why a program, its data or a target was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EotError {
    /// Not the two moves and the emit of the open driver's program.
    Shape,
    /// Not a 40-bit address, or not on its alignment.
    Target,
    /// A code-heap offset off its alignment.
    Offset,
}

fn word(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// The captured `program` with its target replaced by `target_va`: the two state words' address
/// fields, nothing else.
pub fn retarget(program: &[u8], target_va: u64) -> Result<[u8; PROGRAM_BYTES], EotError> {
    if program.len() != PROGRAM_BYTES || MOVES.iter().any(|&at| word(program, at) != MOV_IMMEDIATE)
    {
        return Err(EotError::Shape);
    }
    if target_va >> 40 != 0 || target_va % TARGET_ALIGN != 0 {
        return Err(EotError::Target);
    }
    let mut out = [0u8; PROGRAM_BYTES];
    out.copy_from_slice(program);
    let word1 = (word(program, WORD1_AT) & !WORD1_ADDRESS) | ((target_va >> 34) as u32);
    out[WORD1_AT..WORD1_AT + 4].copy_from_slice(&word1.to_le_bytes());
    out[WORD0_AT..WORD0_AT + 4].copy_from_slice(&((target_va >> 2) as u32).to_le_bytes());
    Ok(out)
}

/// The target a program writes (its two state words' address fields).
pub fn target(program: &[u8]) -> Result<u64, EotError> {
    if program.len() != PROGRAM_BYTES || MOVES.iter().any(|&at| word(program, at) != MOV_IMMEDIATE)
    {
        return Err(EotError::Shape);
    }
    let high = u64::from(word(program, WORD1_AT) & WORD1_ADDRESS) << 34;
    Ok(high | (u64::from(word(program, WORD0_AT)) << 2))
}

/// The captured pixel event `data` naming the program copy at code-heap offset `code_offset`:
/// its first word replaced, the control word kept.
pub fn data(captured: &[u8], code_offset: u32) -> Result<[u8; DATA_BYTES], EotError> {
    if captured.len() != DATA_BYTES {
        return Err(EotError::Shape);
    }
    if code_offset % PROGRAM_ALIGN != 0 {
        return Err(EotError::Offset);
    }
    let mut out = [0u8; DATA_BYTES];
    out.copy_from_slice(captured);
    out[..4].copy_from_slice(&code_offset.to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The render bundle's program (code heap `0x7e80`) and its data (PDS heap `0x2980`); the
    /// bundle's target at `0x80_000b_1000`.
    const PROGRAM: [u32; 10] = [
        0x1340_9286,
        0x0008_2020,
        0xff40_0000,
        0x1340_9286,
        0x0002_c400,
        0xff41_0000,
        0xc280_a048,
        0x4000_40c1,
        0xfff3_0000,
        0xffff_ffff,
    ];
    const DATA: [u32; 2] = [0x0000_7e80, 0x0000_0008];
    const TARGET: u64 = 0x80_000b_1000;

    fn bytes(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    #[test]
    fn the_captured_program_writes_the_captured_target() {
        let p = bytes(&PROGRAM);
        assert_eq!(target(&p), Ok(TARGET));
        assert_eq!(retarget(&p, TARGET).unwrap().to_vec(), p, "the same target: the same bytes");
    }

    #[test]
    fn another_target_changes_the_two_address_fields_only() {
        let p = bytes(&PROGRAM);
        // A plane row 640 rows into the target.
        let other = TARGET + 640 * 7680;
        let q = retarget(&p, other).unwrap();
        assert_eq!(target(&q), Ok(other));
        let changed: Vec<usize> =
            (0..10).filter(|&i| p[i * 4..i * 4 + 4] != q[i * 4..i * 4 + 4]).collect();
        assert_eq!(changed, vec![4], "the bundle's high bits stay 0x20: only state word 0");
        // A target above the 34-bit boundary rewrites state word 1's address bits, not its
        // stride and format.
        let far = 0xc4_0000_0000;
        let r = retarget(&p, far).unwrap();
        assert_eq!(word(&r, WORD1_AT), 0x0008_2031);
        assert_eq!(target(&r), Ok(far));
    }

    #[test]
    fn test_reject_a_program_of_another_shape_or_a_bad_target() {
        let p = bytes(&PROGRAM);
        assert_eq!(retarget(&p[..36], TARGET), Err(EotError::Shape));
        let mut other = p.clone();
        other[12] ^= 1;
        assert_eq!(retarget(&other, TARGET), Err(EotError::Shape));
        assert_eq!(retarget(&p, TARGET + 4), Err(EotError::Target), "off its alignment");
        assert_eq!(retarget(&p, 1 << 40), Err(EotError::Target), "past 40 bits");
    }

    #[test]
    fn the_data_names_the_copy_and_keeps_its_control_word() {
        let d = bytes(&DATA);
        assert_eq!(data(&d, 0x7e80).unwrap().to_vec(), d);
        assert_eq!(data(&d, 0x1_1000).unwrap().to_vec(), bytes(&[0x1_1000, 0x8]));
        assert_eq!(data(&d, 0x1_1008), Err(EotError::Offset));
        assert_eq!(data(&d[..4], 0x1_1000), Err(EotError::Shape));
    }
}
