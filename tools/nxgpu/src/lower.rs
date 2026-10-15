// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! naga's IR of a fragment entry point lowered into ours: the interface first (location
//! inputs, the colour outputs, the immediates' size, the textures and samplers with their
//! bindings), then the body's expressions on demand in SSA order, then the return value written
//! through the epilog. Types come from naga's own resolution of every expression.

use std::collections::HashMap;

use naga::{
    AddressSpace, BinaryOperator, Binding, Expression, Handle, ImageDimension, Interpolation,
    Literal, MathFunction, SampleLevel, ScalarKind, ShaderStage, Statement, TypeInner,
    UnaryOperator, VectorSize,
};
use nexus_gfx::shader::ir::builder::{store_blended, Blend, Builder};
use nexus_gfx::shader::ir::{
    self, BinOp, CmpOp, Interp, Lod, Op, Scalar, Stage, TexDim, Type, UnOp, ValueId,
};

use super::FrontendError;

type Result<T> = core::result::Result<T, FrontendError>;

/// Parses, validates and lowers the fragment entry point `entry` of `src`; its colour outputs
/// are written under `blend`.
pub fn lower_fragment(src: &str, entry: &str, blend: Blend) -> Result<ir::Module> {
    let module = naga::front::wgsl::parse_str(src)
        .map_err(|e| FrontendError::Parse(e.emit_to_string(src)))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::IMMEDIATES,
    )
    .validate(&module)
    .map_err(|e| FrontendError::Validate(format!("{e:?}")))?;
    let (index, ep) = module
        .entry_points
        .iter()
        .enumerate()
        .find(|(_, ep)| ep.name == entry && ep.stage == ShaderStage::Fragment)
        .ok_or_else(|| FrontendError::NoEntryPoint(entry.into()))?;
    let mut l = Lowering {
        module: &module,
        fun: &ep.function,
        info: info.get_entry_point(index),
        b: Builder::new(Stage::Fragment),
        values: HashMap::new(),
        args: Vec::new(),
        globals: HashMap::new(),
    };
    l.interface()?;
    let outputs = l.outputs()?;
    for stmt in ep.function.body.iter() {
        match stmt {
            Statement::Emit(_) => {}
            Statement::Return { value: Some(v) } => {
                l.store_result(*v, &outputs, blend)?;
                return l.b.finish().map_err(FrontendError::Ir);
            }
            Statement::Return { value: None } => {
                return Err(FrontendError::Unsupported("a fragment program without a result"))
            }
            Statement::Block(_) => return Err(FrontendError::Unsupported("a nested block")),
            Statement::If { .. } | Statement::Switch { .. } | Statement::Loop { .. } => {
                return Err(FrontendError::Unsupported("control flow"))
            }
            Statement::Kill => return Err(FrontendError::Unsupported("discard")),
            Statement::Store { .. } => {
                return Err(FrontendError::Unsupported("a store to a variable"))
            }
            Statement::Call { .. } => return Err(FrontendError::Unsupported("a function call")),
            _ => return Err(FrontendError::Unsupported("a statement outside the subset")),
        }
    }
    Err(FrontendError::Unsupported("a body that does not return"))
}

/// What an expression lowers to.
#[derive(Clone, Copy)]
enum Lowered {
    Value(ValueId),
    /// A place in the immediates: its byte offset and naga type.
    Push(u32, Handle<naga::Type>),
    /// A texture or sampler: its resource index.
    Resource(u8),
    /// A struct-typed argument: the first of its members' input indices.
    Args(u8),
}

struct Lowering<'a> {
    module: &'a naga::Module,
    fun: &'a naga::Function,
    info: &'a naga::valid::FunctionInfo,
    b: Builder,
    values: HashMap<Handle<Expression>, Lowered>,
    /// Per function argument: its input index (or, for a struct, its first member's).
    args: Vec<Lowered>,
    /// Per global: what it lowers to.
    globals: HashMap<Handle<naga::GlobalVariable>, Lowered>,
}

fn scalar(s: naga::Scalar) -> Result<Scalar> {
    Ok(match (s.kind, s.width) {
        (ScalarKind::Float | ScalarKind::AbstractFloat, 4 | 8) => Scalar::F32,
        (ScalarKind::Float, 2) => Scalar::F16,
        (ScalarKind::Sint | ScalarKind::AbstractInt, 4 | 8) => Scalar::I32,
        (ScalarKind::Uint, 4) => Scalar::U32,
        (ScalarKind::Bool, _) => Scalar::Bool,
        _ => return Err(FrontendError::Unsupported("a scalar width outside the subset")),
    })
}

fn lanes(size: VectorSize) -> u8 {
    size as u8
}

fn ty_of(inner: &TypeInner) -> Result<Type> {
    match inner {
        TypeInner::Scalar(s) => Ok(Type::new(scalar(*s)?, 1)),
        TypeInner::Vector { size, scalar: s } => Ok(Type::new(scalar(*s)?, lanes(*size))),
        _ => Err(FrontendError::Unsupported("a value type outside scalars and vectors")),
    }
}

fn interp(i: Option<Interpolation>) -> Interp {
    match i {
        Some(Interpolation::Linear) => Interp::Linear,
        Some(Interpolation::Flat) => Interp::Flat,
        _ => Interp::Perspective,
    }
}

impl Lowering<'_> {
    fn inner(&self, ty: Handle<naga::Type>) -> &TypeInner {
        &self.module.types[ty].inner
    }

    /// The inputs, the immediates and the resources.
    fn interface(&mut self) -> Result<()> {
        for arg in &self.fun.arguments {
            match (&arg.binding, self.inner(arg.ty)) {
                (Some(Binding::Location { location, interpolation, .. }), inner) => {
                    let ty = ty_of(inner)?;
                    let i = self.b.input(*location as u8, ty, interp(*interpolation));
                    self.args.push(Lowered::Args(i));
                }
                (None, TypeInner::Struct { members, .. }) => {
                    let mut first = None;
                    for m in members.clone() {
                        let Some(Binding::Location { location, interpolation, .. }) = m.binding
                        else {
                            return Err(FrontendError::Unsupported("a built-in input"));
                        };
                        let ty = ty_of(self.inner(m.ty))?;
                        let i = self.b.input(location as u8, ty, interp(interpolation));
                        first.get_or_insert(i);
                    }
                    self.args.push(Lowered::Args(first.unwrap_or(0)));
                }
                _ => return Err(FrontendError::Unsupported("a built-in input")),
            }
        }
        for (h, g) in self.module.global_variables.iter() {
            let lowered = match (g.space, self.inner(g.ty)) {
                (AddressSpace::Immediate, TypeInner::Struct { span, .. }) => {
                    self.b.push_bytes(*span);
                    Lowered::Push(0, g.ty)
                }
                (
                    AddressSpace::Handle,
                    TypeInner::Image { dim: ImageDimension::D2, arrayed: false, .. },
                ) => {
                    let binding = g.binding.as_ref().map_or(0, |b| b.binding as u8);
                    Lowered::Resource(self.b.texture(binding, TexDim::D2))
                }
                (AddressSpace::Handle, TypeInner::Sampler { comparison: false }) => {
                    let binding = g.binding.as_ref().map_or(0, |b| b.binding as u8);
                    Lowered::Resource(self.b.sampler(binding))
                }
                _ => {
                    return Err(FrontendError::Unsupported(
                        "a global outside immediates, 2D textures and samplers",
                    ))
                }
            };
            self.globals.insert(h, lowered);
        }
        Ok(())
    }

    /// The colour outputs: one per location of the result.
    fn outputs(&mut self) -> Result<Vec<u8>> {
        let Some(result) = &self.fun.result else {
            return Err(FrontendError::Unsupported("a fragment program without a result"));
        };
        match (&result.binding, self.inner(result.ty)) {
            (Some(Binding::Location { location, .. }), inner) => {
                let ty = ty_of(inner)?;
                Ok(vec![self.b.output(*location as u8, ty)])
            }
            (None, TypeInner::Struct { members, .. }) => {
                let mut outs = Vec::new();
                for m in members.clone() {
                    let Some(Binding::Location { location, .. }) = m.binding else {
                        return Err(FrontendError::Unsupported("a built-in output"));
                    };
                    let ty = ty_of(self.inner(m.ty))?;
                    outs.push(self.b.output(location as u8, ty));
                }
                Ok(outs)
            }
            _ => Err(FrontendError::Unsupported("a built-in output")),
        }
    }

    /// The return value written: through the epilog for four-lane colours.
    fn store_result(&mut self, v: Handle<Expression>, outputs: &[u8], blend: Blend) -> Result<()> {
        let parts: Vec<Handle<Expression>> = match (&self.fun.expressions[v], outputs.len()) {
            (_, 1) => vec![v],
            (Expression::Compose { components, .. }, n) if components.len() == n => {
                components.clone()
            }
            _ => return Err(FrontendError::Unsupported("a struct result not built in place")),
        };
        for (out, part) in outputs.iter().zip(parts) {
            let value = self.value(part)?;
            if self.b.ty(value).lanes == 4 {
                store_blended(&mut self.b, *out, value, blend);
            } else {
                self.b.store_output(*out, value);
            }
        }
        Ok(())
    }

    /// The IR type naga resolved for an expression.
    fn ty(&self, h: Handle<Expression>) -> Result<Type> {
        ty_of(self.info[h].ty.inner_with(&self.module.types))
    }

    fn value(&mut self, h: Handle<Expression>) -> Result<ValueId> {
        match self.lower(h)? {
            Lowered::Value(v) => Ok(v),
            _ => Err(FrontendError::Unsupported("a place or handle used as a value")),
        }
    }

    /// `v` widened to `lanes` when it is a scalar (WGSL's scalar-vector arithmetic).
    fn widen(&mut self, v: ValueId, lanes: u8) -> ValueId {
        if self.b.ty(v).lanes == 1 && lanes > 1 {
            self.b.splat(v, lanes)
        } else {
            v
        }
    }

    fn constant(&mut self, lit: Literal) -> Result<ValueId> {
        let (ty, bits) = match lit {
            Literal::F32(x) => (Type::F32, x.to_bits()),
            Literal::F16(x) => (Type::new(Scalar::F16, 1), x.to_f32().to_bits()),
            Literal::AbstractFloat(x) => (Type::F32, (x as f32).to_bits()),
            Literal::I32(x) => (Type::new(Scalar::I32, 1), x as u32),
            Literal::AbstractInt(x) => (Type::new(Scalar::I32, 1), x as i32 as u32),
            Literal::U32(x) => (Type::new(Scalar::U32, 1), x),
            Literal::Bool(x) => (Type::new(Scalar::Bool, 1), u32::from(x)),
            _ => return Err(FrontendError::Unsupported("a 64-bit literal")),
        };
        Ok(self.b.emit(Op::Const([bits, 0, 0, 0]), ty))
    }

    /// A global expression (a constant's initializer): literals and constructors of them.
    fn global_const(&mut self, h: Handle<Expression>) -> Result<ValueId> {
        match &self.module.global_expressions[h] {
            Expression::Literal(lit) => self.constant(*lit),
            Expression::Compose { components, .. } => {
                let parts =
                    components.iter().map(|c| self.global_const(*c)).collect::<Result<Vec<_>>>()?;
                Ok(self.b.construct(&parts))
            }
            _ => Err(FrontendError::Unsupported("a constant outside literals and constructors")),
        }
    }

    fn lower(&mut self, h: Handle<Expression>) -> Result<Lowered> {
        if let Some(l) = self.values.get(&h) {
            return Ok(*l);
        }
        let l = match self.fun.expressions[h].clone() {
            Expression::Literal(lit) => Lowered::Value(self.constant(lit)?),
            Expression::Constant(c) => {
                let init = self.module.constants[c].init;
                Lowered::Value(self.global_const(init)?)
            }
            Expression::ZeroValue(_) => {
                let ty = self.ty(h)?;
                Lowered::Value(self.b.emit(Op::Const([0; 4]), ty))
            }
            Expression::FunctionArgument(i) => match self.args.get(i as usize).copied() {
                Some(Lowered::Args(input))
                    if !matches!(
                        self.inner(self.fun.arguments[i as usize].ty),
                        TypeInner::Struct { .. }
                    ) =>
                {
                    Lowered::Value(self.b.load_input(input))
                }
                Some(a) => a,
                None => return Err(FrontendError::Unsupported("an unknown argument")),
            },
            Expression::GlobalVariable(g) => {
                *self.globals.get(&g).ok_or(FrontendError::Unsupported("an unknown global"))?
            }
            Expression::Load { pointer } => match self.lower(pointer)? {
                Lowered::Push(offset, ty) => {
                    let t = ty_of(self.inner(ty))?;
                    Lowered::Value(self.b.load_push(offset, t))
                }
                Lowered::Resource(r) => Lowered::Resource(r),
                _ => return Err(FrontendError::Unsupported("a load from a variable")),
            },
            Expression::AccessIndex { base, index } => match self.lower(base)? {
                Lowered::Push(offset, ty) => match self.inner(ty) {
                    TypeInner::Struct { members, .. } => {
                        let m = members
                            .get(index as usize)
                            .ok_or(FrontendError::Unsupported("a member index"))?;
                        Lowered::Push(offset + m.offset, m.ty)
                    }
                    _ => return Err(FrontendError::Unsupported("an index into the immediates")),
                },
                Lowered::Args(first) => Lowered::Value(self.b.load_input(first + index as u8)),
                Lowered::Value(v) => Lowered::Value(self.b.extract(v, index as u8)),
                Lowered::Resource(_) => {
                    return Err(FrontendError::Unsupported("an index into a resource"))
                }
            },
            Expression::Splat { size, value } => {
                let v = self.value(value)?;
                Lowered::Value(self.b.splat(v, lanes(size)))
            }
            Expression::Swizzle { size, vector, pattern } => {
                let v = self.value(vector)?;
                let sel: Vec<u8> =
                    pattern[..lanes(size) as usize].iter().map(|c| *c as u8).collect();
                Lowered::Value(self.b.swizzle(v, &sel))
            }
            Expression::Compose { components, .. } => {
                let parts =
                    components.iter().map(|c| self.value(*c)).collect::<Result<Vec<_>>>()?;
                Lowered::Value(self.b.construct(&parts))
            }
            Expression::Unary { op: UnaryOperator::Negate, expr } => {
                let v = self.value(expr)?;
                Lowered::Value(self.b.unary(UnOp::Neg, v))
            }
            Expression::Binary { op, left, right } => {
                let lanes = self.ty(h)?.lanes;
                let (a, b) = (self.value(left)?, self.value(right)?);
                let (a, b) = (self.widen(a, lanes), self.widen(b, lanes));
                let bin = |op| Some(op);
                let arith = match op {
                    BinaryOperator::Add => bin(BinOp::Add),
                    BinaryOperator::Subtract => bin(BinOp::Sub),
                    BinaryOperator::Multiply => bin(BinOp::Mul),
                    BinaryOperator::Divide => bin(BinOp::Div),
                    _ => None,
                };
                if let Some(op) = arith {
                    Lowered::Value(self.b.binary(op, a, b))
                } else {
                    let cmp = match op {
                        BinaryOperator::Less => CmpOp::Lt,
                        BinaryOperator::LessEqual => CmpOp::Le,
                        BinaryOperator::Equal => CmpOp::Eq,
                        BinaryOperator::NotEqual => CmpOp::Ne,
                        BinaryOperator::GreaterEqual => CmpOp::Ge,
                        BinaryOperator::Greater => CmpOp::Gt,
                        _ => {
                            return Err(FrontendError::Unsupported(
                                "a binary operator outside the subset",
                            ))
                        }
                    };
                    let ty = Type::new(Scalar::Bool, self.b.ty(a).lanes);
                    Lowered::Value(self.b.emit(Op::Compare(cmp, a, b), ty))
                }
            }
            Expression::Select { condition, accept, reject } => {
                let (c, a, r) = (self.value(condition)?, self.value(accept)?, self.value(reject)?);
                let ty = self.b.ty(a);
                Lowered::Value(self.b.emit(Op::Select(c, a, r), ty))
            }
            Expression::As { expr, kind, convert: Some(width) } => {
                let v = self.value(expr)?;
                let lanes = self.b.ty(v).lanes;
                let to = scalar(naga::Scalar { kind, width })?;
                Lowered::Value(self.b.emit(Op::Convert(v), Type::new(to, lanes)))
            }
            Expression::As { convert: None, .. } => {
                return Err(FrontendError::Unsupported("a bitcast"))
            }
            Expression::Math { fun, arg, arg1, arg2, .. } => {
                Lowered::Value(self.math(h, fun, arg, arg1, arg2)?)
            }
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
                    return Err(FrontendError::Unsupported("a sample of something not a texture"));
                };
                let coords = self.value(coordinate)?;
                let lod = match level {
                    SampleLevel::Auto => Lod::Implicit,
                    SampleLevel::Zero => Lod::Level(self.b.f32s(&[0.0])),
                    SampleLevel::Exact(e) => Lod::Level(self.value(e)?),
                    SampleLevel::Bias(e) => Lod::Bias(self.value(e)?),
                    SampleLevel::Gradient { .. } => {
                        return Err(FrontendError::Unsupported("a sample with gradients"))
                    }
                };
                Lowered::Value(self.b.sample(t, s, coords, lod))
            }
            Expression::ImageSample { .. } => {
                return Err(FrontendError::Unsupported("a gather, array, offset or depth sample"))
            }
            _ => return Err(FrontendError::Unsupported("an expression outside the subset")),
        };
        self.values.insert(h, l);
        Ok(l)
    }

    fn math(
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
                let (lo, hi) = (operand(self, arg1)?, operand(self, arg2)?);
                let m = self.b.binary(BinOp::Min, a, hi);
                Ok(self.b.binary(BinOp::Max, m, lo))
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
