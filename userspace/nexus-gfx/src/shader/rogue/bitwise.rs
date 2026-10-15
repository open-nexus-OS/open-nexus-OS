// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The bitwise unit's instruction words, one per phase: phase 0 counts, masks or carries a 16-
//! or 32-bit immediate and shifts; phase 1 combines logically; phase 2 shifts and tests. A word
//! does not name its phase — its place in the group does (the header's phase mask, last phase
//! first).

use super::bits::{flag, get, pc, put, Field};
use super::{Phase, RogueError};

const PH0: Field = &[pc(0, 7, 7)];
const PH1: Field = &[pc(0, 6, 6)];
const COUNT_SRC: Field = &[pc(0, 6, 6)];
const BITMASK_IMM: Field = &[pc(0, 4, 4)];
const COUNT_OP: Field = &[pc(0, 1, 1), pc(0, 5, 5)];
const BITMASK_OP: Field = &[pc(0, 0, 0)];
const SHIFT1_OP: Field = &[pc(0, 3, 2)];
const IMM16: Field = &[pc(2, 7, 0), pc(1, 7, 0)];
const IMM32: Field = &[pc(4, 7, 0), pc(3, 7, 0), pc(2, 7, 0), pc(1, 7, 0)];
const MSKB: Field = &[pc(0, 5, 5)];
const RSVD0_PH1: Field = &[pc(0, 4, 4)];
const MSKA: Field = &[pc(0, 3, 3)];
const LOGICAL_OP: Field = &[pc(0, 2, 0)];
const PWEN: Field = &[pc(0, 5, 5)];
const TST_SRC: Field = &[pc(0, 4, 4)];
const TST_OP: Field = &[pc(0, 3, 3)];
const SHIFT2_OP: Field = &[pc(0, 2, 0)];

/// Phase 0's mask: an operation on a source, or an immediate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bitmask {
    /// Bypass (0) or mask (1).
    Src(u8),
    Imm16(u16),
    Imm32(u32),
}

/// A bitwise instruction word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitwiseInstr {
    Phase0 { count_src: u8, count_op: u8, bitmask: Bitmask, shift1: u8 },
    Phase1 { mskb: bool, mska: bool, logical_op: u8 },
    Phase2 { pwen: bool, tst_src: u8, tst_op: u8, shift2: u8 },
}

impl BitwiseInstr {
    /// Decodes the word at the start of `b` in `phase` (0, 1 or 2); its length in bytes.
    pub fn decode(b: &[u8], phase: Phase, at: usize) -> Result<(Self, usize), RogueError> {
        let need = |n: usize| if b.len() < n { Err(RogueError::Truncated { at }) } else { Ok(()) };
        need(1)?;
        if flag(b, PH0) {
            return Err(RogueError::Reserved { at, what: "bitwise phase bit" });
        }
        match phase {
            Phase::P0 => {
                let (count_src, count_op, shift1) =
                    (get(b, COUNT_SRC) as u8, get(b, COUNT_OP) as u8, get(b, SHIFT1_OP) as u8);
                let (bitmask, n) = if !flag(b, BITMASK_IMM) {
                    (Bitmask::Src(get(b, BITMASK_OP) as u8), 1)
                } else if get(b, BITMASK_OP) == 0 {
                    need(3)?;
                    (Bitmask::Imm16(get(b, IMM16) as u16), 3)
                } else {
                    need(5)?;
                    (Bitmask::Imm32(get(b, IMM32)), 5)
                };
                Ok((Self::Phase0 { count_src, count_op, bitmask, shift1 }, n))
            }
            Phase::P1 => {
                if !flag(b, PH1) || get(b, RSVD0_PH1) != 0 {
                    return Err(RogueError::Malformed { at, what: "bitwise phase-1 word" });
                }
                Ok((
                    Self::Phase1 {
                        mskb: flag(b, MSKB),
                        mska: flag(b, MSKA),
                        logical_op: get(b, LOGICAL_OP) as u8,
                    },
                    1,
                ))
            }
            Phase::P2 => {
                if flag(b, PH1) {
                    return Err(RogueError::Malformed { at, what: "bitwise phase-2 word" });
                }
                let i = Self::Phase2 {
                    pwen: flag(b, PWEN),
                    tst_src: get(b, TST_SRC) as u8,
                    tst_op: get(b, TST_OP) as u8,
                    shift2: get(b, SHIFT2_OP) as u8,
                };
                Ok((i, 1))
            }
            _ => Err(RogueError::Malformed { at, what: "bitwise word outside phases 0–2" }),
        }
    }

    /// The word's phase.
    pub const fn phase(&self) -> Phase {
        match self {
            Self::Phase0 { .. } => Phase::P0,
            Self::Phase1 { .. } => Phase::P1,
            Self::Phase2 { .. } => Phase::P2,
        }
    }

    /// Bytes the word takes.
    pub const fn size(&self) -> usize {
        match self {
            Self::Phase0 { bitmask: Bitmask::Imm16(_), .. } => 3,
            Self::Phase0 { bitmask: Bitmask::Imm32(_), .. } => 5,
            _ => 1,
        }
    }

    /// Writes the word into `out[..self.size()]`.
    pub fn encode(&self, out: &mut [u8]) -> Result<(), RogueError> {
        let n = self.size();
        let b = out.get_mut(..n).ok_or(RogueError::TooWide { what: "bitwise word", value: 0 })?;
        b.fill(0);
        match *self {
            Self::Phase0 { count_src, count_op, bitmask, shift1 } => {
                put(b, COUNT_SRC, u32::from(count_src), "count source")?;
                put(b, COUNT_OP, u32::from(count_op), "count op")?;
                put(b, SHIFT1_OP, u32::from(shift1), "shift1 op")?;
                match bitmask {
                    Bitmask::Src(op) => put(b, BITMASK_OP, u32::from(op), "bitmask op"),
                    Bitmask::Imm16(v) => {
                        put(b, BITMASK_IMM, 1, "bitmask immediate")?;
                        put(b, IMM16, u32::from(v), "imm16")
                    }
                    Bitmask::Imm32(v) => {
                        put(b, BITMASK_IMM, 1, "bitmask immediate")?;
                        put(b, BITMASK_OP, 1, "bitmask op")?;
                        put(b, IMM32, v, "imm32")
                    }
                }
            }
            Self::Phase1 { mskb, mska, logical_op } => {
                put(b, PH1, 1, "phase-1 bit")?;
                put(b, MSKB, u32::from(mskb), "mskb")?;
                put(b, MSKA, u32::from(mska), "mska")?;
                put(b, LOGICAL_OP, u32::from(logical_op), "logical op")
            }
            Self::Phase2 { pwen, tst_src, tst_op, shift2 } => {
                put(b, PWEN, u32::from(pwen), "pwen")?;
                put(b, TST_SRC, u32::from(tst_src), "test source")?;
                put(b, TST_OP, u32::from(tst_op), "test op")?;
                put(b, SHIFT2_OP, u32::from(shift2), "shift2 op")
            }
        }
    }
}
