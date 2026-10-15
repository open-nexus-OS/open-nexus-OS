// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! WGSL's math functions in our IR (TASK-0215 5c-2 P2b, P5a): the unary ones the float unit
//! has, min and max, `clamp` (its unit form the free saturation), `fma`, `mix`, `dot` as a chain
//! of multiply-adds and `length` as the square root of a vector's dot with itself.

use naga::{Expression, Handle, Literal, MathFunction};
use nexus_gfx::shader::ir::{BinOp, UnOp, ValueId};

use super::{FrontendError, Lowering, Result};

impl Lowering<'_> {
    /// The value of a float literal, or of a splat or constant of one.
    fn literal_f32(&self, h: Handle<Expression>) -> Option<f32> {
        let lit = match self.fun.expressions[h] {
            Expression::Literal(lit) => lit,
            Expression::Splat { value, .. } => return self.literal_f32(value),
            Expression::Constant(c) => {
                match self.module.global_expressions[self.module.constants[c].init] {
                    Expression::Literal(lit) => lit,
                    _ => return None,
                }
            }
            _ => return None,
        };
        match lit {
            Literal::F32(x) => Some(x),
            Literal::AbstractFloat(x) => Some(x as f32),
            _ => None,
        }
    }

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
    ) -> Result<ValueId> {
        let lanes = self.ty(h)?.lanes;
        let a = self.value(arg)?;
        let operand = |s: &mut Self, e: Option<Handle<Expression>>| -> Result<ValueId> {
            let v = s.value(e.ok_or(FrontendError::Unsupported("a missing math operand"))?)?;
            Ok(s.widen(v, lanes))
        };
        let un = |s: &mut Self, op| Ok(s.b.unary(op, a));
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
                let b = operand(self, arg1)?;
                let op = if fun == MathFunction::Min { BinOp::Min } else { BinOp::Max };
                Ok(self.b.binary(op, a, b))
            }
            MathFunction::Clamp => {
                // clamp(x, 0, 1) is the saturation the float unit applies for free.
                let unit = (
                    arg1.and_then(|e| self.literal_f32(e)),
                    arg2.and_then(|e| self.literal_f32(e)),
                );
                if unit == (Some(0.0), Some(1.0)) {
                    return un(self, UnOp::Sat);
                }
                let (lo, hi) = (operand(self, arg1)?, operand(self, arg2)?);
                let m = self.b.binary(BinOp::Min, a, hi);
                Ok(self.b.binary(BinOp::Max, m, lo))
            }
            MathFunction::Dot => {
                let b =
                    self.value(arg1.ok_or(FrontendError::Unsupported("a missing math operand"))?)?;
                Ok(self.dot(a, b))
            }
            MathFunction::Length => {
                if self.b.ty(a).lanes == 1 {
                    return un(self, UnOp::Abs);
                }
                let d = self.dot(a, a);
                Ok(self.b.unary(UnOp::Sqrt, d))
            }
            MathFunction::Fma => {
                let (b, c) = (operand(self, arg1)?, operand(self, arg2)?);
                Ok(self.b.fma(a, b, c))
            }
            MathFunction::Mix => {
                // mix(a, b, t) = (b − a)·t + a
                let (b, t) = (operand(self, arg1)?, operand(self, arg2)?);
                let d = self.b.binary(BinOp::Sub, b, a);
                Ok(self.b.fma(d, t, a))
            }
            _ => Err(FrontendError::Unsupported("a math function outside the subset")),
        }
    }
}
