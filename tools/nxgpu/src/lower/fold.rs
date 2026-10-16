// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Constants folded while lowering (TASK-0215 5c-2 P5d-2): an operation whose operands are all
//! fixed when the pipeline is built — literals, constants, overrides, a loop's counter — becomes a
//! constant here, never an instruction. IEEE arithmetic lane by lane, a scalar operand widened to
//! the other's lanes; the functions IEEE leaves inexact (exponentials, logarithms, powers) from
//! `libm`, so a library builds bit for bit alike on every host. An operation without a fold here
//! is left to run.

use naga::{BinaryOperator, Literal, MathFunction, UnaryOperator};
use nexus_gfx::shader::ir::{Scalar, Type};

/// A fixed value: its type and each lane's bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct K {
    pub ty: Type,
    pub bits: [u32; 4],
}

impl K {
    pub fn of_literal(lit: Literal) -> Option<K> {
        let (ty, bits) = match lit {
            Literal::F32(x) => (Type::F32, x.to_bits()),
            Literal::F16(x) => (Type::new(Scalar::F16, 1), x.to_f32().to_bits()),
            Literal::AbstractFloat(x) => (Type::F32, (x as f32).to_bits()),
            Literal::I32(x) => (Type::new(Scalar::I32, 1), x as u32),
            Literal::AbstractInt(x) => (Type::new(Scalar::I32, 1), x as i32 as u32),
            Literal::U32(x) => (Type::new(Scalar::U32, 1), x),
            Literal::Bool(x) => (Type::new(Scalar::Bool, 1), u32::from(x)),
            _ => return None,
        };
        Some(K { ty, bits: [bits, 0, 0, 0] })
    }

    pub fn zero(ty: Type) -> K {
        K { ty, bits: [0; 4] }
    }

    fn lane(&self, i: usize) -> u32 {
        self.bits[if self.ty.lanes == 1 { 0 } else { i }]
    }

    pub fn lane_f32(&self, i: usize) -> f32 {
        f32::from_bits(self.lane(i))
    }

    fn of(scalar: Scalar, lanes: u8, f: impl Fn(usize) -> u32) -> K {
        let mut bits = [0; 4];
        for (i, b) in bits.iter_mut().enumerate().take(usize::from(lanes)) {
            *b = f(i);
        }
        K { ty: Type::new(scalar, lanes), bits }
    }

    pub fn splat(self, lanes: u8) -> K {
        K::of(self.ty.scalar, lanes, |_| self.bits[0])
    }

    pub fn extract(self, i: u8) -> Option<K> {
        (i < self.ty.lanes).then(|| K::of(self.ty.scalar, 1, |_| self.bits[usize::from(i)]))
    }

    pub fn swizzle(self, sel: &[u8]) -> Option<K> {
        if sel.iter().any(|s| *s >= self.ty.lanes) {
            return None;
        }
        Some(K::of(self.ty.scalar, sel.len() as u8, |i| self.bits[usize::from(sel[i])]))
    }

    pub fn compose(parts: &[K]) -> Option<K> {
        let scalar = parts.first()?.ty.scalar;
        let mut bits = Vec::with_capacity(4);
        for p in parts {
            if p.ty.scalar != scalar {
                return None;
            }
            bits.extend((0..usize::from(p.ty.lanes)).map(|i| p.bits[i]));
        }
        (1..=4).contains(&bits.len()).then(|| K::of(scalar, bits.len() as u8, |i| bits[i]))
    }

    /// Every lane the same bits: a constant one scalar and a splat make.
    pub fn uniform(&self) -> bool {
        (0..usize::from(self.ty.lanes)).all(|i| self.bits[i] == self.bits[0])
    }
}

fn floats(a: K, b: K, f: impl Fn(f32, f32) -> f32) -> K {
    let lanes = a.ty.lanes.max(b.ty.lanes);
    K::of(Scalar::F32, lanes, |i| f(a.lane_f32(i), b.lane_f32(i)).to_bits())
}

fn ints(a: K, b: K, f: impl Fn(u32, u32) -> Option<u32>) -> Option<K> {
    let lanes = a.ty.lanes.max(b.ty.lanes);
    let mut out = K::of(a.ty.scalar, lanes, |_| 0);
    for i in 0..usize::from(lanes) {
        out.bits[i] = f(a.lane(i), b.lane(i))?;
    }
    Some(out)
}

fn compare(a: K, b: K, f: impl Fn(core::cmp::Ordering) -> bool) -> Option<K> {
    let lanes = a.ty.lanes.max(b.ty.lanes);
    let mut out = K::of(Scalar::Bool, lanes, |_| 0);
    for i in 0..usize::from(lanes) {
        let ord = match a.ty.scalar {
            Scalar::F32 => a.lane_f32(i).partial_cmp(&b.lane_f32(i))?,
            Scalar::I32 => (a.lane(i) as i32).cmp(&(b.lane(i) as i32)),
            Scalar::U32 | Scalar::Bool => a.lane(i).cmp(&b.lane(i)),
            Scalar::F16 => return None,
        };
        out.bits[i] = u32::from(f(ord));
    }
    Some(out)
}

pub(super) fn binary(op: BinaryOperator, a: K, b: K) -> Option<K> {
    use core::cmp::Ordering::{Equal, Greater, Less};
    use BinaryOperator as B;
    if a.ty.scalar != b.ty.scalar {
        return None;
    }
    match (op, a.ty.scalar) {
        (B::Add, Scalar::F32) => Some(floats(a, b, |x, y| x + y)),
        (B::Subtract, Scalar::F32) => Some(floats(a, b, |x, y| x - y)),
        (B::Multiply, Scalar::F32) => Some(floats(a, b, |x, y| x * y)),
        (B::Divide, Scalar::F32) => Some(floats(a, b, |x, y| x / y)),
        (B::Add, Scalar::I32 | Scalar::U32) => ints(a, b, |x, y| Some(x.wrapping_add(y))),
        (B::Subtract, Scalar::I32 | Scalar::U32) => ints(a, b, |x, y| Some(x.wrapping_sub(y))),
        (B::Multiply, Scalar::I32 | Scalar::U32) => ints(a, b, |x, y| Some(x.wrapping_mul(y))),
        (B::Divide, Scalar::I32) => {
            ints(a, b, |x, y| (y != 0).then(|| (x as i32).wrapping_div(y as i32) as u32))
        }
        (B::Divide, Scalar::U32) => ints(a, b, |x, y| x.checked_div(y)),
        (B::Modulo, Scalar::F32) => Some(floats(a, b, |x, y| x % y)),
        (B::Modulo, Scalar::I32) => {
            ints(a, b, |x, y| (y != 0).then(|| (x as i32).wrapping_rem(y as i32) as u32))
        }
        (B::Modulo, Scalar::U32) => ints(a, b, |x, y| x.checked_rem(y)),
        (B::And, Scalar::I32 | Scalar::U32) => ints(a, b, |x, y| Some(x & y)),
        (B::InclusiveOr, Scalar::I32 | Scalar::U32) => ints(a, b, |x, y| Some(x | y)),
        (B::ExclusiveOr, Scalar::I32 | Scalar::U32) => ints(a, b, |x, y| Some(x ^ y)),
        (B::ShiftLeft, Scalar::I32 | Scalar::U32) => ints(a, b, |x, y| x.checked_shl(y)),
        (B::ShiftRight, Scalar::I32) => {
            ints(a, b, |x, y| (x as i32).checked_shr(y).map(|v| v as u32))
        }
        (B::ShiftRight, Scalar::U32) => ints(a, b, |x, y| x.checked_shr(y)),
        (B::Less, _) => compare(a, b, |o| o == Less),
        (B::LessEqual, _) => compare(a, b, |o| o != Greater),
        (B::Equal, _) => compare(a, b, |o| o == Equal),
        (B::NotEqual, _) => compare(a, b, |o| o != Equal),
        (B::GreaterEqual, _) => compare(a, b, |o| o != Less),
        (B::Greater, _) => compare(a, b, |o| o == Greater),
        (B::And | B::LogicalAnd, Scalar::Bool) => ints(a, b, |x, y| Some(x & y)),
        (B::InclusiveOr | B::LogicalOr, Scalar::Bool) => ints(a, b, |x, y| Some(x | y)),
        _ => None,
    }
}

pub(super) fn unary(op: UnaryOperator, a: K) -> Option<K> {
    match (op, a.ty.scalar) {
        (UnaryOperator::Negate, Scalar::F32) => {
            Some(K::of(Scalar::F32, a.ty.lanes, |i| (-a.lane_f32(i)).to_bits()))
        }
        (UnaryOperator::Negate, Scalar::I32) => {
            Some(K::of(Scalar::I32, a.ty.lanes, |i| (a.lane(i) as i32).wrapping_neg() as u32))
        }
        (UnaryOperator::LogicalNot, Scalar::Bool) => {
            Some(K::of(Scalar::Bool, a.ty.lanes, |i| u32::from(a.lane(i) == 0)))
        }
        (UnaryOperator::BitwiseNot, Scalar::I32 | Scalar::U32) => {
            Some(K::of(a.ty.scalar, a.ty.lanes, |i| !a.lane(i)))
        }
        _ => None,
    }
}

/// A conversion's value (float to integer truncates toward zero, saturating; to bool, non-zero).
pub(super) fn convert(a: K, to: Scalar) -> Option<K> {
    let lane = |i: usize| -> Option<u32> {
        Some(match (a.ty.scalar, to) {
            (x, y) if x == y => a.lane(i),
            (Scalar::I32, Scalar::F32) => (a.lane(i) as i32 as f32).to_bits(),
            (Scalar::U32, Scalar::F32) => (a.lane(i) as f32).to_bits(),
            (Scalar::F32, Scalar::I32) => a.lane_f32(i) as i32 as u32,
            (Scalar::F32, Scalar::U32) => a.lane_f32(i) as u32,
            (Scalar::I32, Scalar::U32) | (Scalar::U32, Scalar::I32) => a.lane(i),
            (Scalar::Bool, Scalar::F32) => (a.lane(i) as f32).to_bits(),
            (Scalar::Bool, Scalar::I32 | Scalar::U32) => a.lane(i),
            (Scalar::I32 | Scalar::U32, Scalar::Bool) => u32::from(a.lane(i) != 0),
            (Scalar::F32, Scalar::Bool) => u32::from(a.lane_f32(i) != 0.0),
            _ => return None,
        })
    };
    let mut out = K::of(to, a.ty.lanes, |_| 0);
    for i in 0..usize::from(a.ty.lanes) {
        out.bits[i] = lane(i)?;
    }
    Some(out)
}

pub(super) fn select(c: K, accept: K, reject: K) -> Option<K> {
    if c.ty.scalar != Scalar::Bool || accept.ty != reject.ty {
        return None;
    }
    Some(K::of(accept.ty.scalar, accept.ty.lanes, |i| {
        if c.lane(i) != 0 {
            accept.lane(i)
        } else {
            reject.lane(i)
        }
    }))
}

/// A bitcast's value: the bits as they are, read as `to`.
pub(super) fn bitcast(a: K, to: Scalar) -> Option<K> {
    let plain = |s: Scalar| matches!(s, Scalar::F32 | Scalar::I32 | Scalar::U32);
    (plain(to) && plain(a.ty.scalar)).then(|| K { ty: Type::new(to, a.ty.lanes), bits: a.bits })
}

/// The float math functions on fixed operands.
pub(super) fn math(fun: MathFunction, args: &[K]) -> Option<K> {
    use MathFunction as M;
    let a = *args.first()?;
    if args.iter().any(|k| k.ty.scalar != Scalar::F32) {
        return None;
    }
    let one =
        |f: fn(f32) -> f32| Some(K::of(Scalar::F32, a.ty.lanes, |i| f(a.lane_f32(i)).to_bits()));
    let lanes = args.iter().map(|k| k.ty.lanes).max().unwrap_or(1);
    let all = |f: &dyn Fn(usize) -> f32| Some(K::of(Scalar::F32, lanes, |i| f(i).to_bits()));
    let arg = |n: usize, i: usize| args.get(n).map_or(0.0, |k| k.lane_f32(i));
    match fun {
        M::Abs => one(f32::abs),
        M::Floor => one(f32::floor),
        M::Fract => one(|x| x - x.floor()),
        M::Sqrt => one(f32::sqrt),
        M::InverseSqrt => one(|x| 1.0 / x.sqrt()),
        M::Exp2 => one(libm::exp2f),
        M::Log2 => one(libm::log2f),
        M::Exp => one(libm::expf),
        M::Log => one(libm::logf),
        M::Saturate => one(|x| x.clamp(0.0, 1.0)),
        M::Min => all(&|i| arg(0, i).min(arg(1, i))),
        M::Max => all(&|i| arg(0, i).max(arg(1, i))),
        M::Clamp => all(&|i| arg(0, i).max(arg(1, i)).min(arg(2, i))),
        M::Fma => all(&|i| arg(0, i).mul_add(arg(1, i), arg(2, i))),
        M::Mix => all(&|i| (arg(1, i) - arg(0, i)) * arg(2, i) + arg(0, i)),
        M::Pow => all(&|i| libm::powf(arg(0, i), arg(1, i))),
        M::Dot => {
            let n = usize::from(a.ty.lanes);
            let d = (0..n).map(|i| arg(0, i) * arg(1, i)).sum::<f32>();
            Some(K::of(Scalar::F32, 1, |_| d.to_bits()))
        }
        M::Length => {
            let n = usize::from(a.ty.lanes);
            let d = (0..n).map(|i| arg(0, i) * arg(0, i)).sum::<f32>().sqrt();
            Some(K::of(Scalar::F32, 1, |_| d.to_bits()))
        }
        _ => None,
    }
}
