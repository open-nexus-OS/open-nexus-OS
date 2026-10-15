// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The backend's instruction words: the execution-mask save and move, the visibility test
//! (the ISP feedback of a punch-through program), the iterator, and the texture sample. The
//! vertex-output, phase, store, load and atomic words are not in the compositor's subset yet and
//! are refused by name; so is the sample's second extension, whose fields the description places
//! past a three-byte word.

use super::bits::{flag, get, pc, put, Field};
use super::RogueError;

const BACKEND_OP: Field = &[pc(0, 7, 5)];
const MSK_OP: Field = &[pc(0, 4, 4)];
const SM: Field = &[pc(0, 3, 3)];
const MSK_MODE: Field = &[pc(0, 2, 0)];
const RSVD1: Field = &[pc(1, 7, 3)];
const SRCSEL: Field = &[pc(1, 2, 0)];
const RSVD0_VISTEST: Field = &[pc(0, 4, 3)];
const PWEN_VISTEST: Field = &[pc(0, 2, 2)];
const VISTEST_OP: Field = &[pc(0, 1, 1)];
const IFB: Field = &[pc(0, 0, 0)];
const P_FITR: Field = &[pc(0, 4, 4)];
const DRC: Field = &[pc(0, 3, 3)];
const RSVD0_FITR: Field = &[pc(0, 2, 2)];
const ITER_MODE: Field = &[pc(0, 1, 0)];
const RSVD1_FITR: Field = &[pc(1, 7, 5)];
const SAT_FITR: Field = &[pc(1, 4, 4)];
const COUNT_FITR: Field = &[pc(1, 3, 0)];
const FCNORM: Field = &[pc(0, 4, 4)];
const DMA_OP: Field = &[pc(0, 2, 0)];
const EXTB: Field = &[pc(1, 7, 7)];
const DMN: Field = &[pc(1, 6, 5)];
const EXTA: Field = &[pc(1, 4, 4)];
const CHAN: Field = &[pc(1, 3, 2)];
const LODM: Field = &[pc(1, 1, 0)];
const PPLOD: Field = &[pc(2, 7, 7)];
const PROJ: Field = &[pc(2, 6, 6)];
const SBMODE: Field = &[pc(2, 5, 4)];
const NNCOORDS: Field = &[pc(2, 3, 3)];
const SNO: Field = &[pc(2, 2, 2)];
const SOO: Field = &[pc(2, 1, 1)];
const TAO: Field = &[pc(2, 0, 0)];

/// The backend ops (the first three bits).
pub mod op {
    pub const UVSW: u8 = 0;
    pub const MSK: u8 = 1;
    pub const PHAS: u8 = 2;
    pub const SETL: u8 = 3;
    pub const VISTEST: u8 = 4;
    pub const FITR: u8 = 5;
    pub const EMIT: u8 = 6;
    pub const DMA: u8 = 7;
}

/// The DMA ops.
const DMA_SMP: u32 = 4;
/// The mask move's only mode.
const MSK_MODE_ICM: u8 = 1;

/// The sample's first extension.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SmpExtA {
    pub pplod: bool,
    pub proj: bool,
    pub sbmode: u8,
    pub nncoords: bool,
    pub sno: bool,
    pub soo: bool,
    pub tao: bool,
}

/// A backend instruction word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendInstr {
    /// Save the execution mask (`mode`: vm, icm, icmoc, icmi, caxy).
    SavMsk { mode: u8 },
    /// Move the execution mask; with a source select in its second byte.
    MovMsk { srcsel: Option<u8> },
    /// The visibility test: depth feedback, or alpha test (the punch-through feedback).
    VisTest { atst: bool, pwen: bool, ifb: bool },
    /// Iterate varyings into registers.
    Fitr { persp: u8, drc: u8, mode: u8, sat: bool, count: u8 },
    /// Sample a texture (`dmn` 1–3 dimensions, `lodm` the level-of-detail mode).
    Smp { fcnorm: bool, drc: u8, dmn: u8, chan: u8, lodm: u8, exta: Option<SmpExtA> },
}

impl BackendInstr {
    /// Decodes the word at the start of `b`; its length in bytes.
    pub fn decode(b: &[u8], at: usize) -> Result<(Self, usize), RogueError> {
        let need = |n: usize| if b.len() < n { Err(RogueError::Truncated { at }) } else { Ok(()) };
        need(1)?;
        match get(b, BACKEND_OP) as u8 {
            op::MSK => {
                let mode = get(b, MSK_MODE) as u8;
                if get(b, MSK_OP) == 0 {
                    if flag(b, SM) {
                        return Err(RogueError::Malformed {
                            at,
                            what: "mask save with a source select",
                        });
                    }
                    return Ok((Self::SavMsk { mode }, 1));
                }
                if mode != MSK_MODE_ICM {
                    return Err(RogueError::Malformed { at, what: "mask move mode" });
                }
                if !flag(b, SM) {
                    return Ok((Self::MovMsk { srcsel: None }, 1));
                }
                need(2)?;
                if get(b, RSVD1) != 0 {
                    return Err(RogueError::Reserved { at, what: "mask move" });
                }
                Ok((Self::MovMsk { srcsel: Some(get(b, SRCSEL) as u8) }, 2))
            }
            op::VISTEST => {
                if get(b, RSVD0_VISTEST) != 0 {
                    return Err(RogueError::Reserved { at, what: "visibility test" });
                }
                let (atst, pwen, ifb) = (flag(b, VISTEST_OP), flag(b, PWEN_VISTEST), flag(b, IFB));
                if !atst && (pwen || ifb) {
                    return Err(RogueError::Malformed {
                        at,
                        what: "depth feedback with test flags",
                    });
                }
                Ok((Self::VisTest { atst, pwen, ifb }, 1))
            }
            op::FITR => {
                need(2)?;
                if get(b, RSVD0_FITR) != 0 || get(b, RSVD1_FITR) != 0 {
                    return Err(RogueError::Reserved { at, what: "iterate" });
                }
                let i = Self::Fitr {
                    persp: get(b, P_FITR) as u8,
                    drc: get(b, DRC) as u8,
                    mode: get(b, ITER_MODE) as u8,
                    sat: flag(b, SAT_FITR),
                    count: get(b, COUNT_FITR) as u8,
                };
                Ok((i, 2))
            }
            op::DMA if get(b, DMA_OP) == DMA_SMP => {
                need(2)?;
                if flag(b, EXTB) {
                    return Err(RogueError::Unsupported {
                        at,
                        what: "sample word's second extension",
                    });
                }
                let exta = if flag(b, EXTA) {
                    need(3)?;
                    Some(SmpExtA {
                        pplod: flag(b, PPLOD),
                        proj: flag(b, PROJ),
                        sbmode: get(b, SBMODE) as u8,
                        nncoords: flag(b, NNCOORDS),
                        sno: flag(b, SNO),
                        soo: flag(b, SOO),
                        tao: flag(b, TAO),
                    })
                } else {
                    None
                };
                let i = Self::Smp {
                    fcnorm: flag(b, FCNORM),
                    drc: get(b, DRC) as u8,
                    dmn: get(b, DMN) as u8,
                    chan: get(b, CHAN) as u8,
                    lodm: get(b, LODM) as u8,
                    exta,
                };
                Ok((i, if exta.is_some() { 3 } else { 2 }))
            }
            _ => Err(RogueError::Unsupported { at, what: "backend op" }),
        }
    }

    /// Bytes the word takes.
    pub fn size(&self) -> usize {
        match self {
            Self::SavMsk { .. } | Self::VisTest { .. } => 1,
            Self::MovMsk { srcsel } => 1 + usize::from(srcsel.is_some()),
            Self::Fitr { .. } => 2,
            Self::Smp { exta, .. } => 2 + usize::from(exta.is_some()),
        }
    }

    /// Writes the word into `out[..self.size()]`.
    pub fn encode(&self, out: &mut [u8]) -> Result<(), RogueError> {
        let n = self.size();
        let b = out.get_mut(..n).ok_or(RogueError::TooWide { what: "backend word", value: 0 })?;
        b.fill(0);
        let set = |b: &mut [u8], f: Field, v: bool| put(b, f, u32::from(v), "backend flag");
        match *self {
            Self::SavMsk { mode } => {
                put(b, BACKEND_OP, u32::from(op::MSK), "backend op")?;
                put(b, MSK_MODE, u32::from(mode), "mask mode")
            }
            Self::MovMsk { srcsel } => {
                put(b, BACKEND_OP, u32::from(op::MSK), "backend op")?;
                put(b, MSK_OP, 1, "mask op")?;
                put(b, MSK_MODE, u32::from(MSK_MODE_ICM), "mask mode")?;
                if let Some(s) = srcsel {
                    set(b, SM, true)?;
                    put(b, SRCSEL, u32::from(s), "source select")?;
                }
                Ok(())
            }
            Self::VisTest { atst, pwen, ifb } => {
                if !atst && (pwen || ifb) {
                    return Err(RogueError::Malformed {
                        at: 0,
                        what: "depth feedback with test flags",
                    });
                }
                put(b, BACKEND_OP, u32::from(op::VISTEST), "backend op")?;
                set(b, VISTEST_OP, atst)?;
                set(b, PWEN_VISTEST, pwen)?;
                set(b, IFB, ifb)
            }
            Self::Fitr { persp, drc, mode, sat, count } => {
                put(b, BACKEND_OP, u32::from(op::FITR), "backend op")?;
                put(b, P_FITR, u32::from(persp), "perspective")?;
                put(b, DRC, u32::from(drc), "data return channel")?;
                put(b, ITER_MODE, u32::from(mode), "iterate mode")?;
                set(b, SAT_FITR, sat)?;
                put(b, COUNT_FITR, u32::from(count), "iterate count")
            }
            Self::Smp { fcnorm, drc, dmn, chan, lodm, exta } => {
                put(b, BACKEND_OP, u32::from(op::DMA), "backend op")?;
                put(b, DMA_OP, DMA_SMP, "dma op")?;
                set(b, FCNORM, fcnorm)?;
                put(b, DRC, u32::from(drc), "data return channel")?;
                put(b, DMN, u32::from(dmn), "dimensions")?;
                put(b, CHAN, u32::from(chan), "channels")?;
                put(b, LODM, u32::from(lodm), "lod mode")?;
                if let Some(e) = exta {
                    set(b, EXTA, true)?;
                    set(b, PPLOD, e.pplod)?;
                    set(b, PROJ, e.proj)?;
                    put(b, SBMODE, u32::from(e.sbmode), "sample-back mode")?;
                    set(b, NNCOORDS, e.nncoords)?;
                    set(b, SNO, e.sno)?;
                    set(b, SOO, e.soo)?;
                    set(b, TAO, e.tao)?;
                }
                Ok(())
            }
        }
    }
}
