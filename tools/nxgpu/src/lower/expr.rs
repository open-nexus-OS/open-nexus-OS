// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! An expression lowered on demand, once per evaluation (`flow` forgets a range's results where
//! naga emits it again): what the pipeline fixes folds into a constant (`fold`), the rest becomes
//! instructions with any constant operand made one.

use naga::{BinaryOperator, Expression, Handle, SampleLevel, TypeInner, UnaryOperator};
use nexus_gfx::shader::ir::{BinOp, CmpOp, Lod, Op, Scalar, Type, UnOp};

use super::fold::{self, K};
use super::{lanes, scalar, ty_of, FrontendError, Lowered, Lowering, Result};

/// The constant a lowered expression is, if the pipeline fixes it.
pub(super) fn fixed(l: Lowered) -> Option<K> {
    match l {
        Lowered::Const(k) => Some(k),
        _ => None,
    }
}

/// Whether the pipeline fixes every lane of a float to `x`.
pub(super) fn is_f32(l: Lowered, x: f32) -> bool {
    fixed(l).is_some_and(|k| k.ty.scalar == Scalar::F32 && k.uniform() && k.lane_f32(0) == x)
}

impl Lowering<'_> {
    /// A global expression (a constant's initializer): literals, constructors and splats of
    /// them, zero values.
    fn global_k(&self, h: Handle<Expression>) -> Result<K> {
        match &self.module.global_expressions[h] {
            Expression::Literal(lit) => {
                K::of_literal(*lit).ok_or(FrontendError::Unsupported("a 64-bit literal"))
            }
            Expression::Compose { components, .. } => {
                let parts =
                    components.iter().map(|c| self.global_k(*c)).collect::<Result<Vec<_>>>()?;
                K::compose(&parts)
                    .ok_or(FrontendError::Unsupported("a constant outside scalars and vectors"))
            }
            Expression::Splat { size, value } => Ok(self.global_k(*value)?.splat(lanes(*size))),
            Expression::ZeroValue(ty) => Ok(K::zero(ty_of(self.inner(*ty))?)),
            _ => Err(FrontendError::Unsupported("a constant outside literals and constructors")),
        }
    }

    /// Component `index` of what `base` lowered to.
    fn access(&mut self, base: Lowered, index: u32) -> Result<Lowered> {
        let past = FrontendError::Unsupported("an index past the vector");
        let lane = |n: u8| u8::try_from(index).ok().filter(|i| *i < n).ok_or(past.clone());
        Ok(match base {
            Lowered::Push(offset, ty) => match self.inner(ty) {
                TypeInner::Struct { members, .. } => {
                    let m = members
                        .get(index as usize)
                        .ok_or(FrontendError::Unsupported("a member index"))?;
                    Lowered::Push(offset + m.offset, m.ty)
                }
                TypeInner::Vector { size, .. } => {
                    Lowered::PushLane(offset, ty, lane(lanes(*size))?)
                }
                _ => return Err(FrontendError::Unsupported("an index into the immediates")),
            },
            Lowered::Args(first) => {
                let member = u8::try_from(index)
                    .ok()
                    .and_then(|i| first.checked_add(i))
                    .ok_or(FrontendError::Unsupported("a member index"))?;
                Lowered::Value(self.input(member))
            }
            Lowered::Value(v) => {
                let i = lane(self.b.ty(v).lanes)?;
                Lowered::Value(self.b.extract(v, i))
            }
            Lowered::Const(k) => Lowered::Const(k.extract(lane(k.ty.lanes)?).ok_or(past)?),
            Lowered::Local(var) => match self.inner(self.fun.local_variables[var].ty) {
                TypeInner::Vector { size, .. } => Lowered::LocalLane(var, lane(lanes(*size))?),
                _ => return Err(FrontendError::Unsupported("an index into a scalar variable")),
            },
            Lowered::Resource(_) => {
                return Err(FrontendError::Unsupported("an index into a resource"))
            }
            Lowered::PushLane(..) | Lowered::LocalLane(..) => {
                return Err(FrontendError::Unsupported("an index into a scalar"))
            }
        })
    }

    /// A function variable's value at this point of the walk.
    pub(super) fn local(&self, var: Handle<naga::LocalVariable>) -> Result<Lowered> {
        self.locals.get(&var).copied().ok_or(FrontendError::Unsupported("an unknown variable"))
    }

    /// A binary operation: folded when the pipeline fixes both operands.
    fn binary(
        &mut self,
        h: Handle<Expression>,
        op: BinaryOperator,
        la: Lowered,
        lb: Lowered,
    ) -> Result<Lowered> {
        if let Some(k) = fixed(la).zip(fixed(lb)).and_then(|(a, b)| fold::binary(op, a, b)) {
            return Ok(Lowered::Const(k));
        }
        let lanes = self.ty(h)?.lanes;
        // The identities: x + 0, 0 + x and x − 0 are x (WGSL lets an implementation ignore a
        // zero's sign), x · 1 and 1 · x are x exactly — an unrolled sum's first term costs nothing.
        let same = match op {
            BinaryOperator::Add | BinaryOperator::Subtract if is_f32(lb, 0.0) => Some(la),
            BinaryOperator::Add if is_f32(la, 0.0) => Some(lb),
            BinaryOperator::Multiply if is_f32(lb, 1.0) => Some(la),
            BinaryOperator::Multiply if is_f32(la, 1.0) => Some(lb),
            _ => None,
        };
        if let Some(l) = same {
            let v = self.as_value(l)?;
            return Ok(Lowered::Value(self.widen(v, lanes)));
        }
        let (a, b) = (self.as_value(la)?, self.as_value(lb)?);
        let (a, b) = (self.widen(a, lanes), self.widen(b, lanes));
        let arith = match op {
            BinaryOperator::Add => Some(BinOp::Add),
            BinaryOperator::Subtract => Some(BinOp::Sub),
            BinaryOperator::Multiply => Some(BinOp::Mul),
            BinaryOperator::Divide => Some(BinOp::Div),
            _ => None,
        };
        if let Some(op) = arith {
            return Ok(Lowered::Value(self.b.binary(op, a, b)));
        }
        let cmp = match op {
            BinaryOperator::Less => CmpOp::Lt,
            BinaryOperator::LessEqual => CmpOp::Le,
            BinaryOperator::Equal => CmpOp::Eq,
            BinaryOperator::NotEqual => CmpOp::Ne,
            BinaryOperator::GreaterEqual => CmpOp::Ge,
            BinaryOperator::Greater => CmpOp::Gt,
            _ => return Err(FrontendError::Unsupported("a binary operator outside the subset")),
        };
        let ty = Type::new(Scalar::Bool, self.b.ty(a).lanes);
        Ok(Lowered::Value(self.b.emit(Op::Compare(cmp, a, b), ty)))
    }

    pub(super) fn lower(&mut self, h: Handle<Expression>) -> Result<Lowered> {
        if let Some(l) = self.values.get(&h) {
            return Ok(*l);
        }
        let unsupported = |what| Err(FrontendError::Unsupported(what));
        let l = match self.fun.expressions[h].clone() {
            Expression::Literal(lit) => Lowered::Const(
                K::of_literal(lit).ok_or(FrontendError::Unsupported("a 64-bit literal"))?,
            ),
            Expression::Constant(c) => {
                let constant = &self.module.constants[c];
                // A scalar or vector: an array or struct constant is refused, not flattened.
                ty_of(self.inner(constant.ty))?;
                Lowered::Const(self.global_k(constant.init)?)
            }
            Expression::Override(o) => match self.fixed.get(&o) {
                Some(lit) => Lowered::Const(
                    K::of_literal(*lit).ok_or(FrontendError::Unsupported("a 64-bit override"))?,
                ),
                None => {
                    return Err(FrontendError::Override("an override outside the module".into()))
                }
            },
            Expression::ZeroValue(_) => Lowered::Const(K::zero(self.ty(h)?)),
            Expression::FunctionArgument(i) => match self.args.get(i as usize).copied() {
                Some(Lowered::Args(input))
                    if !matches!(
                        self.inner(self.fun.arguments[i as usize].ty),
                        TypeInner::Struct { .. }
                    ) =>
                {
                    Lowered::Value(self.input(input))
                }
                Some(a) => a,
                None => return unsupported("an unknown argument"),
            },
            Expression::GlobalVariable(g) => {
                *self.globals.get(&g).ok_or(FrontendError::Unsupported("an unknown global"))?
            }
            Expression::LocalVariable(var) => Lowered::Local(var),
            Expression::Load { pointer } => match self.lower(pointer)? {
                Lowered::Push(offset, ty) => {
                    let t = ty_of(self.inner(ty))?;
                    Lowered::Value(self.b.load_push(offset, t))
                }
                Lowered::PushLane(offset, ty, lane) => {
                    let t = ty_of(self.inner(ty))?;
                    let v = self.b.load_push(offset, t);
                    Lowered::Value(self.b.extract(v, lane))
                }
                Lowered::Resource(r) => Lowered::Resource(r),
                Lowered::Local(var) => self.local(var)?,
                Lowered::LocalLane(var, lane) => {
                    let whole = self.local(var)?;
                    self.access(whole, u32::from(lane))?
                }
                _ => return unsupported("a load from something not a variable"),
            },
            Expression::AccessIndex { base, index } => {
                let base = self.lower(base)?;
                self.access(base, index)?
            }
            Expression::Access { base, index } => {
                let base = self.lower(base)?;
                match self.lower(index)? {
                    Lowered::Const(k)
                        if k.ty.lanes == 1 && matches!(k.ty.scalar, Scalar::I32 | Scalar::U32) =>
                    {
                        self.access(base, k.bits[0])?
                    }
                    _ => return unsupported("an index the pipeline does not fix"),
                }
            }
            Expression::Splat { size, value } => match self.lower(value)? {
                Lowered::Const(k) => Lowered::Const(k.splat(lanes(size))),
                l => {
                    let v = self.as_value(l)?;
                    Lowered::Value(self.b.splat(v, lanes(size)))
                }
            },
            Expression::Swizzle { size, vector, pattern } => {
                let sel: Vec<u8> =
                    pattern[..lanes(size) as usize].iter().map(|c| *c as u8).collect();
                match self.lower(vector)? {
                    Lowered::Const(k) => Lowered::Const(
                        k.swizzle(&sel)
                            .ok_or(FrontendError::Unsupported("a swizzle past the vector"))?,
                    ),
                    l => {
                        let v = self.as_value(l)?;
                        Lowered::Value(self.b.swizzle(v, &sel))
                    }
                }
            }
            Expression::Compose { components, .. } => {
                // A vector: an array, struct or matrix built here is refused, not flattened.
                self.ty(h)?;
                let parts =
                    components.iter().map(|c| self.lower(*c)).collect::<Result<Vec<_>>>()?;
                let all: Option<Vec<K>> = parts.iter().map(|p| fixed(*p)).collect();
                match all.as_deref().and_then(K::compose) {
                    Some(k) => Lowered::Const(k),
                    None => {
                        let values = parts
                            .into_iter()
                            .map(|p| self.as_value(p))
                            .collect::<Result<Vec<_>>>()?;
                        Lowered::Value(self.b.construct(&values))
                    }
                }
            }
            Expression::Unary { op, expr } => {
                let l = self.lower(expr)?;
                match (fixed(l).and_then(|k| fold::unary(op, k)), op) {
                    (Some(k), _) => Lowered::Const(k),
                    (None, UnaryOperator::Negate) => {
                        let v = self.as_value(l)?;
                        Lowered::Value(self.b.unary(UnOp::Neg, v))
                    }
                    (None, _) => return unsupported("a unary operator outside the subset"),
                }
            }
            Expression::Binary { op, left, right } => {
                let (la, lb) = (self.lower(left)?, self.lower(right)?);
                self.binary(h, op, la, lb)?
            }
            Expression::Select { condition, accept, reject } => match self.lower(condition)? {
                // Fixed by the pipeline: the operand picked, the other never lowered.
                Lowered::Const(c) if c.ty.lanes == 1 && c.ty.scalar == Scalar::Bool => {
                    self.lower(if c.bits[0] != 0 { accept } else { reject })?
                }
                lc => {
                    let (la, lr) = (self.lower(accept)?, self.lower(reject)?);
                    let folded = match (fixed(lc), fixed(la), fixed(lr)) {
                        (Some(c), Some(a), Some(r)) => fold::select(c, a, r),
                        _ => None,
                    };
                    match folded {
                        Some(k) => Lowered::Const(k),
                        None => {
                            let (c, a, r) =
                                (self.as_value(lc)?, self.as_value(la)?, self.as_value(lr)?);
                            let ty = self.b.ty(a);
                            Lowered::Value(self.b.emit(Op::Select(c, a, r), ty))
                        }
                    }
                }
            },
            Expression::As { expr, kind, convert } => {
                let to = scalar(naga::Scalar { kind, width: convert.unwrap_or(4) })?;
                let l = self.lower(expr)?;
                let folded = match convert {
                    Some(_) => fixed(l).and_then(|k| fold::convert(k, to)),
                    None => fixed(l).and_then(|k| fold::bitcast(k, to)),
                };
                match (folded, convert) {
                    (Some(k), _) => Lowered::Const(k),
                    (None, Some(_)) => {
                        let v = self.as_value(l)?;
                        let lanes = self.b.ty(v).lanes;
                        Lowered::Value(self.b.emit(Op::Convert(v), Type::new(to, lanes)))
                    }
                    (None, None) => return unsupported("a bitcast"),
                }
            }
            Expression::Math { fun, arg, arg1, arg2, .. } => self.math(h, fun, arg, arg1, arg2)?,
            Expression::ImageSample {
                image,
                sampler,
                coordinate,
                level,
                gather: None,
                array_index: None,
                offset: None,
                depth_ref: None,
                ..
            } => {
                let (Lowered::Resource(t), Lowered::Resource(s)) =
                    (self.lower(image)?, self.lower(sampler)?)
                else {
                    return unsupported("a sample of something not a texture");
                };
                let coords = self.value(coordinate)?;
                let lod = match level {
                    SampleLevel::Auto => Lod::Implicit,
                    SampleLevel::Zero => Lod::Level(self.materialize(K::zero(Type::F32))),
                    SampleLevel::Exact(e) => Lod::Level(self.value(e)?),
                    SampleLevel::Bias(e) => Lod::Bias(self.value(e)?),
                    SampleLevel::Gradient { .. } => return unsupported("a sample with gradients"),
                };
                Lowered::Value(self.b.sample(t, s, coords, lod))
            }
            Expression::ImageSample { .. } => {
                return unsupported("a gather, array, offset or depth sample")
            }
            _ => return unsupported("an expression outside the subset"),
        };
        self.values.insert(h, l);
        Ok(l)
    }
}
