// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The main ALU's instruction words: one byte, or two with the extension bit. The first three
//! bits name the op family; two families share a code and the phase tells them apart — fused
//! multiply-add (phases 0 and 1) and conditional move (phase 2's move), 32/64-bit integer
//! arithmetic (phase 0) and test (phase 2's test) — as do pack (phase 2) and unpack (phase 0)
//! under the single-source code. The 8/16-bit integer, gamma and reduction words are not in the
//! compositor's subset yet and are refused by name.

use super::bits::{flag, get, pc, put, Field};
use super::{Phase, RogueError};

const MAIN_OP: Field = &[pc(0, 7, 5)];
const EXT0: Field = &[pc(0, 4, 4)];
const SAT_FAM: Field = &[pc(0, 4, 4)];
const S0NEG_FAM: Field = &[pc(0, 3, 3)];
const S0ABS_FAM: Field = &[pc(0, 2, 2)];
const S1ABS_FAM: Field = &[pc(0, 1, 1)];
const S0FLR_FAM: Field = &[pc(0, 0, 0)];
const SNGL_OP: Field = &[pc(0, 3, 0)];
const RSVD1_SNGL: Field = &[pc(1, 7, 2)];
const S0NEG_SNGL: Field = &[pc(1, 1, 1)];
const S0ABS_SNGL: Field = &[pc(1, 0, 0)];
const PROG: Field = &[pc(1, 7, 7)];
const RTZ: Field = &[pc(1, 6, 6)];
const SCALE: Field = &[pc(1, 5, 5)];
const UPCK_ELEM: Field = &[pc(1, 7, 6)];
const PCK_FORMAT: Field = &[pc(1, 4, 0)];
const S0NEG_FMA: Field = &[pc(0, 3, 3)];
const S0ABS_FMA: Field = &[pc(0, 2, 2)];
const S2NEG_FMA: Field = &[pc(0, 1, 1)];
const SAT_FMA: Field = &[pc(0, 0, 0)];
const RSVD1_FMA: Field = &[pc(1, 7, 5)];
const LP_FMA: Field = &[pc(1, 4, 4)];
const S1ABS_FMA: Field = &[pc(1, 3, 3)];
const S1NEG_FMA: Field = &[pc(1, 2, 2)];
const S2FLR_FMA: Field = &[pc(1, 1, 1)];
const S2ABS_FMA: Field = &[pc(1, 0, 0)];
const S_I3264: Field = &[pc(0, 3, 3)];
const S2NEG_I3264: Field = &[pc(0, 2, 2)];
const INT32_64_OP: Field = &[pc(0, 1, 0)];
const RSVD1_I3264: Field = &[pc(1, 3, 3), pc(1, 7, 7)];
const CIN_I3264: Field = &[pc(1, 6, 6)];
const S1NEG_I3264: Field = &[pc(1, 5, 5)];
const S0NEG_I3264: Field = &[pc(1, 4, 4)];
const S0ABS_I3264: Field = &[pc(1, 2, 2)];
const S1ABS_I3264: Field = &[pc(1, 1, 1)];
const S2ABS_I3264: Field = &[pc(1, 0, 0)];
const MOVW1: Field = &[pc(0, 3, 2)];
const MOVW0: Field = &[pc(0, 1, 0)];
const RSVD1_MOVC: Field = &[pc(1, 7, 6)];
const MASKW0: Field = &[pc(1, 5, 2)];
const AW: Field = &[pc(1, 1, 1)];
const P2END_MOVC: Field = &[pc(1, 0, 0)];
const TST_OP3: Field = &[pc(0, 3, 1)];
const PWEN_TST: Field = &[pc(0, 0, 0)];
const TST_TYPE: Field = &[pc(1, 7, 5)];
const P2END_TST: Field = &[pc(1, 4, 4)];
const TST_ELEM: Field = &[pc(1, 3, 2)];
const RSVD1_TST: Field = &[pc(1, 1, 1)];
const TST_OP4: Field = &[pc(1, 0, 0), pc(0, 3, 1)];

/// The op families (the first three bits).
pub mod op {
    pub const FADD: u8 = 0;
    pub const FADD_LP: u8 = 1;
    pub const FMUL: u8 = 2;
    pub const FMUL_LP: u8 = 3;
    pub const SNGL: u8 = 4;
    pub const INT8_16: u8 = 5;
    pub const FMAD_MOVC: u8 = 6;
    pub const INT32_64_TST: u8 = 7;
}

/// The single-source ops.
pub mod sngl {
    pub const RCP: u8 = 0;
    pub const RSQ: u8 = 1;
    pub const LOG: u8 = 2;
    pub const EXP: u8 = 3;
    pub const GAMMA: u8 = 6;
    pub const BYP: u8 = 7;
    pub const PCK: u8 = 12;
    pub const RED: u8 = 13;
}

/// The pack formats this subset names.
pub mod pck {
    pub const U8888: u8 = 0;
    pub const F16F16: u8 = 14;
    pub const F32: u8 = 15;
    pub const COV: u8 = 16;
    pub const ZERO: u8 = 30;
    pub const ONE: u8 = 31;
}

/// The extended single-source word's modifiers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SrcMods {
    pub s0neg: bool,
    pub s0abs: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FmadExt {
    pub lp: bool,
    pub s1abs: bool,
    pub s1neg: bool,
    pub s2flr: bool,
    pub s2abs: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IntExt {
    pub cin: bool,
    pub s1neg: bool,
    pub s0neg: bool,
    pub s0abs: bool,
    pub s1abs: bool,
    pub s2abs: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MovcExt {
    pub maskw0: u8,
    pub aw: bool,
    pub p2end: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TstExt {
    pub ttype: u8,
    pub p2end: bool,
    pub elem: u8,
}

/// A main-ALU instruction word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AluInstr {
    /// Add or multiply, full or low precision (`op` one of [`op`]'s first four): one byte.
    Fam { op: u8, sat: bool, s0neg: bool, s0abs: bool, s1abs: bool, s0flr: bool },
    /// A single-source op ([`sngl`]); the extended form carries source modifiers.
    Sngl { op: u8, ext: Option<SrcMods> },
    /// Pack, phase 2 ([`pck`] formats).
    Pck { prog: bool, rtz: bool, scale: bool, format: u8 },
    /// Unpack, phase 0.
    Upck { elem: u8, scale_rtz: bool, format: u8 },
    /// Fused multiply-add, phases 0 and 1.
    Fmad { s0neg: bool, s0abs: bool, s2neg: bool, sat: bool, ext: Option<FmadExt> },
    /// 32/64-bit integer arithmetic, phase 0.
    Int3264 { s: bool, s2neg: bool, op: u8, ext: Option<IntExt> },
    /// Conditional move, phase 2's move.
    Movc { movw1: u8, movw0: u8, ext: Option<MovcExt> },
    /// Test, phase 2's test (`op` three bits brief, four extended).
    Tst { op: u8, pwen: bool, ext: Option<TstExt> },
}

impl AluInstr {
    /// The phase-2 slot a word in a main group's phase-2 area belongs to.
    pub fn p2_slot(b: &[u8]) -> Option<Phase> {
        match get(b, MAIN_OP) as u8 {
            op::FMAD_MOVC => Some(Phase::P2Mov),
            op::INT32_64_TST => Some(Phase::P2Tst),
            op::SNGL if get(b, SNGL_OP) as u8 == sngl::PCK => Some(Phase::P2Pck),
            _ => None,
        }
    }

    /// Decodes the word at the start of `b` in `phase`; its length in bytes.
    pub fn decode(b: &[u8], phase: Phase, at: usize) -> Result<(Self, usize), RogueError> {
        let need = |n: usize| if b.len() < n { Err(RogueError::Truncated { at }) } else { Ok(()) };
        need(1)?;
        let ext = flag(b, EXT0);
        let main = get(b, MAIN_OP) as u8;
        let i = match main {
            op::FADD..=op::FMUL_LP => {
                let i = Self::Fam {
                    op: main,
                    sat: flag(b, SAT_FAM),
                    s0neg: flag(b, S0NEG_FAM),
                    s0abs: flag(b, S0ABS_FAM),
                    s1abs: flag(b, S1ABS_FAM),
                    s0flr: flag(b, S0FLR_FAM),
                };
                return Ok((i, 1));
            }
            op::SNGL => {
                let s = get(b, SNGL_OP) as u8;
                if !ext {
                    if s == sngl::PCK {
                        return Err(RogueError::Malformed {
                            at,
                            what: "pack without its second byte",
                        });
                    }
                    return Ok((Self::Sngl { op: s, ext: None }, 1));
                }
                need(2)?;
                match (s, phase) {
                    (sngl::PCK, Phase::P2Pck) => Self::Pck {
                        prog: flag(b, PROG),
                        rtz: flag(b, RTZ),
                        scale: flag(b, SCALE),
                        format: get(b, PCK_FORMAT) as u8,
                    },
                    (sngl::PCK, Phase::P0) => Self::Upck {
                        elem: get(b, UPCK_ELEM) as u8,
                        scale_rtz: flag(b, SCALE),
                        format: get(b, PCK_FORMAT) as u8,
                    },
                    (sngl::PCK, _) => {
                        return Err(RogueError::Malformed {
                            at,
                            what: "pack outside phases 0 and 2",
                        })
                    }
                    (sngl::GAMMA, _) | (sngl::RED, _) => {
                        return Err(RogueError::Unsupported { at, what: "gamma or reduction word" })
                    }
                    _ => {
                        if get(b, RSVD1_SNGL) != 0 {
                            return Err(RogueError::Reserved { at, what: "single-source word" });
                        }
                        Self::Sngl {
                            op: s,
                            ext: Some(SrcMods {
                                s0neg: flag(b, S0NEG_SNGL),
                                s0abs: flag(b, S0ABS_SNGL),
                            }),
                        }
                    }
                }
            }
            op::INT8_16 => {
                return Err(RogueError::Unsupported { at, what: "8/16-bit integer word" })
            }
            op::FMAD_MOVC if phase == Phase::P2Mov => {
                let e = if ext {
                    need(2)?;
                    if get(b, RSVD1_MOVC) != 0 {
                        return Err(RogueError::Reserved { at, what: "movc word" });
                    }
                    Some(MovcExt {
                        maskw0: get(b, MASKW0) as u8,
                        aw: flag(b, AW),
                        p2end: flag(b, P2END_MOVC),
                    })
                } else {
                    None
                };
                Self::Movc { movw1: get(b, MOVW1) as u8, movw0: get(b, MOVW0) as u8, ext: e }
            }
            op::FMAD_MOVC => {
                let e = if ext {
                    need(2)?;
                    if get(b, RSVD1_FMA) != 0 {
                        return Err(RogueError::Reserved { at, what: "fmad word" });
                    }
                    Some(FmadExt {
                        lp: flag(b, LP_FMA),
                        s1abs: flag(b, S1ABS_FMA),
                        s1neg: flag(b, S1NEG_FMA),
                        s2flr: flag(b, S2FLR_FMA),
                        s2abs: flag(b, S2ABS_FMA),
                    })
                } else {
                    None
                };
                Self::Fmad {
                    s0neg: flag(b, S0NEG_FMA),
                    s0abs: flag(b, S0ABS_FMA),
                    s2neg: flag(b, S2NEG_FMA),
                    sat: flag(b, SAT_FMA),
                    ext: e,
                }
            }
            op::INT32_64_TST if phase == Phase::P2Tst => {
                if ext {
                    need(2)?;
                    if get(b, RSVD1_TST) != 0 {
                        return Err(RogueError::Reserved { at, what: "tst word" });
                    }
                    Self::Tst {
                        op: get(b, TST_OP4) as u8,
                        pwen: flag(b, PWEN_TST),
                        ext: Some(TstExt {
                            ttype: get(b, TST_TYPE) as u8,
                            p2end: flag(b, P2END_TST),
                            elem: get(b, TST_ELEM) as u8,
                        }),
                    }
                } else {
                    Self::Tst { op: get(b, TST_OP3) as u8, pwen: flag(b, PWEN_TST), ext: None }
                }
            }
            _ => {
                let e = if ext {
                    need(2)?;
                    if get(b, RSVD1_I3264) != 0 {
                        return Err(RogueError::Reserved { at, what: "int32/64 word" });
                    }
                    Some(IntExt {
                        cin: flag(b, CIN_I3264),
                        s1neg: flag(b, S1NEG_I3264),
                        s0neg: flag(b, S0NEG_I3264),
                        s0abs: flag(b, S0ABS_I3264),
                        s1abs: flag(b, S1ABS_I3264),
                        s2abs: flag(b, S2ABS_I3264),
                    })
                } else {
                    None
                };
                Self::Int3264 {
                    s: flag(b, S_I3264),
                    s2neg: flag(b, S2NEG_I3264),
                    op: get(b, INT32_64_OP) as u8,
                    ext: e,
                }
            }
        };
        Ok((i, if ext { 2 } else { 1 }))
    }

    /// Bytes the word takes.
    pub fn size(&self) -> usize {
        match self {
            Self::Fam { .. } => 1,
            Self::Pck { .. } | Self::Upck { .. } => 2,
            Self::Sngl { ext, .. } => 1 + usize::from(ext.is_some()),
            Self::Fmad { ext, .. } => 1 + usize::from(ext.is_some()),
            Self::Int3264 { ext, .. } => 1 + usize::from(ext.is_some()),
            Self::Movc { ext, .. } => 1 + usize::from(ext.is_some()),
            Self::Tst { ext, .. } => 1 + usize::from(ext.is_some()),
        }
    }

    /// Writes the word into `out[..self.size()]`.
    pub fn encode(&self, out: &mut [u8]) -> Result<(), RogueError> {
        let n = self.size();
        let b = out.get_mut(..n).ok_or(RogueError::TooWide { what: "alu word", value: 0 })?;
        b.fill(0);
        let set = |b: &mut [u8], f: Field, v: bool| put(b, f, u32::from(v), "alu flag");
        match *self {
            Self::Fam { op: o, sat, s0neg, s0abs, s1abs, s0flr } => {
                if o > op::FMUL_LP {
                    return Err(RogueError::TooWide {
                        what: "add/multiply op",
                        value: u32::from(o),
                    });
                }
                put(b, MAIN_OP, u32::from(o), "main op")?;
                set(b, SAT_FAM, sat)?;
                set(b, S0NEG_FAM, s0neg)?;
                set(b, S0ABS_FAM, s0abs)?;
                set(b, S1ABS_FAM, s1abs)?;
                set(b, S0FLR_FAM, s0flr)
            }
            Self::Sngl { op: s, ext } => {
                if matches!(s, sngl::PCK | sngl::GAMMA | sngl::RED) {
                    return Err(RogueError::Unsupported {
                        at: 0,
                        what: "single-source op with its own word",
                    });
                }
                put(b, MAIN_OP, u32::from(op::SNGL), "main op")?;
                put(b, SNGL_OP, u32::from(s), "single op")?;
                if let Some(m) = ext {
                    set(b, EXT0, true)?;
                    set(b, S0NEG_SNGL, m.s0neg)?;
                    set(b, S0ABS_SNGL, m.s0abs)?;
                }
                Ok(())
            }
            Self::Pck { prog, rtz, scale, format } => {
                put(b, MAIN_OP, u32::from(op::SNGL), "main op")?;
                set(b, EXT0, true)?;
                put(b, SNGL_OP, u32::from(sngl::PCK), "single op")?;
                set(b, PROG, prog)?;
                set(b, RTZ, rtz)?;
                set(b, SCALE, scale)?;
                put(b, PCK_FORMAT, u32::from(format), "pack format")
            }
            Self::Upck { elem, scale_rtz, format } => {
                put(b, MAIN_OP, u32::from(op::SNGL), "main op")?;
                set(b, EXT0, true)?;
                put(b, SNGL_OP, u32::from(sngl::PCK), "single op")?;
                put(b, UPCK_ELEM, u32::from(elem), "unpack element")?;
                set(b, SCALE, scale_rtz)?;
                put(b, PCK_FORMAT, u32::from(format), "pack format")
            }
            Self::Fmad { s0neg, s0abs, s2neg, sat, ext } => {
                put(b, MAIN_OP, u32::from(op::FMAD_MOVC), "main op")?;
                set(b, S0NEG_FMA, s0neg)?;
                set(b, S0ABS_FMA, s0abs)?;
                set(b, S2NEG_FMA, s2neg)?;
                set(b, SAT_FMA, sat)?;
                if let Some(e) = ext {
                    set(b, EXT0, true)?;
                    set(b, LP_FMA, e.lp)?;
                    set(b, S1ABS_FMA, e.s1abs)?;
                    set(b, S1NEG_FMA, e.s1neg)?;
                    set(b, S2FLR_FMA, e.s2flr)?;
                    set(b, S2ABS_FMA, e.s2abs)?;
                }
                Ok(())
            }
            Self::Int3264 { s, s2neg, op: o, ext } => {
                put(b, MAIN_OP, u32::from(op::INT32_64_TST), "main op")?;
                set(b, S_I3264, s)?;
                set(b, S2NEG_I3264, s2neg)?;
                put(b, INT32_64_OP, u32::from(o), "int op")?;
                if let Some(e) = ext {
                    set(b, EXT0, true)?;
                    set(b, CIN_I3264, e.cin)?;
                    set(b, S1NEG_I3264, e.s1neg)?;
                    set(b, S0NEG_I3264, e.s0neg)?;
                    set(b, S0ABS_I3264, e.s0abs)?;
                    set(b, S1ABS_I3264, e.s1abs)?;
                    set(b, S2ABS_I3264, e.s2abs)?;
                }
                Ok(())
            }
            Self::Movc { movw1, movw0, ext } => {
                put(b, MAIN_OP, u32::from(op::FMAD_MOVC), "main op")?;
                put(b, MOVW1, u32::from(movw1), "movw1")?;
                put(b, MOVW0, u32::from(movw0), "movw0")?;
                if let Some(e) = ext {
                    set(b, EXT0, true)?;
                    put(b, MASKW0, u32::from(e.maskw0), "maskw0")?;
                    set(b, AW, e.aw)?;
                    set(b, P2END_MOVC, e.p2end)?;
                }
                Ok(())
            }
            Self::Tst { op: o, pwen, ext } => {
                put(b, MAIN_OP, u32::from(op::INT32_64_TST), "main op")?;
                set(b, PWEN_TST, pwen)?;
                match ext {
                    None => put(b, TST_OP3, u32::from(o), "test op"),
                    Some(e) => {
                        set(b, EXT0, true)?;
                        put(b, TST_OP4, u32::from(o), "test op")?;
                        put(b, TST_TYPE, u32::from(e.ttype), "test type")?;
                        set(b, P2END_TST, e.p2end)?;
                        put(b, TST_ELEM, u32::from(e.elem), "test element")
                    }
                }
            }
        }
    }
}
