// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Building groups from instructions and registers — the layer a backend emits through. A main
//! group names its instructions by phase, its six sources, the internal-source selects and its
//! write ports; [`Main::build`] orders the instructions, derives the phase organisation and picks
//! the smallest source and destination forms that hold the registers (the open compiler's rule,
//! so equal operands give equal bytes — the goldens prove it). Control groups carry one control
//! instruction and no parts.

use alloc::vec::Vec;

use super::alu::AluInstr;
use super::backend::BackendInstr;
use super::ctrl::CtrlInstr;
use super::group::{Group, Instr, Instrs};
use super::header::{AluType, Header, OpOrg};
use super::io::{bank, Dests, DstForm, Iss, Reg, SrcForm, Srcs};
use super::{Phase, RogueError};

/// Registers by name.
pub const fn temp(index: u16) -> Reg {
    Reg { bank: bank::TEMP, index }
}
pub const fn coeff(index: u16) -> Reg {
    Reg { bank: bank::COEFF, index }
}
pub const fn shared(index: u16) -> Reg {
    Reg { bank: bank::SHARED, index }
}
/// A special register or constant: constants below 32 and from 64 (64 is 1.0), registers between.
pub const fn special(index: u16) -> Reg {
    Reg { bank: bank::SPECIAL, index }
}
/// The pixel output register the end of a fragment program writes (and its epilog reads).
pub const PIXOUT0: Reg = special(32);
pub const ZERO: Reg = special(0);
pub const ONE: Reg = special(64);

/// The internal-source select values.
pub mod sel {
    /// `is0`: which source feeds the first internal source.
    pub const IS0_S2: u8 = 5;
    /// `is1`–`is5`: feedthroughs.
    pub const FT0: u8 = 0;
    pub const FT1: u8 = 1;
    pub const FT2: u8 = 2;
    pub const FTE: u8 = 3;
}

fn last_bit(v: u32) -> u32 {
    32 - v.leading_zeros()
}

/// A form's capacities: per source (bank bits, index bits), the mux's bits; `None` = absent.
struct Capacity {
    form: SrcForm,
    slots: [Option<(u32, u32)>; 3],
    mux: Option<u32>,
}

const LOWER: [Capacity; 8] = [
    Capacity { form: SrcForm::One1b6i, slots: [Some((1, 6)), None, None], mux: None },
    Capacity { form: SrcForm::One3b11i, slots: [Some((3, 11)), None, None], mux: Some(2) },
    Capacity { form: SrcForm::Two1b6i1b5i, slots: [Some((1, 6)), Some((1, 5)), None], mux: None },
    Capacity {
        form: SrcForm::Two2b7i2b7i,
        slots: [Some((2, 7)), Some((2, 7)), None],
        mux: Some(2),
    },
    Capacity {
        form: SrcForm::Two3b11i2b8i,
        slots: [Some((3, 11)), Some((2, 8)), None],
        mux: Some(3),
    },
    Capacity {
        form: SrcForm::Three2b7i2b7i2b6i,
        slots: [Some((2, 7)), Some((2, 7)), Some((2, 6))],
        mux: Some(2),
    },
    Capacity {
        form: SrcForm::Three3b8i2b8i3b8i,
        slots: [Some((3, 8)), Some((2, 8)), Some((3, 8))],
        mux: Some(3),
    },
    Capacity {
        form: SrcForm::Three3b11i2b8i3b11i,
        slots: [Some((3, 11)), Some((2, 8)), Some((3, 11))],
        mux: Some(3),
    },
];

const UPPER: [Capacity; 8] = [
    Capacity { form: SrcForm::One1b6i, slots: [Some((1, 6)), None, None], mux: None },
    Capacity { form: SrcForm::One3b11i, slots: [Some((3, 11)), None, None], mux: None },
    Capacity { form: SrcForm::Two1b6i1b5i, slots: [Some((1, 6)), Some((1, 5)), None], mux: None },
    Capacity { form: SrcForm::Two2b7i2b7i, slots: [Some((2, 7)), Some((2, 7)), None], mux: None },
    Capacity { form: SrcForm::Two3b11i2b8i, slots: [Some((3, 11)), Some((2, 8)), None], mux: None },
    Capacity {
        form: SrcForm::Three2b7i2b7i2b6i,
        slots: [Some((2, 7)), Some((2, 7)), Some((2, 6))],
        mux: None,
    },
    Capacity {
        form: SrcForm::Three3b8i2b8i3b8i,
        slots: [Some((3, 8)), Some((2, 8)), Some((2, 8))],
        mux: None,
    },
    Capacity {
        form: SrcForm::Three3b11i2b8i3b11i,
        slots: [Some((3, 11)), Some((2, 8)), Some((2, 8))],
        mux: None,
    },
];

/// The smallest source form holding `regs` (and, in the lower part, `mux`).
fn src_part(regs: [Option<Reg>; 3], mux: u8, upper: bool) -> Result<Srcs, RogueError> {
    let table = if upper { &UPPER } else { &LOWER };
    let need = regs.map(|r| r.map(|r| (last_bit(r.bank.into()), last_bit(r.index.into()))));
    let mux_bits = last_bit(mux.into());
    for cap in table {
        let fits = need.iter().zip(&cap.slots).all(|(n, c)| match (n, c) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some((nb, ni)), Some((cb, ci))) => nb <= cb && ni <= ci,
        });
        let mux_fits = mux_bits == 0 || cap.mux.is_some_and(|m| mux_bits <= m);
        if fits && mux_fits {
            return Ok(Srcs {
                form: cap.form,
                upper,
                regs: regs.map(|r| r.unwrap_or_default()),
                mux: if cap.mux.is_some() { mux } else { 0 },
            });
        }
    }
    Err(RogueError::TooWide { what: "sources for every form", value: 0 })
}

/// The smallest destination form holding the write ports' registers.
fn dst_part(w0: Option<Reg>, w1: Option<Reg>) -> Result<Option<Dests>, RogueError> {
    let fits =
        |r: Reg, b: u32, i: u32| last_bit(r.bank.into()) <= b && last_bit(r.index.into()) <= i;
    Ok(match (w0, w1) {
        (None, None) => None,
        (Some(d), None) | (None, Some(d)) => {
            let form = if fits(d, 1, 6) {
                DstForm::One1b6i
            } else if fits(d, 3, 11) {
                DstForm::One3b11i
            } else {
                return Err(RogueError::TooWide { what: "destination", value: d.index.into() });
            };
            Some(Dests { form, d0: d, d1: Reg::default() })
        }
        (Some(a), Some(b)) => {
            let form = if fits(a, 1, 7) && fits(b, 1, 6) {
                DstForm::Two1b7i1b6i
            } else if fits(a, 3, 8) && fits(b, 3, 8) {
                DstForm::Two3b8i3b8i
            } else if fits(a, 3, 11) && fits(b, 3, 11) {
                DstForm::Two3b11i3b11i
            } else {
                return Err(RogueError::TooWide {
                    what: "destinations",
                    value: a.index.max(b.index).into(),
                });
            };
            Some(Dests { form, d0: a, d1: b })
        }
    })
}

/// A main-ALU group under construction.
#[derive(Clone, Debug, Default)]
pub struct Main {
    pub alu: Vec<(Phase, AluInstr)>,
    pub backend: Option<BackendInstr>,
    /// s0–s5.
    pub srcs: [Option<Reg>; 6],
    /// is0 (the lower part's mux).
    pub mux: u8,
    pub iss: Iss,
    pub w0: Option<Reg>,
    pub w1: Option<Reg>,
    pub olchk: bool,
    pub end: bool,
    /// Repetitions, 1–4 (each repetition steps the registers).
    pub repeat: u8,
    pub cc: u8,
}

impl Main {
    pub fn new() -> Self {
        Self { repeat: 1, ..Self::default() }
    }

    /// The group: instructions in encoding order, the phase organisation, the forms.
    pub fn build(self) -> Result<Group, RogueError> {
        let has = |p: fn(Phase) -> bool| self.alu.iter().any(|(ph, _)| p(*ph));
        let (p0, p1) = (has(|p| p == Phase::P0), has(|p| p == Phase::P1));
        let p2 = has(|p| matches!(p, Phase::P2Pck | Phase::P2Tst | Phase::P2Mov));
        let be = self.backend.is_some();
        let org = match (p0, p1, p2, be) {
            (true, false, false, false) => OpOrg::P0,
            (false, false, true, false) => OpOrg::P2,
            (false, false, false, true) => OpOrg::Be,
            (true, true, false, false) => OpOrg::P0P1,
            (true, false, true, false) => OpOrg::P0P2,
            (true, true, true, false) => OpOrg::P0P1P2,
            (true, false, true, true) => OpOrg::P0P2Be,
            (true, true, true, true) => OpOrg::P0P1P2Be,
            _ => {
                return Err(RogueError::Malformed {
                    at: 0,
                    what: "a phase set no organisation names",
                })
            }
        };
        if !(1..=4).contains(&self.repeat) {
            return Err(RogueError::TooWide { what: "repeat", value: self.repeat.into() });
        }
        let mut instrs = Instrs::new();
        if let Some(b) = self.backend {
            instrs.push(Phase::Backend, Instr::Backend(b))?;
        }
        let mut alu = self.alu.clone();
        alu.sort_by(|a, b| b.0.cmp(&a.0));
        for (p, i) in alu {
            instrs.push(p, Instr::Alu(i))?;
        }
        let mut header = Header::new(AluType::Main, org as u8);
        header.brief = false;
        header.olchk = self.olchk;
        header.end = self.end;
        header.rpt = self.repeat - 1;
        header.cc = self.cc;
        header.w0p = self.w0.is_some();
        header.w1p = self.w1.is_some();
        let s = self.srcs;
        let mut g = Group {
            header,
            instrs,
            lower: Some(src_part([s[0], s[1], s[2]], self.mux, false)?),
            upper: Some(src_part([s[3], s[4], s[5]], 0, true)?),
            iss: Some(self.iss),
            dests: dst_part(self.w0, self.w1)?,
            align_pad: 0,
        };
        if g.size() > super::group::MAX_GROUP_BYTES {
            return Err(RogueError::TooWide { what: "group length", value: g.size() as u32 });
        }
        g.seal();
        Ok(g)
    }
}

/// A control group of one instruction.
pub fn ctrl(i: CtrlInstr, cc: u8) -> Group {
    let mut header = Header::new(AluType::Control, 0);
    header.brief = false;
    header.ctrlop = i.ctrlop();
    header.cc = cc;
    let mut g = Group {
        header,
        instrs: Instrs::one(Phase::Ctrl, Instr::Ctrl(i)),
        lower: None,
        upper: None,
        iss: None,
        dests: None,
        align_pad: 0,
    };
    g.seal();
    g
}

/// `dst = src` through phase 0's bypass (the tile read and the output write are this).
pub fn mov(dst: Reg, src: Reg) -> Main {
    let mut m = Main::new();
    m.alu.push((Phase::P0, AluInstr::Sngl { op: super::alu::sngl::BYP, ext: None }));
    m.srcs[0] = Some(src);
    m.w0 = Some(dst);
    m
}

/// The add and multiply's modifiers: the result saturated to 0..1, the first source negated,
/// either source's absolute value, the first source's floor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FamMods {
    pub sat: bool,
    pub s0neg: bool,
    pub s0abs: bool,
    pub s1abs: bool,
    pub s0flr: bool,
}

/// `dst = a <op> b` for add or multiply (`op` one of `alu::op`'s first four).
pub fn fam(op: u8, dst: Reg, a: Reg, b: Reg) -> Main {
    fam_mods(op, dst, a, b, FamMods::default())
}

/// [`fam`] with its modifiers (`fadd.sat x, sc0` saturates, `fadd x.neg, sc0` negates).
pub fn fam_mods(op: u8, dst: Reg, a: Reg, b: Reg, m: FamMods) -> Main {
    let mut g = Main::new();
    g.alu.push((
        Phase::P0,
        AluInstr::Fam {
            op,
            sat: m.sat,
            s0neg: m.s0neg,
            s0abs: m.s0abs,
            s1abs: m.s1abs,
            s0flr: m.s0flr,
        },
    ));
    g.srcs[0] = Some(a);
    g.srcs[1] = Some(b);
    g.w0 = Some(dst);
    g
}

/// The fused multiply-add's modifiers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FmadMods {
    pub s0neg: bool,
    pub s0abs: bool,
    pub s2neg: bool,
    pub sat: bool,
}

/// `dst = a * b + c`.
pub fn fmad(dst: Reg, a: Reg, b: Reg, c: Reg) -> Main {
    fmad_mods(dst, a, b, c, FmadMods::default())
}

/// [`fmad`] with its modifiers.
pub fn fmad_mods(dst: Reg, a: Reg, b: Reg, c: Reg, m: FmadMods) -> Main {
    let mut g = Main::new();
    g.alu.push((
        Phase::P0,
        AluInstr::Fmad { s0neg: m.s0neg, s0abs: m.s0abs, s2neg: m.s2neg, sat: m.sat, ext: None },
    ));
    g.srcs[0] = Some(a);
    g.srcs[1] = Some(b);
    g.srcs[2] = Some(c);
    g.w0 = Some(dst);
    g
}

/// `dst = op(src)` through the single-source unit (`alu::sngl`: reciprocal, inverse square
/// root, base-2 logarithm and exponent).
pub fn sngl(op: u8, dst: Reg, src: Reg) -> Main {
    let mut m = Main::new();
    m.alu.push((Phase::P0, AluInstr::Sngl { op, ext: None }));
    m.srcs[0] = Some(src);
    m.w0 = Some(dst);
    m
}

/// `dst = min(a, b)` (or `max`), the open compiler's way: both through the bypasses (phases 0
/// and 1), phase 2's float test, the conditional move taking `a` where the test holds and `b`
/// elsewhere — one group.
pub fn minmax(dst: Reg, a: Reg, b: Reg, max: bool) -> Main {
    use super::alu::{tst, MovcExt, TstExt};
    let mut m = Main::new();
    let byp = AluInstr::Sngl { op: super::alu::sngl::BYP, ext: None };
    m.alu.push((Phase::P0, byp));
    m.alu.push((Phase::P1, byp));
    m.alu.push((
        Phase::P2Tst,
        AluInstr::Tst {
            op: if max { tst::G } else { tst::L },
            pwen: false,
            ext: Some(TstExt { ttype: tst::F32, p2end: true, elem: 0 }),
        },
    ));
    m.alu.push((
        Phase::P2Mov,
        AluInstr::Movc {
            movw1: 0,
            movw0: 0,
            ext: Some(MovcExt { maskw0: 15, aw: false, p2end: false }),
        },
    ));
    m.srcs[0] = Some(a);
    m.srcs[3] = Some(b);
    // is1 = ft0, is2 = ft1 (the test's operands), is4 = ft1 (the move's other choice).
    m.iss = Iss { is1: 0, is2: 0, is4: sel::FT1, ..Iss::default() };
    m.w0 = Some(dst);
    m
}

/// Unpacks four unorm8 channels of `src` into `dst..dst+4` (repeated over the channels).
pub fn unpack_unorm8x4(dst: Reg, src: Reg) -> Main {
    let mut m = Main::new();
    m.alu.push((
        Phase::P0,
        AluInstr::Upck { elem: 0, scale_rtz: true, format: super::alu::pck::U8888 },
    ));
    m.srcs[0] = Some(src);
    m.w0 = Some(dst);
    m.repeat = 4;
    m
}

/// Packs `src..src+4` into one unorm8×4 word at `dst` (phase 2's pack, repeated over the channels).
pub fn pack_unorm8x4(dst: Reg, src: Reg) -> Main {
    let mut m = Main::new();
    m.alu.push((
        Phase::P2Pck,
        AluInstr::Pck { prog: false, rtz: false, scale: true, format: super::alu::pck::U8888 },
    ));
    m.srcs[2] = Some(src);
    m.mux = sel::IS0_S2;
    m.iss = Iss { is3: sel::FTE, is4: sel::FT2, ..Iss::default() };
    m.w0 = Some(dst);
    m.repeat = 4;
    m
}

/// A 2D sample: texture state `tex`, coordinates `coords`, sampler state `smp`, the channels
/// returned to `dst..` through data return channel `drc` (wait for it with a data fence).
pub fn sample_2d(dst: Reg, tex: Reg, coords: Reg, smp: Reg, chans: u8, drc: u8) -> Main {
    let mut m = Main::new();
    m.backend =
        Some(BackendInstr::Smp { fcnorm: true, drc, dmn: 2, chan: chans - 1, lodm: 0, exta: None });
    m.srcs[0] = Some(tex);
    m.srcs[1] = Some(coords);
    m.srcs[2] = Some(smp);
    m.srcs[3] = Some(ZERO);
    m.srcs[4] = Some(dst);
    m
}
