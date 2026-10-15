// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The instruction group's header: two bytes for a plain main-ALU group, three for one that
//! ends the program, repeats, is atomic or carries a two-bit condition, and for every bitwise
//! and control group. It names the phases the group carries, whether it writes through the two
//! write ports, its length in 16-bit words and the decode-assist offset (where the phase-0 and
//! phase-1 words begin in a main group, where the inputs begin in the others).

use super::bits::{flag, get, pc, put, Field};
use super::RogueError;

const DA: Field = &[pc(0, 7, 4)];
const LENGTH: Field = &[pc(0, 3, 0)];
const EXT: Field = &[pc(1, 7, 7)];
const OPS: Field = &[pc(1, 6, 4)];
const OLCHK: Field = &[pc(1, 3, 3)];
const W1P: Field = &[pc(1, 2, 2)];
const W0P: Field = &[pc(1, 1, 1)];
const CC1: Field = &[pc(1, 0, 0)];
const END: Field = &[pc(2, 7, 7)];
const MISCCTL: Field = &[pc(2, 7, 7)];
const ALUTYPE: Field = &[pc(2, 6, 5)];
const RSVD: Field = &[pc(2, 4, 4)];
const ATOM: Field = &[pc(2, 3, 3)];
const RPT: Field = &[pc(2, 2, 1)];
const CTRLOP: Field = &[pc(2, 4, 1)];
const CC: Field = &[pc(2, 0, 0), pc(1, 0, 0)];

/// The unit a group drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AluType {
    Main,
    Bitwise,
    Control,
}

/// The phases of a main group (`oporg`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum OpOrg {
    P0 = 0,
    P2 = 1,
    Be = 2,
    P0P1 = 3,
    P0P2 = 4,
    P0P1P2 = 5,
    P0P2Be = 6,
    P0P1P2Be = 7,
}

impl OpOrg {
    pub const fn from_bits(v: u8) -> Self {
        match v & 7 {
            0 => Self::P0,
            1 => Self::P2,
            2 => Self::Be,
            3 => Self::P0P1,
            4 => Self::P0P2,
            5 => Self::P0P1P2,
            6 => Self::P0P2Be,
            _ => Self::P0P1P2Be,
        }
    }
    pub const fn has_p0(self) -> bool {
        !matches!(self, Self::P2 | Self::Be)
    }
    pub const fn has_p1(self) -> bool {
        matches!(self, Self::P0P1 | Self::P0P1P2 | Self::P0P1P2Be)
    }
    pub const fn has_p2(self) -> bool {
        matches!(self, Self::P2 | Self::P0P2 | Self::P0P1P2 | Self::P0P2Be | Self::P0P1P2Be)
    }
    pub const fn has_backend(self) -> bool {
        matches!(self, Self::Be | Self::P0P2Be | Self::P0P1P2Be)
    }
}

/// The bitwise unit's phases (`opcnt`, a mask).
pub mod opcnt {
    pub const P0: u8 = 1;
    pub const P1: u8 = 2;
    pub const P2: u8 = 4;
}

/// A group header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub alutype: AluType,
    /// Main only: the two-byte form (no end, atomic, repeat or wide condition).
    pub brief: bool,
    /// Decode assist, in bytes (0 for a control group without inputs or outputs).
    pub da: u8,
    /// The group's length in 16-bit words, padding included.
    pub length: u8,
    /// Main: [`OpOrg`] bits; bitwise: an [`opcnt`] mask; control: 0.
    pub ops: u8,
    pub olchk: bool,
    pub w1p: bool,
    pub w0p: bool,
    /// The execution condition (one bit in the brief form).
    pub cc: u8,
    pub end: bool,
    pub atom: bool,
    pub rpt: u8,
    /// Control only.
    pub miscctl: bool,
    pub ctrlop: u8,
}

impl Header {
    /// A blank header of a unit; the group fills the rest.
    pub const fn new(alutype: AluType, ops: u8) -> Self {
        Self {
            alutype,
            brief: matches!(alutype, AluType::Main),
            da: 0,
            length: 0,
            ops,
            olchk: false,
            w1p: false,
            w0p: false,
            cc: 0,
            end: false,
            atom: false,
            rpt: 0,
            miscctl: false,
            ctrlop: 0,
        }
    }

    /// Bytes the header takes.
    pub const fn size(&self) -> usize {
        if self.brief {
            2
        } else {
            3
        }
    }

    /// Decodes the header at the start of `b`.
    pub fn decode(b: &[u8], at: usize) -> Result<Self, RogueError> {
        if b.len() < 2 {
            return Err(RogueError::Truncated { at });
        }
        let mut h = Self::new(AluType::Main, get(b, OPS) as u8);
        h.da = get(b, DA) as u8;
        h.length = get(b, LENGTH) as u8;
        h.olchk = flag(b, OLCHK);
        h.w1p = flag(b, W1P);
        h.w0p = flag(b, W0P);
        if !flag(b, EXT) {
            h.cc = get(b, CC1) as u8;
            return Ok(h);
        }
        if b.len() < 3 {
            return Err(RogueError::Truncated { at });
        }
        h.brief = false;
        h.cc = get(b, CC) as u8;
        match get(b, ALUTYPE) {
            0 | 2 => {
                h.alutype = if get(b, ALUTYPE) == 0 { AluType::Main } else { AluType::Bitwise };
                if get(b, RSVD) != 0 {
                    return Err(RogueError::Reserved { at, what: "header" });
                }
                h.end = flag(b, END);
                h.atom = flag(b, ATOM);
                h.rpt = get(b, RPT) as u8;
            }
            3 => {
                h.alutype = AluType::Control;
                if h.ops != 0 {
                    return Err(RogueError::Reserved { at, what: "control header opcnt" });
                }
                h.miscctl = flag(b, MISCCTL);
                h.ctrlop = get(b, CTRLOP) as u8;
            }
            _ => return Err(RogueError::Malformed { at, what: "header alutype" }),
        }
        Ok(h)
    }

    /// Writes the header into `out[..self.size()]`.
    pub fn encode(&self, out: &mut [u8]) -> Result<(), RogueError> {
        let n = self.size();
        let out = out.get_mut(..n).ok_or(RogueError::TooWide { what: "header", value: 0 })?;
        out.fill(0);
        put(out, DA, u32::from(self.da), "header da")?;
        put(out, LENGTH, u32::from(self.length), "header length")?;
        put(out, OPS, u32::from(self.ops), "header ops")?;
        put(out, OLCHK, u32::from(self.olchk), "olchk")?;
        put(out, W1P, u32::from(self.w1p), "w1p")?;
        put(out, W0P, u32::from(self.w0p), "w0p")?;
        if self.brief {
            if self.alutype != AluType::Main || self.end || self.atom || self.rpt != 0 {
                return Err(RogueError::Malformed { at: 0, what: "brief header of a full group" });
            }
            return put(out, CC1, u32::from(self.cc), "header cc");
        }
        put(out, EXT, 1, "ext")?;
        put(out, CC, u32::from(self.cc), "header cc")?;
        match self.alutype {
            AluType::Main | AluType::Bitwise => {
                let t = if self.alutype == AluType::Main { 0 } else { 2 };
                put(out, ALUTYPE, t, "alutype")?;
                put(out, END, u32::from(self.end), "end")?;
                put(out, ATOM, u32::from(self.atom), "atom")?;
                put(out, RPT, u32::from(self.rpt), "rpt")
            }
            AluType::Control => {
                if self.ops != 0 {
                    return Err(RogueError::Malformed { at: 0, what: "control header ops" });
                }
                put(out, ALUTYPE, 3, "alutype")?;
                put(out, MISCCTL, u32::from(self.miscctl), "miscctl")?;
                put(out, CTRLOP, u32::from(self.ctrlop), "ctrlop")
            }
        }
    }
}
