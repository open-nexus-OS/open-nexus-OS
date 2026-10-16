// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! naga's IR of a fragment entry point lowered into ours: the interface first (location
//! inputs, the colour outputs, the immediates' size, the textures and samplers with their
//! bindings), then the body's statements in order with their expressions on demand in SSA order
//! (`flow`, `expr`), the return value written through the epilog. Types come from naga's own
//! resolution of every expression.
//!
//! A pipeline fixes the module's overrides (WGSL's pipeline-overridable constants) when it is
//! built — the library's variants of one source (ADR-0072 §5). What the pipeline fixes is
//! evaluated here (`fold`): an expression of literals, constants, overrides and the function
//! variables holding them is a constant, not an instruction, and becomes one only where it meets
//! a value the program computes. A `select` or `if` on a fixed condition takes its operand or
//! branch and never lowers the other, so a variant carries no instruction of the paths it
//! switched off; a loop whose condition is fixed runs here, its body lowered once an iteration.

use std::collections::HashMap;

use naga::{AddressSpace, Binding, Expression, Handle, ImageDimension, Interpolation};
use naga::{Literal, ScalarKind, ShaderStage, TypeInner, VectorSize};
use nexus_gfx::shader::ir::builder::{store_blended, Blend, Builder};
use nexus_gfx::shader::ir::{self, Interp, Op, Scalar, Stage, TexDim, Type, ValueId};

use super::FrontendError;
use flow::Flow;
use fold::K;

mod expr;
mod flow;
mod fold;
mod math;

type Result<T> = core::result::Result<T, FrontendError>;

/// Parses, validates and lowers the fragment entry point `entry` of `src` with its overrides
/// fixed by `overrides` — keyed as a pipeline's constants are (an override's `@id` in decimal,
/// else its name), a bool true for a non-zero value — or by their defaults; its colour outputs
/// are written under `blend`.
pub fn lower_fragment(
    src: &str,
    entry: &str,
    overrides: &[(&str, f64)],
    blend: Blend,
) -> Result<ir::Module> {
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
    let fixed = super::overrides::fix(&module, overrides)?;
    let mut l = Lowering {
        module: &module,
        fun: &ep.function,
        info: info.get_entry_point(index),
        b: Builder::new(Stage::Fragment),
        values: HashMap::new(),
        args: Vec::new(),
        globals: HashMap::new(),
        fixed,
        locals: HashMap::new(),
        consts: Vec::new(),
        inputs: HashMap::new(),
        outputs: Vec::new(),
        blend,
    };
    l.interface()?;
    l.outputs = l.outputs()?;
    l.init_locals()?;
    match l.block(&ep.function.body)? {
        Flow::Return => l.b.finish().map_err(FrontendError::Ir),
        _ => Err(FrontendError::Unsupported("a body that does not return")),
    }
}

/// What an expression lowers to.
#[derive(Clone, Copy)]
enum Lowered {
    Value(ValueId),
    /// A value the pipeline fixes: no instruction until it meets a computed one.
    Const(K),
    /// A place in the immediates: its byte offset and naga type.
    Push(u32, Handle<naga::Type>),
    /// One component of a vector in the immediates: the vector's offset and type, the lane.
    PushLane(u32, Handle<naga::Type>, u8),
    /// A texture or sampler: its resource index.
    Resource(u8),
    /// A struct-typed argument: the first of its members' input indices.
    Args(u8),
    /// A function variable (the place, not its value).
    Local(Handle<naga::LocalVariable>),
    /// One lane of a vector function variable.
    LocalLane(Handle<naga::LocalVariable>, u8),
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
    /// Per override: its value in this pipeline.
    fixed: HashMap<Handle<naga::Override>, Literal>,
    /// Per function variable: what was last stored to it.
    locals: HashMap<Handle<naga::LocalVariable>, Lowered>,
    /// The constants made instructions so far, each once.
    consts: Vec<(K, ValueId)>,
    /// The inputs loaded so far, each once (a load is iterated where the arithmetic reads it).
    inputs: HashMap<u8, ValueId>,
    /// The colour outputs, by result member.
    outputs: Vec<u8>,
    blend: Blend,
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
    fn store_result(&mut self, v: Handle<Expression>) -> Result<()> {
        let parts: Vec<Handle<Expression>> = match (&self.fun.expressions[v], self.outputs.len()) {
            (_, 1) => vec![v],
            (Expression::Compose { components, .. }, n) if components.len() == n => {
                components.clone()
            }
            _ => return Err(FrontendError::Unsupported("a struct result not built in place")),
        };
        for (out, part) in self.outputs.clone().into_iter().zip(parts) {
            let value = self.value(part)?;
            if self.b.ty(value).lanes == 4 {
                store_blended(&mut self.b, out, value, self.blend);
            } else {
                self.b.store_output(out, value);
            }
        }
        Ok(())
    }

    /// The IR type naga resolved for an expression.
    fn ty(&self, h: Handle<Expression>) -> Result<Type> {
        ty_of(self.info[h].ty.inner_with(&self.module.types))
    }

    fn value(&mut self, h: Handle<Expression>) -> Result<ValueId> {
        let l = self.lower(h)?;
        self.as_value(l)
    }

    /// A lowered value as an instruction's result: a constant made one here.
    fn as_value(&mut self, l: Lowered) -> Result<ValueId> {
        match l {
            Lowered::Value(v) => Ok(v),
            Lowered::Const(k) => Ok(self.materialize(k)),
            _ => Err(FrontendError::Unsupported("a place or handle used as a value")),
        }
    }

    /// A constant as an instruction, once a program: a scalar one; a vector of one value that
    /// scalar splat (the backend's free broadcast); any other vector built of its lanes.
    fn materialize(&mut self, k: K) -> ValueId {
        if let Some((_, v)) = self.consts.iter().find(|(c, _)| *c == k) {
            return *v;
        }
        let v = if k.ty.lanes == 1 {
            self.b.emit(Op::Const(k.bits), k.ty)
        } else if k.uniform() {
            let s =
                self.materialize(K { ty: Type::new(k.ty.scalar, 1), bits: [k.bits[0], 0, 0, 0] });
            self.b.splat(s, k.ty.lanes)
        } else {
            let mut parts = Vec::with_capacity(usize::from(k.ty.lanes));
            for i in 0..usize::from(k.ty.lanes) {
                let lane = K { ty: Type::new(k.ty.scalar, 1), bits: [k.bits[i], 0, 0, 0] };
                parts.push(self.materialize(lane));
            }
            self.b.construct(&parts)
        };
        self.consts.push((k, v));
        v
    }

    /// Input `i`'s value: one load a program, however often the body reads it.
    fn input(&mut self, i: u8) -> ValueId {
        let b = &mut self.b;
        *self.inputs.entry(i).or_insert_with(|| b.load_input(i))
    }

    /// `v` widened to `lanes` when it is a scalar (WGSL's scalar-vector arithmetic).
    fn widen(&mut self, v: ValueId, lanes: u8) -> ValueId {
        if self.b.ty(v).lanes == 1 && lanes > 1 {
            self.b.splat(v, lanes)
        } else {
            v
        }
    }
}
