// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The setup programs (the PDS): 32-bit instructions that run before a shader-core program and
//! feed it — kick it (`doutu`), iterate its varyings into the coefficient store (`douti`), DMA
//! constants and state into its registers (`doutd`, and `ddmad` with a stride times an index for
//! vertex attributes) — reading 32- and 64-bit constants from the program's data segment. A
//! program is its data segment, aligned to 16 bytes, then its code. This module encodes the
//! instructions and the constants' layouts the compositor's pipelines need; the program shapes
//! (the kick, the iterate, the state load, the vertex fetch) are built from them by the backend.

use super::RogueError;

/// The opcode classes (bits 31–28).
mod opc {
    pub const ADD64: u32 = 0x8;
    pub const ADD32: u32 = 0x9;
    pub const SP: u32 = 0xd;
    pub const DDMAD: u32 = 0xe;
    pub const DOUT: u32 = 0xf;
}
const SP_HALT: u32 = 6;

/// Where a `dout` writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DoutDst {
    /// DMA from memory into the shader core's registers.
    D = 0,
    /// Write constants into registers.
    W = 1,
    /// Kick the shader core.
    U = 2,
    V = 3,
    /// Iterate varyings into the coefficient store.
    I = 4,
    C = 5,
}

impl DoutDst {
    fn from_bits(v: u32) -> Option<Self> {
        Some(match v {
            0 => Self::D,
            1 => Self::W,
            2 => Self::U,
            3 => Self::V,
            4 => Self::I,
            5 => Self::C,
            _ => return None,
        })
    }
}

/// A setup-program instruction. Register operands are the instruction's fields: a 64-bit
/// constant `n` is `n` (32-bit constants `2n` and `2n+1`), temporaries start at 64 (64-bit) and
/// 128 (32-bit).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PdsInstr {
    /// Output `src0` (64-bit) and `src1` (32-bit) to `dst`; `end` halts after it.
    Dout {
        cc: bool,
        end: bool,
        src1: u8,
        src0: u8,
        dst: DoutDst,
    },
    /// DMA `src3`'s control at `src2 + src0 * src1` (32-bit stride times a temporary index).
    Ddmad {
        cc: bool,
        end: bool,
        src0: u8,
        src1: u8,
        src2: u8,
        src3: u8,
    },
    Add32 {
        cc: bool,
        alum: bool,
        sna: bool,
        src0: u8,
        src1: u8,
        dst: u8,
    },
    Add64 {
        cc: bool,
        alum: bool,
        sna: bool,
        src0: u8,
        src1: u8,
        dst: u8,
    },
    Halt {
        cc: bool,
    },
}

fn field(w: u32, shift: u32, bits: u32) -> u32 {
    (w >> shift) & ((1 << bits) - 1)
}

fn fits(v: u8, bits: u32, what: &'static str) -> Result<u32, RogueError> {
    let v = u32::from(v);
    if v >> bits != 0 {
        return Err(RogueError::TooWide { what, value: v });
    }
    Ok(v)
}

impl PdsInstr {
    /// Decodes one instruction word (`at`: its offset, for the error).
    pub fn decode(w: u32, at: usize) -> Result<Self, RogueError> {
        let cc = field(w, 27, 1) != 0;
        let reserved = |mask: u32| {
            if w & mask != 0 {
                Err(RogueError::Reserved { at, what: "setup instruction" })
            } else {
                Ok(())
            }
        };
        Ok(match w >> 28 {
            opc::DOUT => {
                reserved((1 << 25) | (1 << 24) | (1 << 15) | 0xf8)?;
                let dst = DoutDst::from_bits(field(w, 0, 3))
                    .ok_or(RogueError::Unsupported { at, what: "dout destination" })?;
                Self::Dout {
                    cc,
                    end: field(w, 26, 1) != 0,
                    src1: field(w, 16, 8) as u8,
                    src0: field(w, 8, 7) as u8,
                    dst,
                }
            }
            opc::DDMAD => Self::Ddmad {
                cc,
                end: field(w, 26, 1) != 0,
                src0: field(w, 18, 8) as u8,
                src1: field(w, 13, 5) as u8,
                src2: field(w, 6, 7) as u8,
                src3: field(w, 0, 6) as u8,
            },
            opc::ADD32 => {
                reserved((1 << 25) | (1 << 23) | (1 << 22))?;
                Self::Add32 {
                    cc,
                    alum: field(w, 26, 1) != 0,
                    sna: field(w, 24, 1) != 0,
                    src0: field(w, 14, 8) as u8,
                    src1: field(w, 6, 8) as u8,
                    dst: field(w, 0, 6) as u8,
                }
            }
            opc::ADD64 => {
                reserved((1 << 25) | (0x1f << 19))?;
                Self::Add64 {
                    cc,
                    alum: field(w, 26, 1) != 0,
                    sna: field(w, 24, 1) != 0,
                    src0: field(w, 12, 7) as u8,
                    src1: field(w, 5, 7) as u8,
                    dst: field(w, 0, 5) as u8,
                }
            }
            opc::SP if field(w, 23, 4) == SP_HALT => {
                reserved(0x007f_ffff)?;
                Self::Halt { cc }
            }
            _ => return Err(RogueError::Unsupported { at, what: "setup instruction" }),
        })
    }

    /// The instruction word.
    pub fn encode(&self) -> Result<u32, RogueError> {
        let b = |v: bool| u32::from(v);
        Ok(match *self {
            Self::Dout { cc, end, src1, src0, dst } => {
                (opc::DOUT << 28)
                    | (b(cc) << 27)
                    | (b(end) << 26)
                    | (fits(src1, 8, "dout src1")? << 16)
                    | (fits(src0, 7, "dout src0")? << 8)
                    | dst as u32
            }
            Self::Ddmad { cc, end, src0, src1, src2, src3 } => {
                (opc::DDMAD << 28)
                    | (b(cc) << 27)
                    | (b(end) << 26)
                    | (fits(src0, 8, "ddmad src0")? << 18)
                    | (fits(src1, 5, "ddmad src1")? << 13)
                    | (fits(src2, 7, "ddmad src2")? << 6)
                    | fits(src3, 6, "ddmad src3")?
            }
            Self::Add32 { cc, alum, sna, src0, src1, dst } => {
                (opc::ADD32 << 28)
                    | (b(cc) << 27)
                    | (b(alum) << 26)
                    | (b(sna) << 24)
                    | (fits(src0, 8, "add32 src0")? << 14)
                    | (fits(src1, 8, "add32 src1")? << 6)
                    | fits(dst, 6, "add32 dst")?
            }
            Self::Add64 { cc, alum, sna, src0, src1, dst } => {
                (opc::ADD64 << 28)
                    | (b(cc) << 27)
                    | (b(alum) << 26)
                    | (b(sna) << 24)
                    | (fits(src0, 7, "add64 src0")? << 12)
                    | (fits(src1, 7, "add64 src1")? << 5)
                    | fits(dst, 5, "add64 dst")?
            }
            Self::Halt { cc } => (opc::SP << 28) | (b(cc) << 27) | (SP_HALT << 23),
        })
    }
}

/// The shader-core kick's constant (`doutu` src0, 64-bit).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Doutu {
    /// The program's byte offset from the code heap's base (a multiple of 4).
    pub exe_off: u32,
    /// Temporaries in units of four.
    pub temps: u8,
    pub sample_rate: u8,
    pub dual_phase: bool,
}

impl Doutu {
    pub fn decode(v: u64) -> Self {
        Self {
            exe_off: (v as u32) & !3,
            temps: ((v >> 35) & 0x3f) as u8,
            sample_rate: ((v >> 33) & 3) as u8,
            dual_phase: (v >> 41) & 1 != 0,
        }
    }

    pub fn encode(&self) -> Result<u64, RogueError> {
        if self.exe_off & 3 != 0 || self.temps > 0x3f || self.sample_rate > 3 {
            return Err(RogueError::TooWide { what: "doutu word", value: self.exe_off });
        }
        Ok(u64::from(self.exe_off)
            | (u64::from(self.sample_rate) << 33)
            | (u64::from(self.temps) << 35)
            | (u64::from(self.dual_phase) << 41))
    }
}

/// One varying's iteration (`douti` src0, 64-bit): the issue fields, the destination in the
/// coefficient store (128-bit words), and whether this is the triangle's last issue.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Douti {
    pub depthbias: bool,
    pub primitiveid: bool,
    /// 0–2 flat from a vertex, 3 interpolated.
    pub shademodel: u8,
    pub pointsprite: bool,
    pub wraps: bool,
    pub wrapv: bool,
    pub wrapu: bool,
    /// Components minus one.
    pub size: u8,
    pub f16: bool,
    pub perspective: bool,
    /// The varying's offset in the vertex outputs (32-bit and 16-bit).
    pub f32_offset: u8,
    pub f16_offset: u8,
    pub dest: u8,
    pub last: bool,
}

impl Douti {
    pub fn decode(v: u64, at: usize) -> Result<Self, RogueError> {
        if v & 0x4000_0000_f000_0000 != 0 || (v >> 32) & 0x003f_ffff != 0 {
            return Err(RogueError::Reserved { at, what: "douti word" });
        }
        let bit = |n: u32| (v >> n) & 1 != 0;
        Ok(Self {
            depthbias: bit(27),
            primitiveid: bit(26),
            shademodel: ((v >> 24) & 3) as u8,
            pointsprite: bit(23),
            wraps: bit(22),
            wrapv: bit(21),
            wrapu: bit(20),
            size: ((v >> 18) & 3) as u8,
            f16: bit(17),
            perspective: bit(16),
            f32_offset: ((v >> 8) & 0xff) as u8,
            f16_offset: (v & 0xff) as u8,
            dest: ((v >> 54) & 0xff) as u8,
            last: bit(63),
        })
    }

    pub fn encode(&self) -> Result<u64, RogueError> {
        if self.shademodel > 3 || self.size > 3 {
            return Err(RogueError::TooWide {
                what: "douti word",
                value: u32::from(self.shademodel.max(self.size)),
            });
        }
        let b = |v: bool, n: u32| u64::from(v) << n;
        Ok(b(self.depthbias, 27)
            | b(self.primitiveid, 26)
            | (u64::from(self.shademodel) << 24)
            | b(self.pointsprite, 23)
            | b(self.wraps, 22)
            | b(self.wrapv, 21)
            | b(self.wrapu, 20)
            | (u64::from(self.size) << 18)
            | b(self.f16, 17)
            | b(self.perspective, 16)
            | (u64::from(self.f32_offset) << 8)
            | u64::from(self.f16_offset)
            | (u64::from(self.dest) << 54)
            | b(self.last, 63))
    }
}

/// A DMA's control (`doutd` src1, 32-bit; `ddmad` src3's low word): `a0` the first destination
/// register, `bsize` the dwords, `dest` 0 the unified store (temporaries), 1 the common store
/// (shared registers); `last` marks the program's last DMA.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DmaCtl {
    pub last: bool,
    pub wordsize: u8,
    pub dest: u8,
    pub cmode: u8,
    pub a0: u16,
    pub repeat: bool,
    pub bsize: u16,
}

impl DmaCtl {
    pub fn decode(v: u32) -> Self {
        Self {
            last: v >> 31 != 0,
            wordsize: ((v >> 29) & 3) as u8,
            dest: ((v >> 28) & 1) as u8,
            cmode: ((v >> 26) & 3) as u8,
            a0: ((v >> 13) & 0x1fff) as u16,
            repeat: (v >> 12) & 1 != 0,
            bsize: (v & 0xfff) as u16,
        }
    }

    pub fn encode(&self) -> Result<u32, RogueError> {
        if self.wordsize > 3
            || self.dest > 1
            || self.cmode > 3
            || self.a0 > 0x1fff
            || self.bsize > 0xfff
        {
            return Err(RogueError::TooWide { what: "dma control", value: u32::from(self.a0) });
        }
        Ok((u32::from(self.last) << 31)
            | (u32::from(self.wordsize) << 29)
            | (u32::from(self.dest) << 28)
            | (u32::from(self.cmode) << 26)
            | (u32::from(self.a0) << 13)
            | (u32::from(self.repeat) << 12)
            | u32::from(self.bsize))
    }
}

/// A DMA's source (`doutd` src0, 64-bit): a 40-bit address and a destination offset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DmaSrc {
    pub sbase: u64,
    pub doffset: u16,
}

impl DmaSrc {
    pub fn decode(v: u64, at: usize) -> Result<Self, RogueError> {
        if v >> 53 != 0 {
            return Err(RogueError::Reserved { at, what: "dma source" });
        }
        Ok(Self { sbase: v & 0xff_ffff_ffff, doffset: ((v >> 40) & 0x1fff) as u16 })
    }

    pub fn encode(&self) -> Result<u64, RogueError> {
        if self.sbase >> 40 != 0 || self.doffset > 0x1fff {
            return Err(RogueError::TooWide { what: "dma source", value: self.doffset.into() });
        }
        Ok(self.sbase | (u64::from(self.doffset) << 40))
    }
}

/// A setup program: its data segment (dwords) and its code. The code starts at the data
/// segment's size rounded up to 16 bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PdsProgram {
    pub data: alloc::vec::Vec<u32>,
    pub code: alloc::vec::Vec<PdsInstr>,
}

impl PdsProgram {
    /// The code's offset in bytes from the program's start.
    pub fn code_offset(&self) -> usize {
        (self.data.len() * 4).div_ceil(16) * 16
    }

    /// The program as bytes: data, zero up to the code, code.
    pub fn encode(&self) -> Result<alloc::vec::Vec<u8>, RogueError> {
        let mut out = alloc::vec::Vec::with_capacity(self.code_offset() + self.code.len() * 4);
        for d in &self.data {
            out.extend_from_slice(&d.to_le_bytes());
        }
        out.resize(self.code_offset(), 0);
        for i in &self.code {
            out.extend_from_slice(&i.encode()?.to_le_bytes());
        }
        Ok(out)
    }
}
