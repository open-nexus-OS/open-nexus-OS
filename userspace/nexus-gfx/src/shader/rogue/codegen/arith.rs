// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The backend's arithmetic (TASK-0215 5c-2 P3c, P5a): adds, multiplies and fused multiply-adds
//! over the lanes; the single-source unit (reciprocal, inverse square root, base-2 exponent and
//! logarithm — a square root as the reciprocal of the inverse square root, so √0 is 0);
//! negation, absolute value, floor and saturation as an add of constant register 0 with the
//! modifier (the compiler's `fadd.sat x, sc0`); min and max as its one group of bypasses, test
//! and conditional move; a division as a reciprocal and a product; a constant as its constant
//! register or its register in the pipeline's constant block; a vector built from parts in
//! consecutive registers, the first part's own when it
//! dies there with room behind it. A repeated group steps every register operand — constant
//! registers too — so an operation with one register for every lane (a broadcast) takes a group a
//! lane; a broadcast constant in an add, multiply or multiply-add is therefore a run of the
//! constant block, one register a lane, and the group repeats over it.

use super::{CodegenError, Gen, Loc, MAX_CONSTANTS};
use crate::shader::ir::{UnOp, ValueId};
use crate::shader::rogue::alu::{op, sngl};
use crate::shader::rogue::asm::{
    fam_mods, fmad, minmax, mov, shared, sngl as single, special, temp, FamMods, Main, ZERO,
};
use crate::shader::rogue::constants;
use crate::shader::rogue::io::{bank, Reg};

/// An operand's register for one lane.
fn lane(l: Loc, lane: u16) -> Reg {
    match l {
        Loc::Regs(r, _) => Reg { index: r.index + lane, ..r },
        Loc::Broadcast(r) => r,
        Loc::Input(_) => ZERO,
    }
}

fn at_lane(r: Reg, lane: u16) -> Reg {
    Reg { index: r.index + lane, ..r }
}

fn is_shared(l: &Loc) -> bool {
    matches!(l, Loc::Regs(r, _) | Loc::Broadcast(r) if r.bank == bank::SHARED)
}

impl Gen<'_> {
    /// An operand moved into fresh temporaries (a shared register cannot be the second source:
    /// its bank field is two bits) — a repeated move, or one for a broadcast; the temporaries
    /// to give back after use.
    fn in_temps(&mut self, l: Loc, lanes: u8) -> Result<(Loc, (Reg, u8)), CodegenError> {
        let n = if matches!(l, Loc::Broadcast(_)) { 1 } else { lanes };
        let t = temp(self.alloc(usize::from(n))?);
        let mut m = mov(t, lane(l, 0));
        m.repeat = n;
        self.groups.push(m.build()?);
        let placed = if n == 1 && lanes > 1 { Loc::Broadcast(t) } else { Loc::Regs(t, n) };
        Ok((placed, (t, n)))
    }

    /// `dst = a op b` (add or multiply) over the lanes, a shared operand first.
    pub(super) fn arith(
        &mut self,
        at: usize,
        op: u8,
        a: ValueId,
        b: ValueId,
        s0neg: bool,
        lanes: u8,
    ) -> Result<Loc, CodegenError> {
        let (la, lb) = (self.operand(at, a, lanes)?, self.operand(at, b, lanes)?);
        let dst = self.step_dest(at, lanes, &[a, b])?;
        self.arith_locs(at, op, la, lb, s0neg, lanes, dst)?;
        Ok(Loc::Regs(dst, lanes))
    }

    /// [`Self::arith`] over operands already placed, into `dst`.
    #[allow(clippy::too_many_arguments)]
    fn arith_locs(
        &mut self,
        at: usize,
        op: u8,
        la: Loc,
        lb: Loc,
        s0neg: bool,
        lanes: u8,
        dst: Reg,
    ) -> Result<(), CodegenError> {
        let mut locs = [la, lb];
        self.constant_runs(at, &mut locs, lanes)?;
        let [mut la, mut lb] = locs;
        if is_shared(&lb) && !s0neg {
            core::mem::swap(&mut la, &mut lb);
        }
        let mut moved = None;
        if is_shared(&lb) {
            let (l, t) = self.in_temps(lb, lanes)?;
            lb = l;
            moved = Some(t);
        }
        let broadcast = matches!(la, Loc::Broadcast(_)) || matches!(lb, Loc::Broadcast(_));
        let groups = if broadcast { lanes } else { 1 };
        let mods = FamMods { s0neg, ..FamMods::default() };
        for l in 0..u16::from(groups) {
            let mut m: Main = fam_mods(op, at_lane(dst, l), lane(la, l), lane(lb, l), mods);
            m.repeat = if broadcast { 1 } else { lanes };
            self.groups.push(m.build()?);
        }
        if let Some((t, n)) = moved {
            self.release(t, n);
        }
        Ok(())
    }

    pub(super) fn fma(
        &mut self,
        at: usize,
        a: ValueId,
        b: ValueId,
        c: ValueId,
        lanes: u8,
    ) -> Result<Loc, CodegenError> {
        let mut locs =
            [self.operand(at, a, lanes)?, self.operand(at, b, lanes)?, self.operand(at, c, lanes)?];
        self.constant_runs(at, &mut locs, lanes)?;
        let [mut la, mut lb, lc] = locs;
        // The product commutes: a shared factor goes first; two are one moved into temporaries.
        if is_shared(&lb) {
            core::mem::swap(&mut la, &mut lb);
        }
        let mut moved = None;
        if is_shared(&lb) {
            let (l, t) = self.in_temps(lb, lanes)?;
            lb = l;
            moved = Some(t);
        }
        let dst = self.step_dest(at, lanes, &[c, a])?;
        let broadcast = [la, lb, lc].iter().any(|l| matches!(l, Loc::Broadcast(_)));
        let groups = if broadcast { lanes } else { 1 };
        for l in 0..u16::from(groups) {
            let mut m = fmad(at_lane(dst, l), lane(la, l), lane(lb, l), lane(lc, l));
            m.repeat = if broadcast { 1 } else { lanes };
            self.groups.push(m.build()?);
        }
        if let Some((t, n)) = moved {
            self.release(t, n);
        }
        Ok(Loc::Regs(dst, lanes))
    }

    /// The value `r` holds when it is a constant: a plain constant register, or a register of
    /// the constant block.
    fn constant_bits(&self, r: Reg) -> Option<u32> {
        match r.bank {
            bank::SPECIAL => constants::TABLE
                .iter()
                .find(|c| c.index == r.index && !c.neg && !c.flr)
                .map(|c| c.bits),
            bank::SHARED => {
                let i = usize::from(r.index.checked_sub(self.const_base)?);
                self.consts.get(i).copied()
            }
            _ => None,
        }
    }

    /// `lanes` copies of `bits` in consecutive registers of the constant block: a run already
    /// there, else the block's trailing copies extended (a value's scalar register becomes its
    /// run's first), else appended.
    fn constant_run(&mut self, at: usize, bits: u32, lanes: u8) -> Result<Loc, CodegenError> {
        let n = usize::from(lanes);
        let start = match self.consts.windows(n).position(|w| w.iter().all(|c| *c == bits)) {
            Some(i) => i,
            None => {
                let tail = self.consts.iter().rev().take_while(|c| **c == bits).count();
                let need = n - tail.min(n);
                if self.consts.len() + need > MAX_CONSTANTS {
                    return Err(CodegenError::Unsupported {
                        at,
                        what: "more constants than a pipeline's block holds",
                    });
                }
                self.consts.extend(core::iter::repeat_n(bits, need));
                self.consts.len() - n
            }
        };
        Ok(Loc::Regs(shared(self.const_base + start as u16), lanes))
    }

    /// A lane-stepping group's operands with every broadcast a constant: each broadcast made a
    /// run of the constant block, so one repeated group covers the lanes where a broadcast takes
    /// a group a lane (a shared operand more costs one repeated move at most). A computed
    /// broadcast among them keeps the group a lane, and the constants their registers — and so
    /// does a block too full for the runs (a wide blur's weights, TASK-0215 5c-3 P4b): every run
    /// or none, the block as it was when they do not all fit.
    fn constant_runs(
        &mut self,
        at: usize,
        locs: &mut [Loc],
        lanes: u8,
    ) -> Result<(), CodegenError> {
        if lanes < 2 {
            return Ok(());
        }
        let mut bits = [None; 3];
        for (b, l) in bits.iter_mut().zip(locs.iter()) {
            if let Loc::Broadcast(r) = l {
                match self.constant_bits(*r) {
                    Some(v) => *b = Some(v),
                    None => return Ok(()),
                }
            }
        }
        let kept = self.consts.len();
        let mut runs = [None; 3];
        for (run, b) in runs.iter_mut().zip(bits.iter()) {
            if let Some(v) = b {
                match self.constant_run(at, *v, lanes) {
                    Ok(loc) => *run = Some(loc),
                    Err(_) => {
                        self.consts.truncate(kept);
                        return Ok(());
                    }
                }
            }
        }
        for (l, run) in locs.iter_mut().zip(runs) {
            if let Some(loc) = run {
                *l = loc;
            }
        }
        Ok(())
    }

    /// A constant: the constant register that holds the value as it is, else its register in
    /// the pipeline's constant block — the same register for every use of the value, no
    /// instruction either way.
    pub(super) fn constant(&mut self, at: usize, bits: u32) -> Result<Loc, CodegenError> {
        if let Some(r) = constants::find(bits).filter(|r| !r.neg && !r.flr) {
            return Ok(Loc::Regs(special(r.index), 1));
        }
        let i = match self.consts.iter().position(|c| *c == bits) {
            Some(i) => i,
            None if self.consts.len() < MAX_CONSTANTS => {
                self.consts.push(bits);
                self.consts.len() - 1
            }
            None => {
                return Err(CodegenError::Unsupported {
                    at,
                    what: "more constants than a pipeline's block holds",
                })
            }
        };
        Ok(Loc::Regs(shared(self.const_base + i as u16), 1))
    }

    /// `u(a)` over the lanes; a broadcast operand computed once and broadcast again.
    pub(super) fn unary(
        &mut self,
        at: usize,
        u: UnOp,
        a: ValueId,
        lanes: u8,
    ) -> Result<Loc, CodegenError> {
        let la = self.operand(at, a, lanes)?;
        let once = matches!(la, Loc::Broadcast(_));
        let n = if once { 1 } else { lanes };
        let dst = if once { temp(self.alloc(1)?) } else { self.step_dest(at, n, &[a])? };
        let single_op = match u {
            UnOp::Rcp => Some(sngl::RCP),
            UnOp::Rsqrt | UnOp::Sqrt => Some(sngl::RSQ),
            UnOp::Exp2 => Some(sngl::EXP),
            UnOp::Log2 => Some(sngl::LOG),
            _ => None,
        };
        if let Some(first) = single_op {
            let mut m = single(first, dst, lane(la, 0));
            m.repeat = n;
            self.groups.push(m.build()?);
            if u == UnOp::Sqrt {
                let mut m = single(sngl::RCP, dst, dst);
                m.repeat = n;
                self.groups.push(m.build()?);
            }
        } else {
            let mods = match u {
                UnOp::Neg => FamMods { s0neg: true, ..FamMods::default() },
                UnOp::Abs => FamMods { s0abs: true, ..FamMods::default() },
                UnOp::Floor => FamMods { s0flr: true, ..FamMods::default() },
                UnOp::Sat => FamMods { sat: true, ..FamMods::default() },
                _ => return Err(CodegenError::Unsupported { at, what: "a fractional part" }),
            };
            // A group a lane: constant register 0 must not step.
            for l in 0..u16::from(n) {
                let m = fam_mods(op::FADD, at_lane(dst, l), lane(la, l), ZERO, mods);
                self.groups.push(m.build()?);
            }
        }
        Ok(if once { Loc::Broadcast(dst) } else { Loc::Regs(dst, n) })
    }

    /// min or max over the lanes, a group a lane.
    pub(super) fn minmax(
        &mut self,
        at: usize,
        a: ValueId,
        b: ValueId,
        max: bool,
        lanes: u8,
    ) -> Result<Loc, CodegenError> {
        let (la, lb) = (self.operand(at, a, lanes)?, self.operand(at, b, lanes)?);
        let once = matches!((la, lb), (Loc::Broadcast(_), Loc::Broadcast(_)));
        let n = if once { 1 } else { lanes };
        let dst = if once { temp(self.alloc(1)?) } else { self.step_dest(at, n, &[a, b])? };
        for l in 0..u16::from(n) {
            self.groups.push(minmax(at_lane(dst, l), lane(la, l), lane(lb, l), max).build()?);
        }
        Ok(if once { Loc::Broadcast(dst) } else { Loc::Regs(dst, n) })
    }

    /// `a / b`: the reciprocal of `b` into temporaries, then the product.
    pub(super) fn div(
        &mut self,
        at: usize,
        a: ValueId,
        b: ValueId,
        lanes: u8,
    ) -> Result<Loc, CodegenError> {
        let (la, lb) = (self.operand(at, a, lanes)?, self.operand(at, b, lanes)?);
        let once = matches!(lb, Loc::Broadcast(_));
        let n = if once { 1 } else { lanes };
        let r = temp(self.alloc(usize::from(n))?);
        let mut m = single(sngl::RCP, r, lane(lb, 0));
        m.repeat = n;
        self.groups.push(m.build()?);
        let lr = if once { Loc::Broadcast(r) } else { Loc::Regs(r, n) };
        let dst = self.step_dest(at, lanes, &[a])?;
        self.arith_locs(at, op::FMUL, la, lr, false, lanes, dst)?;
        self.release(r, n);
        Ok(Loc::Regs(dst, lanes))
    }

    /// The lanes of `parts` in consecutive temporaries: the first part's own registers when it
    /// dies here and the registers after them are free, else fresh ones; every lane not yet in
    /// place moved in, a group a lane.
    pub(super) fn construct(
        &mut self,
        at: usize,
        parts: &[ValueId],
        lanes: u8,
    ) -> Result<Loc, CodegenError> {
        let mut regs = alloc::vec::Vec::with_capacity(usize::from(lanes));
        for p in parts {
            let l = self.placed(at, *p)?;
            let n = match l {
                Loc::Regs(_, n) => n,
                Loc::Broadcast(_) => self.m.body[p.0 as usize].ty.lanes,
                Loc::Input(_) => {
                    return Err(CodegenError::Unsupported { at, what: "an input in a vector" })
                }
            };
            regs.extend((0..u16::from(n)).map(|i| lane(l, i)));
        }
        if regs.len() != usize::from(lanes) {
            return Err(CodegenError::Unsupported { at, what: "a vector of other lanes" });
        }
        let first = parts.first().and_then(|p| self.dying_regs(at, *p).map(|r| (*p, r)));
        let dst = match first {
            Some((p, (r, n)))
                if self.claim(usize::from(r.index) + usize::from(n), usize::from(lanes - n)) =>
            {
                // The first part's registers pass to the vector.
                self.locs[p.0 as usize] = None;
                r
            }
            _ => temp(self.alloc(usize::from(lanes))?),
        };
        for (i, r) in regs.iter().enumerate() {
            let to = at_lane(dst, i as u16);
            if *r != to {
                self.groups.push(mov(to, *r).build()?);
            }
        }
        Ok(Loc::Regs(dst, lanes))
    }
}
