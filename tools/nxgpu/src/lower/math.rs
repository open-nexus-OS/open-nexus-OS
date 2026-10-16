// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! WGSL's math functions in our IR (TASK-0215 5c-2 P2b, P5a, P5d-2): folded when the pipeline
//! fixes every operand; else the unary ones the float unit has, min and max, `clamp` (its unit
//! form the free saturation), `fma`, `mix`, `dot` as a chain of multiply-adds and `length` as the
//! square root of a vector's dot with itself.

use naga::{Expression, Handle, MathFunction};
use nexus_gfx::shader::ir::{BinOp, UnOp, ValueId};

use super::expr::{fixed, is_f32};
use super::fold;
use super::{FrontendError, Lowered, Lowering, Result};

impl Lowering<'_> {
    /// `a · b`: the first lanes' product, then a multiply-add a lane.
    fn dot(&mut self, a: ValueId, b: ValueId) -> ValueId {
        let lanes = self.b.ty(a).lanes;
        let (a0, b0) = (self.b.extract(a, 0), self.b.extract(b, 0));
        let mut acc = self.b.binary(BinOp::Mul, a0, b0);
        for i in 1..lanes {
            let (ai, bi) = (self.b.extract(a, i), self.b.extract(b, i));
            acc = self.b.fma(ai, bi, acc);
        }
        acc
    }

    pub(super) fn math(
        &mut self,
        h: Handle<Expression>,
        fun: MathFunction,
        arg: Handle<Expression>,
        arg1: Option<Handle<Expression>>,
        arg2: Option<Handle<Expression>>,
    ) -> Result<Lowered> {
        let mut args = Vec::with_capacity(3);
        for e in [Some(arg), arg1, arg2].into_iter().flatten() {
            args.push(self.lower(e)?);
        }
        let all: Option<Vec<_>> = args.iter().map(|l| fixed(*l)).collect();
        if let Some(k) = all.and_then(|ks| fold::math(fun, &ks)) {
            return Ok(Lowered::Const(k));
        }
        // Operand n a float the pipeline fixes to `x` in every lane.
        let is = |n: usize, x: f32| args.get(n).is_some_and(|l| is_f32(*l, x));
        let (unit_low, unit_high) = (is(1, 0.0), is(2, 1.0));
        let (zero_addend, unit_factor) = (is(2, 0.0), [is(0, 1.0), is(1, 1.0)]);
        let lanes = self.ty(h)?.lanes;
        let a = self.as_value(args[0])?;
        let operand = |s: &mut Self, n: usize| -> Result<ValueId> {
            let l = *args.get(n).ok_or(FrontendError::Unsupported("a missing math operand"))?;
            let v = s.as_value(l)?;
            Ok(s.widen(v, lanes))
        };
        let un = |s: &mut Self, op| Ok(Lowered::Value(s.b.unary(op, a)));
        let value = |v| Ok(Lowered::Value(v));
        match fun {
            MathFunction::Abs => un(self, UnOp::Abs),
            MathFunction::Floor => un(self, UnOp::Floor),
            MathFunction::Fract => un(self, UnOp::Fract),
            MathFunction::Sqrt => un(self, UnOp::Sqrt),
            MathFunction::InverseSqrt => un(self, UnOp::Rsqrt),
            MathFunction::Exp2 => un(self, UnOp::Exp2),
            MathFunction::Log2 => un(self, UnOp::Log2),
            MathFunction::Saturate => un(self, UnOp::Sat),
            MathFunction::Min | MathFunction::Max => {
                let b = operand(self, 1)?;
                let op = if fun == MathFunction::Min { BinOp::Min } else { BinOp::Max };
                value(self.b.binary(op, a, b))
            }
            // clamp(x, 0, 1) is the saturation the float unit applies for free.
            MathFunction::Clamp if unit_low && unit_high => un(self, UnOp::Sat),
            MathFunction::Clamp => {
                let (lo, hi) = (operand(self, 1)?, operand(self, 2)?);
                let m = self.b.binary(BinOp::Min, a, hi);
                value(self.b.binary(BinOp::Max, m, lo))
            }
            MathFunction::Dot => {
                let l = *args.get(1).ok_or(FrontendError::Unsupported("a missing math operand"))?;
                let b = self.as_value(l)?;
                value(self.dot(a, b))
            }
            MathFunction::Length => {
                if self.b.ty(a).lanes == 1 {
                    return un(self, UnOp::Abs);
                }
                let d = self.dot(a, a);
                value(self.b.unary(UnOp::Sqrt, d))
            }
            // fma(a, b, 0) is a · b, fma(1, b, c) is b + c, fma(a, 1, c) is a + c (the
            // identities of `binary`): an unrolled sum's first tap is a multiply.
            MathFunction::Fma if zero_addend => {
                let (a, b) = (operand(self, 0)?, operand(self, 1)?);
                value(self.b.binary(BinOp::Mul, a, b))
            }
            MathFunction::Fma if unit_factor[0] || unit_factor[1] => {
                let (x, c) = (operand(self, usize::from(unit_factor[0]))?, operand(self, 2)?);
                value(self.b.binary(BinOp::Add, x, c))
            }
            MathFunction::Fma => {
                let (b, c) = (operand(self, 1)?, operand(self, 2)?);
                value(self.b.fma(a, b, c))
            }
            MathFunction::Mix => {
                // mix(a, b, t) = (b − a)·t + a
                let (b, t) = (operand(self, 1)?, operand(self, 2)?);
                let d = self.b.binary(BinOp::Sub, b, a);
                value(self.b.fma(d, t, a))
            }
            _ => Err(FrontendError::Unsupported("a math function outside the subset")),
        }
    }
}
