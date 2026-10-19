// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The board GPU's backend (TASK-0215 5c-2 P3c, ADR-0072): a fragment module of our IR compiled
//! into instruction groups through the assembler. The compositor's subset, chosen by what the
//! board measured (5c-2 cycle 14): a texture sampled at an interpolated input becomes one
//! iterate-and-sample word (or iterate, fence, sample, fence when fusing is off); a multiply
//! over four channels one repeated group, a shared operand always the first source; the tile
//! read a bypass and an unpack; the colour packed straight into the pixel output, which ends the
//! program; the pass type opaque unless the program reads the tile. A constant is a constant
//! register's value or one of the pipeline's constant block (shared registers the state load
//! fills, after the push constants) — never an instruction. Registers are temporaries
//! handed out first-fit and taken back at a value's last use; an operation that steps lane by
//! lane may write over an operand that dies with it (an unpack may not: it reads its source on
//! every repetition). What the subset lacks is refused by name.

use alloc::vec;
use alloc::vec::Vec;

mod arith;

use super::alu::op;
use super::asm::{
    ctrl, mov, pack_unorm8x4, sample_2d, shared, temp, unpack_unorm8x4, Main, PIXOUT0,
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

    /// The constant block's first shared register: after `push_bytes` of push constants, at a
    /// multiple of four.
    pub fn constants(&self, push_bytes: u32) -> u16 {
        self.push + ((push_bytes / 4).div_ceil(4) * 4) as u16
    }
}

/// The most values a pipeline's constant block holds: the widest blur windowd asks (radius 40,
/// TASK-0215 5c-3 P4b) with room. The state load's DMA moves up to 4095 dwords and the library's
/// DMA record names them in a byte (255); a source names a shared register in eleven bits.
pub const MAX_CONSTANTS: usize = 128;

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
    /// The constant block: the values no constant register holds, in register order from
    /// [`Layout::constants`].
    pub constants: Vec<u32>,
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
    /// The constant block so far, and its first shared register.
    consts: Vec<u32>,
    const_base: u16,
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

    /// Takes `n` temporaries from `start` when every one is free.
    fn claim(&mut self, start: usize, n: usize) -> bool {
        let ok = start + n <= MAX_TEMPS && self.free[start..start + n].iter().all(|f| *f);
        if ok {
            self.free[start..start + n].fill(false);
            self.high = self.high.max(start + n);
        }
        ok
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

    /// A value in registers: an input the arithmetic reads is iterated here, once (a sample
    /// of it may already have iterated it inside its own word).
    fn placed(&mut self, at: usize, v: ValueId) -> Result<Loc, CodegenError> {
        match self.loc(at, v)? {
            Loc::Input(i) => {
                let l = self.iterate(at, i)?;
                self.locs[v.0 as usize] = Some(l);
                Ok(l)
            }
            l => Ok(l),
        }
    }

    /// An arithmetic operand of `lanes` lanes (or one register for every lane).
    fn operand(&mut self, at: usize, v: ValueId, lanes: u8) -> Result<Loc, CodegenError> {
        match self.placed(at, v)? {
            l @ Loc::Regs(_, n) if n == lanes || n == 1 && lanes == 1 => Ok(l),
            l @ Loc::Broadcast(_) => Ok(l),
            _ => Err(CodegenError::Unsupported { at, what: "an operand of another width" }),
        }
    }

    fn instr(&mut self, at: usize) -> Result<(), CodegenError> {
        let ins = &self.m.body[at];
        let t = ins.ty;
        let unsupported = |what| CodegenError::Unsupported { at, what };
        // The arithmetic here is the float unit's: integer and boolean arithmetic is refused.
        let arith = matches!(ins.op, Op::Unary(..) | Op::Binary(..) | Op::Fma(..));
        if arith && t.scalar != Scalar::F32 {
            return Err(unsupported("arithmetic on other than 32-bit floats"));
        }
        let loc = match ins.op.clone() {
            // A constant is a constant register's value, bit for bit (`rogue::constants`).
            Op::Const(bits) if t == Type::F32 => self.constant(at, bits[0])?,
            Op::Const(_) => return Err(unsupported("a vector or non-float constant")),
            Op::LoadInput(i) => Loc::Input(i),
            Op::LoadPush(off) => Loc::Regs(shared(self.layout.push + (off / 4) as u16), t.lanes),
            Op::Sample { coords, lod, .. } => self.sample(at, coords, lod)?,
            Op::Binary(BinOp::Mul, a, b) => self.arith(at, op::FMUL, a, b, false, t.lanes)?,
            Op::Binary(BinOp::Add, a, b) => self.arith(at, op::FADD, a, b, false, t.lanes)?,
            // a − b = (−b) + a.
            Op::Binary(BinOp::Sub, a, b) => self.arith(at, op::FADD, b, a, true, t.lanes)?,
            Op::Binary(BinOp::Min, a, b) => self.minmax(at, a, b, false, t.lanes)?,
            Op::Binary(BinOp::Max, a, b) => self.minmax(at, a, b, true, t.lanes)?,
            Op::Binary(BinOp::Div, a, b) => self.div(at, a, b, t.lanes)?,
            Op::Unary(u, a) => self.unary(at, u, a, t.lanes)?,
            Op::Construct(parts) => self.construct(at, &parts, t.lanes)?,
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
            // A swizzle of consecutive lanes in order (`.xy`, `.zw`, `.yzw`) is its registers
            // from the first lane on; one register for every lane stays that.
            Op::Swizzle(v, sel) => match self.placed(at, v)? {
                Loc::Regs(r, n)
                    if sel[..usize::from(t.lanes)]
                        .iter()
                        .enumerate()
                        .all(|(i, s)| usize::from(*s) == usize::from(sel[0]) + i)
                        && sel[0] + t.lanes <= n =>
                {
                    Loc::Regs(Reg { index: r.index + u16::from(sel[0]), ..r }, t.lanes)
                }
                Loc::Broadcast(r) => Loc::Broadcast(r),
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
        consts: Vec::new(),
        const_base: layout.constants(m.interface.push_bytes),
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
        constants: g.consts,
    })
}
