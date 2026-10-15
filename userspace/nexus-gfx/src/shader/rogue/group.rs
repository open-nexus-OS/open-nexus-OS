// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! An instruction group as the shader core fetches it: the header, the instructions last phase
//! first (backend, phase 2's move, test and pack, phase 1, phase 0), the lower and upper source
//! parts, the select byte, the destinations, then one 0xff when the parts end on an odd byte,
//! and at the program's end the alignment padding (0xf0 with its word count, then 0xff). The
//! header's length counts the whole in 16-bit words; its decode assist is where a main group's
//! phase-1 and phase-0 words begin (where the parts begin in the others, 0 in a control group
//! without parts). The encoder derives both; a decoded group re-encodes byte for byte.

use alloc::vec::Vec;

use super::alu::AluInstr;
use super::backend::BackendInstr;
use super::bitwise::BitwiseInstr;
use super::ctrl::CtrlInstr;
use super::header::{opcnt, AluType, Header, OpOrg};
use super::io::{Dests, Iss, Srcs};
use super::{Phase, RogueError};

/// The longest group the header's 4-bit length can name, in bytes.
pub const MAX_GROUP_BYTES: usize = 30;

/// One instruction of a group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Instr {
    Alu(AluInstr),
    Backend(BackendInstr),
    Bitwise(BitwiseInstr),
    Ctrl(CtrlInstr),
}

impl Instr {
    pub fn size(&self) -> usize {
        match self {
            Self::Alu(i) => i.size(),
            Self::Backend(i) => i.size(),
            Self::Bitwise(i) => i.size(),
            Self::Ctrl(i) => i.size(),
        }
    }

    fn encode(&self, out: &mut [u8]) -> Result<(), RogueError> {
        match self {
            Self::Alu(i) => i.encode(out),
            Self::Backend(i) => i.encode(out),
            Self::Bitwise(i) => i.encode(out),
            Self::Ctrl(i) => i.encode(out),
        }
    }
}

/// An instruction group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// The header; its `length` and `da` are derived when the group is encoded.
    pub header: Header,
    /// The instructions with their phases, in encoding order (last phase first).
    pub instrs: Vec<(Phase, Instr)>,
    pub lower: Option<Srcs>,
    pub upper: Option<Srcs>,
    pub iss: Option<Iss>,
    pub dests: Option<Dests>,
    /// Bytes of program-end alignment padding after the parts and the word pad.
    pub align_pad: u8,
}

fn need(b: &[u8], n: usize, at: usize) -> Result<(), RogueError> {
    if b.len() < n {
        Err(RogueError::Truncated { at })
    } else {
        Ok(())
    }
}

impl Group {
    /// Decodes the group at the start of `b` (`at` is its offset in the program); its length.
    pub fn decode(b: &[u8], at: usize) -> Result<(Self, usize), RogueError> {
        let header = Header::decode(b, at)?;
        let total = usize::from(header.length) * 2;
        if total < header.size() {
            return Err(RogueError::Malformed { at, what: "group shorter than its header" });
        }
        need(b, total, at)?;
        let g = &b[..total];
        let mut pos = header.size();
        let mut instrs = Vec::new();
        let tail = |pos: usize| g.get(pos..).unwrap_or(&[]);
        match header.alutype {
            AluType::Main => {
                let org = OpOrg::from_bits(header.ops);
                let da = usize::from(header.da);
                if org.has_backend() {
                    let (i, n) = BackendInstr::decode(tail(pos), at + pos)?;
                    instrs.push((Phase::Backend, Instr::Backend(i)));
                    pos += n;
                }
                let mut last: Option<Phase> = None;
                while pos < da {
                    let slot = AluInstr::p2_slot(tail(pos)).filter(|_| org.has_p2()).ok_or(
                        RogueError::Malformed { at: at + pos, what: "word in the phase-2 area" },
                    )?;
                    if last.is_some_and(|prev| slot >= prev) {
                        return Err(RogueError::Malformed { at: at + pos, what: "phase-2 order" });
                    }
                    let (i, n) = AluInstr::decode(tail(pos), slot, at + pos)?;
                    instrs.push((slot, Instr::Alu(i)));
                    pos += n;
                    last = Some(slot);
                }
                if pos != da || (org.has_p2() && last.is_none()) {
                    return Err(RogueError::Malformed { at, what: "decode assist" });
                }
                for (present, phase) in [(org.has_p1(), Phase::P1), (org.has_p0(), Phase::P0)] {
                    if present {
                        let (i, n) = AluInstr::decode(tail(pos), phase, at + pos)?;
                        instrs.push((phase, Instr::Alu(i)));
                        pos += n;
                    }
                }
            }
            AluType::Bitwise => {
                for (bit, phase) in
                    [(opcnt::P2, Phase::P2), (opcnt::P1, Phase::P1), (opcnt::P0, Phase::P0)]
                {
                    if header.ops & bit != 0 {
                        let (i, n) = BitwiseInstr::decode(tail(pos), phase, at + pos)?;
                        instrs.push((phase, Instr::Bitwise(i)));
                        pos += n;
                    }
                }
                if pos != usize::from(header.da) {
                    return Err(RogueError::Malformed { at, what: "decode assist" });
                }
            }
            AluType::Control => {
                let (i, n) = CtrlInstr::decode(tail(pos), header.ctrlop, at + pos)?;
                instrs.push((Phase::Ctrl, Instr::Ctrl(i)));
                pos += n;
                if header.da != 0 || header.w0p || header.w1p {
                    return Err(RogueError::Unsupported {
                        at,
                        what: "control group with inputs or outputs",
                    });
                }
            }
        }
        let (mut lower, mut upper, mut iss, mut dests) = (None, None, None, None);
        if header.alutype != AluType::Control {
            let s = Srcs::decode(tail(pos), false, at + pos)?;
            pos += s.size();
            lower = Some(s);
            let s = Srcs::decode(tail(pos), true, at + pos)?;
            pos += s.size();
            upper = Some(s);
            if header.alutype == AluType::Main {
                iss = Some(Iss::decode(tail(pos), at + pos)?);
                pos += 1;
            }
            if header.w0p || header.w1p {
                let d = Dests::decode(tail(pos), header.w0p && header.w1p, at + pos)?;
                pos += d.size();
                dests = Some(d);
            }
        }
        if pos > total {
            return Err(RogueError::Malformed { at, what: "parts past the group's length" });
        }
        let align_pad = Self::padding(&g[pos..], pos % 2 == 1, at + pos)?;
        Ok((Self { header, instrs, lower, upper, iss, dests, align_pad }, total))
    }

    /// Checks the padding after the parts; the alignment bytes it holds.
    fn padding(rest: &[u8], word: bool, at: usize) -> Result<u8, RogueError> {
        let bad = RogueError::Malformed { at, what: "padding" };
        let rest = if word {
            match rest.split_first() {
                Some((0xff, r)) => r,
                _ => return Err(bad),
            }
        } else {
            rest
        };
        match rest.split_first() {
            None => Ok(0),
            Some((&first, r)) => {
                let n = rest.len();
                if n % 2 != 0
                    || usize::from(first) != 0xf0 | (n / 2)
                    || r.iter().any(|&x| x != 0xff)
                {
                    return Err(bad);
                }
                Ok(n as u8)
            }
        }
    }

    /// The parts' bytes before padding, and the decode assist they imply.
    fn layout(&self) -> (usize, usize) {
        let hdr = self.header.size();
        let instrs: usize = self.instrs.iter().map(|(_, i)| i.size()).sum();
        let io = self.lower.map_or(0, |s| s.size())
            + self.upper.map_or(0, |s| s.size())
            + usize::from(self.iss.is_some())
            + self.dests.map_or(0, |d| d.size());
        let da = match self.header.alutype {
            AluType::Main => {
                hdr + self
                    .instrs
                    .iter()
                    .filter(|(p, _)| !matches!(p, Phase::P0 | Phase::P1))
                    .map(|(_, i)| i.size())
                    .sum::<usize>()
            }
            AluType::Bitwise => hdr + instrs,
            AluType::Control if io == 0 => 0,
            AluType::Control => hdr + instrs,
        };
        (hdr + instrs + io, da)
    }

    /// Writes the derived header fields — the length in words and the decode assist — so a
    /// built group equals the same group decoded.
    pub fn seal(&mut self) {
        let (_, da) = self.layout();
        self.header.length = (self.size() / 2).min(15) as u8;
        self.header.da = da.min(15) as u8;
    }

    /// The group's length in bytes, padding included.
    pub fn size(&self) -> usize {
        let (parts, _) = self.layout();
        parts + parts % 2 + usize::from(self.align_pad)
    }

    /// Appends the encoded group to `out`; the header's length and decode assist are derived.
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<usize, RogueError> {
        let (parts, da) = self.layout();
        let total = self.size();
        if total > MAX_GROUP_BYTES || total % 2 != 0 || self.align_pad % 2 != 0 {
            return Err(RogueError::TooWide { what: "group length", value: total as u32 });
        }
        if da > 15 {
            return Err(RogueError::TooWide { what: "decode assist", value: da as u32 });
        }
        let mut header = self.header;
        header.length = (total / 2) as u8;
        header.da = da as u8;
        let start = out.len();
        out.resize(start + total, 0);
        let buf = &mut out[start..];
        header.encode(buf)?;
        let mut pos = header.size();
        for (_, i) in &self.instrs {
            i.encode(&mut buf[pos..])?;
            pos += i.size();
        }
        for s in [self.lower, self.upper].into_iter().flatten() {
            s.encode(&mut buf[pos..])?;
            pos += s.size();
        }
        if let Some(i) = self.iss {
            i.encode(&mut buf[pos..])?;
            pos += 1;
        }
        if let Some(d) = self.dests {
            d.encode(&mut buf[pos..])?;
            pos += d.size();
        }
        debug_assert_eq!(pos, parts);
        if parts % 2 == 1 {
            buf[pos] = 0xff;
            pos += 1;
        }
        if self.align_pad != 0 {
            buf[pos] = 0xf0 | (self.align_pad / 2);
            buf[pos + 1..].fill(0xff);
        }
        Ok(total)
    }
}

/// The instruction cache's alignment, in bytes: a program's last group starts at a multiple of
/// it and is a multiple of it long.
pub const ICACHE_ALIGN: usize = 8;

/// Pads a program's end as the hardware fetches it: the group before the last grows until the
/// last starts aligned, the last until it ends aligned (the open compiler's rule).
pub fn align_end(groups: &mut [Group]) -> Result<(), RogueError> {
    let n = groups.len();
    if n == 0 {
        return Ok(());
    }
    let offset: usize = groups[..n - 1].iter().map(|g| g.size()).sum();
    let pad_last = (ICACHE_ALIGN - groups[n - 1].size() % ICACHE_ALIGN) % ICACHE_ALIGN;
    groups[n - 1].align_pad += pad_last as u8;
    let pad_before = (ICACHE_ALIGN - offset % ICACHE_ALIGN) % ICACHE_ALIGN;
    if pad_before != 0 {
        if n < 2 {
            return Err(RogueError::Malformed {
                at: 0,
                what: "a one-group program off its alignment",
            });
        }
        groups[n - 2].align_pad += pad_before as u8;
    }
    for g in &mut groups[n.saturating_sub(2)..] {
        if g.size() > MAX_GROUP_BYTES {
            return Err(RogueError::TooWide {
                what: "group length after alignment",
                value: g.size() as u32,
            });
        }
        g.seal();
    }
    Ok(())
}

/// Decodes a whole program into its groups.
pub fn decode_program(b: &[u8]) -> Result<Vec<Group>, RogueError> {
    let mut groups = Vec::new();
    let mut pos = 0;
    while pos < b.len() {
        let (g, n) = Group::decode(&b[pos..], pos)?;
        groups.push(g);
        pos += n;
    }
    Ok(groups)
}

/// Encodes a program from its groups.
pub fn encode_program(groups: &[Group]) -> Result<Vec<u8>, RogueError> {
    let mut out = Vec::new();
    for g in groups {
        g.encode(&mut out)?;
    }
    Ok(out)
}
