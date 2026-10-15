// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The board GPU's backend (TASK-0215 5c-2 P3c, ADR-0072): a fragment module of our IR compiled
//! into instruction groups through the assembler. The compositor's subset, chosen by what the
//! board measured (5c-2 cycle 14): a texture sampled at an interpolated input becomes one
//! iterate-and-sample word (or iterate, fence, sample, fence when fusing is off); a multiply
//! over four channels one repeated group, a shared operand always the first source; the tile
//! read a bypass and an unpack; the colour packed straight into the pixel output, which ends the
//! program; the pass type opaque unless the program reads the tile. Registers are temporaries
//! handed out first-fit and taken back at a value's last use; an operation that steps lane by
//! lane may write over an operand that dies with it (an unpack may not: it reads its source on
//! every repetition). What the subset lacks is refused by name.

use alloc::vec;
use alloc::vec::Vec;

use super::alu::{op, AluInstr};
use super::asm::{
    ctrl, fam, fmad, mov, pack_unorm8x4, sample_2d, shared, temp, unpack_unorm8x4, Main, ONE,
    PIXOUT0, ZERO,
};
use super::ctrl::{CtrlInstr, Ditr, Itrsmp};
use super::group::{align_end, Group, Instr};
use super::io::{bank, Reg};
use super::reference::{PASS_OPAQUE, PASS_TRANSLUCENT};
use super::RogueError;
use crate::shader::ir::validate::validate;
use crate::shader::ir::{BinOp, Interp, IrError, Lod, Module, Op, Scalar, Stage, Type, ValueId};
use crate::shader::library::feature;

/// Where the setup programs put what a program reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// The perspective W coefficients (a coefficient register index).
    pub w_coeff: u16,
    /// The first input's coefficients; four per component, inputs in order.
    pub inputs_coeff: u16,
    /// The texture and sampler states (shared register indices, four each).
    pub texture_state: u16,
    pub sampler_state: u16,
    /// The push-constant block's first shared register.
    pub push: u16,
}

impl Layout {
    /// The render bundle's setup programs (5b's captured pass).
    pub const BUNDLE: Self =
        Self { w_coeff: 0, inputs_coeff: 4, texture_state: 4, sampler_state: 12, push: 24 };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// A texture sampled at an interpolated input as one iterate-and-sample word.
    pub fused_sample: bool,
}

/// A compiled program and what its kick and draw state must say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub groups: Vec<Group>,
    /// Temporaries in units of four (the kick word's field).
    pub temps: u8,
    pub pass_type: u8,
}

impl Program {
    /// The library feature bits the program needs of the device.
    pub fn features(&self) -> u32 {
        let fused = self
            .groups
            .iter()
            .any(|g| g.instrs.iter().any(|(_, i)| matches!(i, Instr::Ctrl(CtrlInstr::Itrsmp(_)))));
        if fused {
            feature::FUSED_ITERATE_SAMPLE
        } else {
            0
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodegenError {
    Ir(IrError),
    Encode(RogueError),
    /// Instruction `at` is outside the subset.
    Unsupported {
        at: usize,
        what: &'static str,
    },
    /// More temporaries than the kick word can name.
    Registers,
}

impl From<RogueError> for CodegenError {
    fn from(e: RogueError) -> Self {
        Self::Encode(e)
    }
}

/// The temporaries the allocator hands out — the kick word names a program's in units of four
/// (up to 63 units); 64 registers are ample for the compositor's programs.
const MAX_TEMPS: usize = 64;

/// A coefficient or shared register as the units of four a control word names it in.
fn fours(at: usize, reg: u16) -> Result<u8, CodegenError> {
    u8::try_from(reg / 4)
        .map_err(|_| CodegenError::Unsupported { at, what: "a register beyond a control word" })
}

/// Where a value lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Loc {
    /// An input not yet iterated.
    Input(u8),
    /// `lanes` consecutive registers from the first.
    Regs(Reg, u8),
    /// One register for every lane.
    Broadcast(Reg),
}

struct Gen<'a> {
    m: &'a Module,
    layout: Layout,
    opts: Options,
    groups: Vec<Group>,
    locs: Vec<Option<Loc>>,
    /// The value whose registers each value's location aliases (itself for owners).
    root: Vec<usize>,
    last_use: Vec<usize>,
    free: [bool; MAX_TEMPS],
    high: usize,
    reads_tile: bool,
}

impl Gen<'_> {
    fn alloc(&mut self, n: usize) -> Result<u16, CodegenError> {
        let start = (0..=MAX_TEMPS - n)
            .find(|&s| self.free[s..s + n].iter().all(|f| *f))
            .ok_or(CodegenError::Registers)?;
        self.free[start..start + n].fill(false);
        self.high = self.high.max(start + n);
        Ok(start as u16)
    }

    fn release(&mut self, r: Reg, n: u8) {
        if r.bank == bank::TEMP {
            let s = usize::from(r.index);
            self.free[s..s + usize::from(n)].fill(true);
        }
    }

    fn loc(&self, at: usize, v: ValueId) -> Result<Loc, CodegenError> {
        self.locs
            .get(v.0 as usize)
            .copied()
            .flatten()
            .ok_or(CodegenError::Unsupported { at, what: "a value without a location" })
    }

    /// Registers an operand that dies at `at` and owns temporaries: freed now, so the result
    /// can take them (only for operations that step lane by lane).
    fn dying_regs(&self, at: usize, v: ValueId) -> Option<(Reg, u8)> {
        let i = v.0 as usize;
        if self.root[i] != i || self.last_use[i] != at {
            return None;
        }
        match self.locs[i] {
            Some(Loc::Regs(r, n)) if r.bank == bank::TEMP => Some((r, n)),
            _ => None,
        }
    }

    /// The input's first coefficient register, its lanes and whether it is perspective-correct.
    fn input_coeff(&self, at: usize, i: u8) -> Result<(u16, u8, u8), CodegenError> {
        let inputs = &self.m.interface.inputs;
        let inp = inputs
            .get(usize::from(i))
            .ok_or(CodegenError::Unsupported { at, what: "an input index" })?;
        let before: u16 = inputs[..usize::from(i)].iter().map(|x| u16::from(x.ty.lanes) * 4).sum();
        let persp = match inp.interp {
            Interp::Perspective => 1,
            Interp::Linear => 0,
            Interp::Flat => return Err(CodegenError::Unsupported { at, what: "a flat input" }),
        };
        Ok((self.layout.inputs_coeff + before, inp.ty.lanes, persp))
    }

    fn wdf(&mut self) {
        self.groups.push(ctrl(CtrlInstr::Wdf, 0));
    }

    /// An input iterated into fresh temporaries.
    fn iterate(&mut self, at: usize, i: u8) -> Result<Loc, CodegenError> {
        let (coeff, lanes, persp) = self.input_coeff(at, i)?;
        let dest = self.alloc(usize::from(lanes))?;
        let d = Ditr {
            dest: dest as u8,
            coff: fours(at, coeff)?,
            persp,
            woff: fours(at, self.layout.w_coeff)?,
            count: lanes,
            ..Ditr::default()
        };
        self.groups.push(ctrl(CtrlInstr::Ditr(d), 0));
        self.wdf();
        Ok(Loc::Regs(temp(dest), lanes))
    }

    fn sample(&mut self, at: usize, coords: ValueId, lod: Lod) -> Result<Loc, CodegenError> {
        if lod != Lod::Implicit {
            return Err(CodegenError::Unsupported { at, what: "an explicit level of detail" });
        }
        let c = self.loc(at, coords)?;
        if let (Loc::Input(i), true) = (c, self.opts.fused_sample) {
            let (coeff, _, persp) = self.input_coeff(at, i)?;
            let dest = self.alloc(4)?;
            let w = Itrsmp {
                dest: dest as u8,
                coff: fours(at, coeff)?,
                persp,
                texoff: fours(at, self.layout.texture_state)?,
                smpoff: fours(at, self.layout.sampler_state)?,
                dmn: 2,
                chan: 3,
                fcnorm: true,
                woff: fours(at, self.layout.w_coeff)?,
                ..Itrsmp::default()
            };
            self.groups.push(ctrl(CtrlInstr::Itrsmp(w), 0));
            self.wdf();
            return Ok(Loc::Regs(temp(dest), 4));
        }
        let regs = match c {
            Loc::Input(i) => {
                let l = self.iterate(at, i)?;
                // The iterated coordinates are this sample's alone.
                self.locs[coords.0 as usize] = Some(l);
                l
            }
            other => other,
        };
        let Loc::Regs(cr, 2) = regs else {
            return Err(CodegenError::Unsupported {
                at,
                what: "sample coordinates outside two registers",
            });
        };
        // The sample reads its coordinates at issue: its result may take their registers when
        // they die with it (freed once, here — not again at the instruction's end).
        if let Some((r, n)) = self.dying_regs(at, coords) {
            self.release(r, n);
            self.locs[coords.0 as usize] = None;
        }
        let dest = self.alloc(4)?;
        let ts = shared(self.layout.texture_state);
        let ss = shared(self.layout.sampler_state);
        self.groups.push(sample_2d(temp(dest), ts, cr, ss, 4, 0).build()?);
        self.wdf();
        Ok(Loc::Regs(temp(dest), 4))
    }

    /// The destination of a lane-stepping operation over `lanes`: a dying operand's registers,
    /// else fresh ones.
    fn step_dest(
        &mut self,
        at: usize,
        lanes: u8,
        operands: &[ValueId],
    ) -> Result<Reg, CodegenError> {
        for v in operands {
            if let Some((r, n)) = self.dying_regs(at, *v) {
                if n == lanes {
                    // Ownership passes to the result; the operand no longer frees them.
                    self.locs[v.0 as usize] = None;
                    return Ok(r);
                }
            }
        }
        Ok(temp(self.alloc(usize::from(lanes))?))
    }

    fn regs(&self, at: usize, v: ValueId, lanes: u8) -> Result<Loc, CodegenError> {
        match self.loc(at, v)? {
            l @ Loc::Regs(_, n) if n == lanes || n == 1 && lanes == 1 => Ok(l),
            l @ Loc::Broadcast(_) => Ok(l),
            _ => Err(CodegenError::Unsupported { at, what: "an operand of another width" }),
        }
    }

    /// `dst = a op b` (add or multiply) over the lanes, a shared operand first.
    fn arith(
        &mut self,
        at: usize,
        op: u8,
        a: ValueId,
        b: ValueId,
        s0neg: bool,
        lanes: u8,
    ) -> Result<Loc, CodegenError> {
        let (mut la, mut lb) = (self.regs(at, a, lanes)?, self.regs(at, b, lanes)?);
        let is_shared =
            |l: &Loc| matches!(l, Loc::Regs(r, _) | Loc::Broadcast(r) if r.bank == bank::SHARED);
        if is_shared(&lb) && !s0neg {
            core::mem::swap(&mut la, &mut lb);
        }
        if is_shared(&lb) {
            return Err(CodegenError::Unsupported {
                at,
                what: "a shared register as the second source",
            });
        }
        let dst = self.step_dest(at, lanes, &[a, b])?;
        let reg = |l: Loc, lane: u16| match l {
            Loc::Regs(r, _) => Reg { index: r.index + lane, ..r },
            Loc::Broadcast(r) => r,
            Loc::Input(_) => r_none(),
        };
        let broadcast = matches!(la, Loc::Broadcast(_)) || matches!(lb, Loc::Broadcast(_));
        let groups = if broadcast { usize::from(lanes) } else { 1 };
        for lane in 0..groups as u16 {
            let mut m =
                fam(op, Reg { index: dst.index + lane, ..dst }, reg(la, lane), reg(lb, lane));
            m.alu[0].1 =
                AluInstr::Fam { op, sat: false, s0neg, s0abs: false, s1abs: false, s0flr: false };
            m.repeat = if broadcast { 1 } else { lanes };
            self.groups.push(m.build()?);
        }
        Ok(Loc::Regs(dst, lanes))
    }

    fn fma(
        &mut self,
        at: usize,
        a: ValueId,
        b: ValueId,
        c: ValueId,
        lanes: u8,
    ) -> Result<Loc, CodegenError> {
        let (la, lb, lc) =
            (self.regs(at, a, lanes)?, self.regs(at, b, lanes)?, self.regs(at, c, lanes)?);
        if matches!(lb, Loc::Regs(r, _) | Loc::Broadcast(r) if r.bank == bank::SHARED) {
            return Err(CodegenError::Unsupported {
                at,
                what: "a shared register as the second source",
            });
        }
        let dst = self.step_dest(at, lanes, &[c, a])?;
        let reg = |l: Loc, lane: u16| match l {
            Loc::Regs(r, _) => Reg { index: r.index + lane, ..r },
            Loc::Broadcast(r) => r,
            Loc::Input(_) => r_none(),
        };
        let broadcast = [la, lb, lc].iter().any(|l| matches!(l, Loc::Broadcast(_)));
        let groups = if broadcast { usize::from(lanes) } else { 1 };
        for lane in 0..groups as u16 {
            let mut m = fmad(
                Reg { index: dst.index + lane, ..dst },
                reg(la, lane),
                reg(lb, lane),
                reg(lc, lane),
            );
            m.repeat = if broadcast { 1 } else { lanes };
            self.groups.push(m.build()?);
        }
        Ok(Loc::Regs(dst, lanes))
    }

    fn instr(&mut self, at: usize) -> Result<(), CodegenError> {
        let ins = &self.m.body[at];
        let t = ins.ty;
        let unsupported = |what| CodegenError::Unsupported { at, what };
        let loc = match ins.op.clone() {
            // The constant registers hold +0.0 and 1.0: a constant is theirs bit for bit (-0.0 is
            // not +0.0).
            Op::Const(bits) if t == Type::F32 => match bits[0] {
                0x0000_0000 => Loc::Regs(ZERO, 1),
                0x3f80_0000 => Loc::Regs(ONE, 1),
                _ => return Err(unsupported("a constant other than 0 and 1")),
            },
            Op::Const(_) => return Err(unsupported("a vector or non-float constant")),
            Op::LoadInput(i) => Loc::Input(i),
            Op::LoadPush(off) => Loc::Regs(shared(self.layout.push + (off / 4) as u16), t.lanes),
            Op::Sample { coords, lod, .. } => self.sample(at, coords, lod)?,
            Op::Binary(BinOp::Mul, a, b) => self.arith(at, op::FMUL, a, b, false, t.lanes)?,
            Op::Binary(BinOp::Add, a, b) => self.arith(at, op::FADD, a, b, false, t.lanes)?,
            // a − b = (−b) + a.
            Op::Binary(BinOp::Sub, a, b) => self.arith(at, op::FADD, b, a, true, t.lanes)?,
            Op::Binary(..) => {
                return Err(unsupported("a binary operation outside add, subtract, multiply"))
            }
            Op::Fma(a, b, c) => self.fma(at, a, b, c, t.lanes)?,
            Op::Extract(v, lane) => match self.loc(at, v)? {
                Loc::Regs(r, _) => Loc::Regs(Reg { index: r.index + u16::from(lane), ..r }, 1),
                Loc::Broadcast(r) => Loc::Regs(r, 1),
                Loc::Input(_) => return Err(unsupported("a lane of an input")),
            },
            Op::Splat(v) => match self.loc(at, v)? {
                Loc::Regs(r, 1) => Loc::Broadcast(r),
                _ => return Err(unsupported("a splat of a vector")),
            },
            Op::Swizzle(v, sel) => match self.loc(at, v)? {
                Loc::Regs(r, _)
                    if sel[..usize::from(t.lanes)]
                        .iter()
                        .enumerate()
                        .all(|(i, s)| usize::from(*s) == i) =>
                {
                    Loc::Regs(r, t.lanes)
                }
                _ => return Err(unsupported("a swizzle that reorders lanes")),
            },
            Op::LoadDst(_) => {
                self.reads_tile = true;
                let word = self.alloc(1)?;
                let mut read = mov(temp(word), PIXOUT0);
                read.olchk = true;
                self.groups.push(read.build()?);
                let dest = self.alloc(4)?;
                self.groups.push(unpack_unorm8x4(temp(dest), temp(word)).build()?);
                self.release(temp(word), 1);
                Loc::Regs(temp(dest), 4)
            }
            Op::StoreOutput(_, v) => {
                if at + 1 != self.m.body.len() {
                    return Err(unsupported("an output written before the end"));
                }
                let Loc::Regs(r, 4) = self.loc(at, v)? else {
                    return Err(unsupported("an output outside four registers"));
                };
                if r.bank != bank::TEMP {
                    return Err(unsupported("an output straight from a shared register"));
                }
                let mut m: Main = pack_unorm8x4(PIXOUT0, r);
                m.olchk = true;
                m.end = true;
                self.groups.push(m.build()?);
                return Ok(());
            }
            _ => return Err(unsupported("an operation outside the subset")),
        };
        self.locs[at] = Some(loc);
        Ok(())
    }
}

fn r_none() -> Reg {
    ZERO
}

/// Compiles a fragment module for the layout's registers.
pub fn compile(m: &Module, layout: Layout, opts: Options) -> Result<Program, CodegenError> {
    validate(m).map_err(CodegenError::Ir)?;
    if m.interface.stage != Stage::Fragment {
        return Err(CodegenError::Unsupported { at: 0, what: "a stage other than fragment" });
    }
    if m.interface.outputs.iter().any(|o| o.ty != Type::new(Scalar::F32, 4))
        || m.interface.outputs.len() != 1
    {
        return Err(CodegenError::Unsupported {
            at: 0,
            what: "outputs other than one four-lane colour",
        });
    }
    let n = m.body.len();
    // Aliases (lanes, splats, identity swizzles) extend their root's life.
    let mut root: Vec<usize> = (0..n).collect();
    for (i, ins) in m.body.iter().enumerate() {
        if let Op::Extract(v, _) | Op::Splat(v) | Op::Swizzle(v, _) = ins.op {
            root[i] = root[v.0 as usize];
        }
    }
    let mut last_use = vec![0usize; n];
    for (i, ins) in m.body.iter().enumerate() {
        let mut mark = |v: &ValueId| {
            let r = root[v.0 as usize];
            last_use[r] = last_use[r].max(i);
        };
        match &ins.op {
            Op::Unary(_, a)
            | Op::Convert(a)
            | Op::Extract(a, _)
            | Op::Swizzle(a, _)
            | Op::Splat(a) => mark(a),
            Op::Binary(_, a, b) | Op::Compare(_, a, b) => {
                mark(a);
                mark(b);
            }
            Op::Fma(a, b, c) | Op::Select(a, b, c) => {
                mark(a);
                mark(b);
                mark(c);
            }
            Op::Construct(parts) => parts.iter().for_each(&mut mark),
            Op::Sample { coords, lod, .. } => {
                mark(coords);
                if let Lod::Level(l) | Lod::Bias(l) = lod {
                    mark(l);
                }
            }
            Op::StoreOutput(_, v) => mark(v),
            _ => {}
        }
    }
    let mut g = Gen {
        m,
        layout,
        opts,
        groups: Vec::new(),
        locs: vec![None; n],
        root,
        last_use,
        free: [true; MAX_TEMPS],
        high: 0,
        reads_tile: false,
    };
    for at in 0..n {
        g.instr(at)?;
        // Values whose last use this was give their temporaries back, and so does a value never
        // used (its last use stays 0: nothing can use a value at instruction 0).
        for v in 0..=at {
            let dies = if v < at { g.last_use[v] == at } else { g.last_use[v] == 0 };
            if g.root[v] == v && dies {
                if let Some(Loc::Regs(r, lanes)) = g.locs[v] {
                    g.release(r, lanes);
                    g.locs[v] = None;
                }
            }
        }
    }
    if !g.groups.last().is_some_and(|l| l.header.end) {
        return Err(CodegenError::Unsupported { at: n, what: "a program that writes no output" });
    }
    align_end(&mut g.groups)?;
    let temps = g.high.max(1).div_ceil(4) as u8;
    Ok(Program {
        groups: g.groups,
        temps,
        pass_type: if g.reads_tile { PASS_TRANSLUCENT } else { PASS_OPAQUE },
    })
}
