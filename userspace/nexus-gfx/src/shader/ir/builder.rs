// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Building a module instruction by instruction — what a frontend's lowering and the pipeline
//! library's own parts (the epilogs) write through — and the epilogs: a fragment program's
//! colour written to its output under a blend, as program code (the GPU has no blend unit):
//! replace; straight-alpha over with the source's alpha kept (the factors of the captured
//! pipeline, so measured programs and ours compare); premultiplied over (the compositor's).

use alloc::vec::Vec;

use super::validate::validate;
use super::{BinOp, Input, Instr, Interface, Interp, IrError, Lod, Module, Op, Output};
use super::{Resource, Scalar, Stage, TexDim, Type, UnOp, ValueId, VERSION};

/// A module under construction.
pub struct Builder {
    interface: Interface,
    body: Vec<Instr>,
}

impl Builder {
    pub fn new(stage: Stage) -> Self {
        Self {
            interface: Interface {
                stage,
                inputs: Vec::new(),
                outputs: Vec::new(),
                resources: Vec::new(),
                push_bytes: 0,
            },
            body: Vec::new(),
        }
    }

    /// Declares an input; its index.
    pub fn input(&mut self, location: u8, ty: Type, interp: Interp) -> u8 {
        self.interface.inputs.push(Input { location, ty, interp });
        (self.interface.inputs.len() - 1) as u8
    }

    /// Declares an output; its index.
    pub fn output(&mut self, location: u8, ty: Type) -> u8 {
        self.interface.outputs.push(Output { location, ty });
        (self.interface.outputs.len() - 1) as u8
    }

    /// Declares a texture; its resource index.
    pub fn texture(&mut self, binding: u8, dim: TexDim) -> u8 {
        self.interface.resources.push(Resource::Texture { binding, dim });
        (self.interface.resources.len() - 1) as u8
    }

    /// Declares a sampler; its resource index.
    pub fn sampler(&mut self, binding: u8) -> u8 {
        self.interface.resources.push(Resource::Sampler { binding });
        (self.interface.resources.len() - 1) as u8
    }

    pub fn push_bytes(&mut self, bytes: u32) {
        self.interface.push_bytes = bytes;
    }

    /// A value's type (`NONE` for an unknown value).
    pub fn ty(&self, v: ValueId) -> Type {
        self.body.get(v.0 as usize).map_or(Type::NONE, |i| i.ty)
    }

    pub fn emit(&mut self, op: Op, ty: Type) -> ValueId {
        self.body.push(Instr { op, ty });
        ValueId((self.body.len() - 1) as u32)
    }

    /// A float constant of one to four lanes.
    pub fn f32s(&mut self, v: &[f32]) -> ValueId {
        let mut bits = [0u32; 4];
        for (b, x) in bits.iter_mut().zip(v) {
            *b = x.to_bits();
        }
        self.emit(Op::Const(bits), Type::new(Scalar::F32, v.len().min(4) as u8))
    }

    pub fn load_input(&mut self, input: u8) -> ValueId {
        let ty = self.interface.inputs.get(usize::from(input)).map_or(Type::NONE, |i| i.ty);
        self.emit(Op::LoadInput(input), ty)
    }

    pub fn load_push(&mut self, offset: u32, ty: Type) -> ValueId {
        self.emit(Op::LoadPush(offset), ty)
    }

    pub fn load_dst(&mut self, output: u8) -> ValueId {
        let ty = self.interface.outputs.get(usize::from(output)).map_or(Type::NONE, |o| o.ty);
        self.emit(Op::LoadDst(output), ty)
    }

    pub fn unary(&mut self, op: UnOp, a: ValueId) -> ValueId {
        let ty = self.ty(a);
        self.emit(Op::Unary(op, a), ty)
    }

    pub fn binary(&mut self, op: BinOp, a: ValueId, b: ValueId) -> ValueId {
        let ty = self.ty(a);
        self.emit(Op::Binary(op, a, b), ty)
    }

    pub fn fma(&mut self, a: ValueId, b: ValueId, c: ValueId) -> ValueId {
        let ty = self.ty(a);
        self.emit(Op::Fma(a, b, c), ty)
    }

    pub fn extract(&mut self, a: ValueId, lane: u8) -> ValueId {
        let ty = self.ty(a).scalar_of();
        self.emit(Op::Extract(a, lane), ty)
    }

    pub fn swizzle(&mut self, a: ValueId, lanes: &[u8]) -> ValueId {
        let mut l = [0u8; 4];
        for (d, s) in l.iter_mut().zip(lanes) {
            *d = *s;
        }
        let ty = Type::new(self.ty(a).scalar, lanes.len().min(4) as u8);
        self.emit(Op::Swizzle(a, l), ty)
    }

    pub fn splat(&mut self, a: ValueId, lanes: u8) -> ValueId {
        let ty = Type::new(self.ty(a).scalar, lanes);
        self.emit(Op::Splat(a), ty)
    }

    pub fn construct(&mut self, parts: &[ValueId]) -> ValueId {
        let scalar = parts.first().map_or(Scalar::F32, |p| self.ty(*p).scalar);
        let lanes = parts.iter().map(|p| self.ty(*p).lanes).sum();
        self.emit(Op::Construct(parts.to_vec()), Type::new(scalar, lanes))
    }

    pub fn sample(&mut self, texture: u8, sampler: u8, coords: ValueId, lod: Lod) -> ValueId {
        self.emit(Op::Sample { texture, sampler, coords, lod }, Type::VEC4)
    }

    pub fn store_output(&mut self, output: u8, v: ValueId) {
        self.emit(Op::StoreOutput(output, v), Type::NONE);
    }

    /// The module, validated.
    pub fn finish(self) -> Result<Module, IrError> {
        let m = Module { version: VERSION, interface: self.interface, body: self.body };
        validate(&m)?;
        Ok(m)
    }
}

/// How a fragment program's colour meets the tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Replace,
    /// `rgb = src.rgb·a + dst.rgb·(1 − a)`, alpha the source's.
    OverStraight,
    /// `out = src + dst·(1 − src.a)` on every channel.
    OverPremultiplied,
}

/// The epilog: `color` (four lanes) written to `output` under `blend`.
pub fn store_blended(b: &mut Builder, output: u8, color: ValueId, blend: Blend) {
    match blend {
        Blend::Replace => b.store_output(output, color),
        Blend::OverStraight => {
            let dst = b.load_dst(output);
            let a = b.extract(color, 3);
            let one = b.f32s(&[1.0]);
            let inv = b.binary(BinOp::Sub, one, a);
            let (a3, inv3) = (b.splat(a, 3), b.splat(inv, 3));
            let (src_rgb, dst_rgb) = (b.swizzle(color, &[0, 1, 2]), b.swizzle(dst, &[0, 1, 2]));
            let kept = b.binary(BinOp::Mul, dst_rgb, inv3);
            let rgb = b.fma(src_rgb, a3, kept);
            let out = b.construct(&[rgb, a]);
            b.store_output(output, out);
        }
        Blend::OverPremultiplied => {
            let dst = b.load_dst(output);
            let a = b.extract(color, 3);
            let one = b.f32s(&[1.0]);
            let inv = b.binary(BinOp::Sub, one, a);
            let inv4 = b.splat(inv, 4);
            let out = b.fma(dst, inv4, color);
            b.store_output(output, out);
        }
    }
}
