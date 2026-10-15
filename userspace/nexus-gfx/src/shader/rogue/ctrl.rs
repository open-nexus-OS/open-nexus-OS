// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The control instructions, chosen by the control header's op: branch, condition, the
//! zero-byte words (link-address save and restore, wait for operations, wait for a data fence),
//! mutex, no-op, shared-buffer offset, iterate (varyings into registers through the data return
//! channel) and iterate-and-sample (the varyings iterated and a texture sampled in one word —
//! the path a texture fetch with unmodified coordinates takes without the shader core's ALUs).

use super::bits::{flag, get, pc, put, Field};
use super::RogueError;

const RSVD_BRANCH: Field = &[pc(1, 0, 0), pc(0, 0, 0), pc(0, 6, 5), pc(0, 7, 7)];
const LINK: Field = &[pc(0, 4, 4)];
const BPRED: Field = &[pc(0, 3, 2)];
const ABS: Field = &[pc(0, 1, 1)];
const BRANCH_OFFSET: Field = &[pc(4, 7, 0), pc(3, 7, 0), pc(2, 7, 0), pc(1, 7, 1)];
const RSVD_CND: Field = &[pc(0, 7, 7)];
const ADJUST: Field = &[pc(0, 6, 5)];
const PCND: Field = &[pc(0, 4, 3)];
const CNDINST: Field = &[pc(0, 2, 0)];
const LR: Field = &[pc(0, 7, 6)];
const RSVD_MUTEX: Field = &[pc(0, 5, 4)];
const ID: Field = &[pc(0, 3, 0)];
const RSVD_NOP: Field = &[pc(0, 7, 0)];
const RSVD_SBO: Field = &[pc(1, 7, 1)];
const TGT: Field = &[pc(0, 0, 0)];
const SBO_OFFSET: Field = &[pc(1, 0, 0), pc(0, 7, 1)];
const DEST: Field = &[pc(0, 7, 0)];
const COFF: Field = &[pc(1, 7, 2)];
const P_ITR: Field = &[pc(1, 1, 0)];
const WOFF_B2: Field = &[pc(2, 7, 2)];
const DITR_MODE: Field = &[pc(2, 1, 0)];
const ITR_COUNT_B3: Field = &[pc(3, 7, 4)];
const COFF_IDX_B3: Field = &[pc(3, 3, 2)];
const WOFF_IDX_B3: Field = &[pc(3, 1, 0)];
const RSVD4_7_DITR: Field = &[pc(4, 7, 7)];
const F16_B4: Field = &[pc(4, 6, 6)];
const RSVD4_5_DITR: Field = &[pc(4, 5, 4)];
const SCHED_B4: Field = &[pc(4, 3, 2)];
const DRC_B4: Field = &[pc(4, 1, 1)];
const SAT_B4: Field = &[pc(4, 0, 0)];
const TEXOFF: Field = &[pc(2, 7, 2)];
const DRC_B2: Field = &[pc(2, 1, 1)];
const MODE_B2: Field = &[pc(2, 0, 0)];
const SMPOFF: Field = &[pc(3, 7, 2)];
const DMN: Field = &[pc(3, 1, 0)];
const WOFF_B4: Field = &[pc(4, 7, 2)];
const PROJ: Field = &[pc(4, 1, 1)];
const EXT5: Field = &[pc(5, 7, 7)];
const F16_B5: Field = &[pc(5, 6, 6)];
const SCHED_B5: Field = &[pc(5, 5, 4)];
const CHAN: Field = &[pc(5, 3, 2)];
const NNCOORDS: Field = &[pc(5, 1, 1)];
const FCNORM: Field = &[pc(5, 0, 0)];
const EXT6: Field = &[pc(6, 7, 7)];
const ITR_COUNT_B6: Field = &[pc(6, 6, 3)];
const COMPARISON: Field = &[pc(6, 2, 2)];
const RSVD6: Field = &[pc(6, 1, 0)];
const RSVD7: Field = &[pc(7, 7, 5)];
const COFF_IDX_B7: Field = &[pc(7, 4, 3)];
const WOFF_IDX_B7: Field = &[pc(7, 2, 1)];
const MODE_2BIT: Field = &[pc(7, 0, 0), pc(2, 0, 0)];

/// The control ops (the header's `ctrlop`).
pub mod op {
    pub const B: u8 = 0;
    pub const LAPC: u8 = 1;
    pub const SAVL: u8 = 2;
    pub const CND: u8 = 3;
    pub const WOP: u8 = 4;
    pub const WDF: u8 = 5;
    pub const MUTEX: u8 = 6;
    pub const NOP: u8 = 7;
    pub const ITRSMP: u8 = 8;
    pub const SBO: u8 = 11;
    pub const DITR: u8 = 12;
}

/// The iterate word's fields (offsets are the field values: bits 7–2 of the byte offsets).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ditr {
    pub dest: u8,
    pub coff: u8,
    pub persp: u8,
    pub woff: u8,
    pub mode: u8,
    pub count: u8,
    pub coff_idx: u8,
    pub woff_idx: u8,
    pub f16: bool,
    pub sched: u8,
    pub drc: u8,
    pub sat: bool,
}

/// Iterate-and-sample's extensions: the count and comparison (seven bytes), then the index
/// controls and a two-bit mode (eight).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ItrsmpExt {
    pub count: u8,
    pub comparison: bool,
    /// The eight-byte form: (coefficient index control, write index control, the mode's high bit).
    pub idx: Option<(u8, u8, u8)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Itrsmp {
    pub dest: u8,
    pub coff: u8,
    pub persp: u8,
    pub texoff: u8,
    pub drc: u8,
    /// The mode's low bit (the whole mode in the six- and seven-byte forms).
    pub mode: u8,
    pub smpoff: u8,
    pub dmn: u8,
    pub woff: u8,
    pub proj: bool,
    pub sat: bool,
    pub f16: bool,
    pub sched: u8,
    pub chan: u8,
    pub nncoords: bool,
    pub fcnorm: bool,
    pub ext: Option<ItrsmpExt>,
}

/// A control instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtrlInstr {
    /// `offset` is the field: bits 31–1 of the branch's byte offset.
    Branch {
        link: bool,
        bpred: u8,
        abs: bool,
        offset: u32,
    },
    Lapc,
    Savl,
    Wop,
    Wdf,
    Cnd {
        adjust: u8,
        pcnd: u8,
        cndinst: u8,
    },
    Mutex {
        lr: u8,
        id: u8,
    },
    Nop,
    Sbo {
        tgt: u8,
        offset: u8,
    },
    Ditr(Ditr),
    Itrsmp(Itrsmp),
}

impl CtrlInstr {
    /// Decodes the instruction of control op `ctrlop` at the start of `b`; its length.
    pub fn decode(b: &[u8], ctrlop: u8, at: usize) -> Result<(Self, usize), RogueError> {
        let need = |n: usize| if b.len() < n { Err(RogueError::Truncated { at }) } else { Ok(()) };
        let reserved = |f: Field, what| {
            if get(b, f) != 0 {
                Err(RogueError::Reserved { at, what })
            } else {
                Ok(())
            }
        };
        match ctrlop {
            op::B => {
                need(5)?;
                reserved(RSVD_BRANCH, "branch")?;
                let i = Self::Branch {
                    link: flag(b, LINK),
                    bpred: get(b, BPRED) as u8,
                    abs: flag(b, ABS),
                    offset: get(b, BRANCH_OFFSET),
                };
                Ok((i, 5))
            }
            op::LAPC => Ok((Self::Lapc, 0)),
            op::SAVL => Ok((Self::Savl, 0)),
            op::WOP => Ok((Self::Wop, 0)),
            op::WDF => Ok((Self::Wdf, 0)),
            op::CND => {
                need(1)?;
                reserved(RSVD_CND, "condition")?;
                Ok((
                    Self::Cnd {
                        adjust: get(b, ADJUST) as u8,
                        pcnd: get(b, PCND) as u8,
                        cndinst: get(b, CNDINST) as u8,
                    },
                    1,
                ))
            }
            op::MUTEX => {
                need(1)?;
                reserved(RSVD_MUTEX, "mutex")?;
                Ok((Self::Mutex { lr: get(b, LR) as u8, id: get(b, ID) as u8 }, 1))
            }
            op::NOP => {
                need(1)?;
                reserved(RSVD_NOP, "nop")?;
                Ok((Self::Nop, 1))
            }
            op::SBO => {
                need(2)?;
                reserved(RSVD_SBO, "shared-buffer offset")?;
                Ok((Self::Sbo { tgt: get(b, TGT) as u8, offset: get(b, SBO_OFFSET) as u8 }, 2))
            }
            op::DITR => {
                need(5)?;
                reserved(RSVD4_7_DITR, "iterate")?;
                reserved(RSVD4_5_DITR, "iterate")?;
                let d = Ditr {
                    dest: get(b, DEST) as u8,
                    coff: get(b, COFF) as u8,
                    persp: get(b, P_ITR) as u8,
                    woff: get(b, WOFF_B2) as u8,
                    mode: get(b, DITR_MODE) as u8,
                    count: get(b, ITR_COUNT_B3) as u8,
                    coff_idx: get(b, COFF_IDX_B3) as u8,
                    woff_idx: get(b, WOFF_IDX_B3) as u8,
                    f16: flag(b, F16_B4),
                    sched: get(b, SCHED_B4) as u8,
                    drc: get(b, DRC_B4) as u8,
                    sat: flag(b, SAT_B4),
                };
                Ok((Self::Ditr(d), 5))
            }
            op::ITRSMP => {
                need(6)?;
                let mut s = Itrsmp {
                    dest: get(b, DEST) as u8,
                    coff: get(b, COFF) as u8,
                    persp: get(b, P_ITR) as u8,
                    texoff: get(b, TEXOFF) as u8,
                    drc: get(b, DRC_B2) as u8,
                    mode: get(b, MODE_B2) as u8,
                    smpoff: get(b, SMPOFF) as u8,
                    dmn: get(b, DMN) as u8,
                    woff: get(b, WOFF_B4) as u8,
                    proj: flag(b, PROJ),
                    sat: flag(b, SAT_B4),
                    f16: flag(b, F16_B5),
                    sched: get(b, SCHED_B5) as u8,
                    chan: get(b, CHAN) as u8,
                    nncoords: flag(b, NNCOORDS),
                    fcnorm: flag(b, FCNORM),
                    ext: None,
                };
                if !flag(b, EXT5) {
                    return Ok((Self::Itrsmp(s), 6));
                }
                need(7)?;
                reserved(RSVD6, "iterate-and-sample")?;
                let mut e = ItrsmpExt {
                    count: get(b, ITR_COUNT_B6) as u8,
                    comparison: flag(b, COMPARISON),
                    idx: None,
                };
                let n = if flag(b, EXT6) {
                    need(8)?;
                    reserved(RSVD7, "iterate-and-sample")?;
                    let mode = get(b, MODE_2BIT) as u8;
                    e.idx = Some((get(b, COFF_IDX_B7) as u8, get(b, WOFF_IDX_B7) as u8, mode >> 1));
                    8
                } else {
                    7
                };
                s.ext = Some(e);
                Ok((Self::Itrsmp(s), n))
            }
            _ => Err(RogueError::Unsupported { at, what: "control op" }),
        }
    }

    /// The control op the header must name for this instruction.
    pub const fn ctrlop(&self) -> u8 {
        match self {
            Self::Branch { .. } => op::B,
            Self::Lapc => op::LAPC,
            Self::Savl => op::SAVL,
            Self::Wop => op::WOP,
            Self::Wdf => op::WDF,
            Self::Cnd { .. } => op::CND,
            Self::Mutex { .. } => op::MUTEX,
            Self::Nop => op::NOP,
            Self::Sbo { .. } => op::SBO,
            Self::Ditr(_) => op::DITR,
            Self::Itrsmp(_) => op::ITRSMP,
        }
    }

    /// Bytes the instruction takes.
    pub const fn size(&self) -> usize {
        match self {
            Self::Lapc | Self::Savl | Self::Wop | Self::Wdf => 0,
            Self::Cnd { .. } | Self::Mutex { .. } | Self::Nop => 1,
            Self::Sbo { .. } => 2,
            Self::Branch { .. } | Self::Ditr(_) => 5,
            Self::Itrsmp(s) => match s.ext {
                None => 6,
                Some(ItrsmpExt { idx: None, .. }) => 7,
                Some(_) => 8,
            },
        }
    }

    /// Writes the instruction into `out[..self.size()]`.
    pub fn encode(&self, out: &mut [u8]) -> Result<(), RogueError> {
        let n = self.size();
        let b = out.get_mut(..n).ok_or(RogueError::TooWide { what: "control word", value: 0 })?;
        b.fill(0);
        let u = |v: u8| u32::from(v);
        match *self {
            Self::Lapc | Self::Savl | Self::Wop | Self::Wdf | Self::Nop => Ok(()),
            Self::Branch { link, bpred, abs, offset } => {
                put(b, LINK, u32::from(link), "link")?;
                put(b, BPRED, u(bpred), "branch predicate")?;
                put(b, ABS, u32::from(abs), "absolute")?;
                put(b, BRANCH_OFFSET, offset, "branch offset")
            }
            Self::Cnd { adjust, pcnd, cndinst } => {
                put(b, ADJUST, u(adjust), "adjust")?;
                put(b, PCND, u(pcnd), "predicate condition")?;
                put(b, CNDINST, u(cndinst), "condition instruction")
            }
            Self::Mutex { lr, id } => {
                put(b, LR, u(lr), "mutex op")?;
                put(b, ID, u(id), "mutex id")
            }
            Self::Sbo { tgt, offset } => {
                put(b, TGT, u(tgt), "target")?;
                put(b, SBO_OFFSET, u(offset), "shared-buffer offset")
            }
            Self::Ditr(d) => {
                put(b, DEST, u(d.dest), "destination")?;
                put(b, COFF, u(d.coff), "coefficient offset")?;
                put(b, P_ITR, u(d.persp), "perspective")?;
                put(b, WOFF_B2, u(d.woff), "write offset")?;
                put(b, DITR_MODE, u(d.mode), "iterate mode")?;
                put(b, ITR_COUNT_B3, u(d.count), "iterate count")?;
                put(b, COFF_IDX_B3, u(d.coff_idx), "coefficient index control")?;
                put(b, WOFF_IDX_B3, u(d.woff_idx), "write index control")?;
                put(b, F16_B4, u32::from(d.f16), "f16")?;
                put(b, SCHED_B4, u(d.sched), "schedule control")?;
                put(b, DRC_B4, u(d.drc), "data return channel")?;
                put(b, SAT_B4, u32::from(d.sat), "saturate")
            }
            Self::Itrsmp(s) => {
                put(b, DEST, u(s.dest), "destination")?;
                put(b, COFF, u(s.coff), "coefficient offset")?;
                put(b, P_ITR, u(s.persp), "perspective")?;
                put(b, TEXOFF, u(s.texoff), "texture state offset")?;
                put(b, DRC_B2, u(s.drc), "data return channel")?;
                put(b, MODE_B2, u(s.mode), "iterate mode")?;
                put(b, SMPOFF, u(s.smpoff), "sampler state offset")?;
                put(b, DMN, u(s.dmn), "dimensions")?;
                put(b, WOFF_B4, u(s.woff), "write offset")?;
                put(b, PROJ, u32::from(s.proj), "projection")?;
                put(b, SAT_B4, u32::from(s.sat), "saturate")?;
                put(b, F16_B5, u32::from(s.f16), "f16")?;
                put(b, SCHED_B5, u(s.sched), "schedule control")?;
                put(b, CHAN, u(s.chan), "channels")?;
                put(b, NNCOORDS, u32::from(s.nncoords), "non-normalized coordinates")?;
                put(b, FCNORM, u32::from(s.fcnorm), "fixed-point normalize")?;
                if let Some(e) = s.ext {
                    put(b, EXT5, 1, "ext5")?;
                    put(b, ITR_COUNT_B6, u(e.count), "iterate count")?;
                    put(b, COMPARISON, u32::from(e.comparison), "comparison")?;
                    if let Some((ci, wi, hi)) = e.idx {
                        put(b, EXT6, 1, "ext6")?;
                        put(b, COFF_IDX_B7, u(ci), "coefficient index control")?;
                        put(b, WOFF_IDX_B7, u(wi), "write index control")?;
                        put(b, MODE_2BIT, (u(hi) << 1) | u(s.mode), "iterate mode")?;
                    }
                }
                Ok(())
            }
        }
    }
}
