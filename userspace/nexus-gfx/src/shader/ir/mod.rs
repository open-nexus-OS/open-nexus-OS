// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The system's GPU program IR (ADR-0072, TASK-0215 5c-2 P2) — the center every
//! frontend lowers into (WGSL first, through a host build tool) and every backend compiles from
//! (the board GPU's, the virtual GPU's, and the interpreter here, the reference that proves a
//! program before it meets a GPU). A module is its interface — the stage, the inputs with their
//! interpolation, the outputs, the resources, the push-constant block's size — and a body of
//! instructions in SSA form: instruction `i` defines value `i`, operands name earlier values.
//! Values are scalars or vectors of up to four lanes. A fragment program writes its outputs
//! through the epilog's ops (`LoadDst` reads the tile's current value, which is how blending is
//! a program part on a GPU without a blend unit). Straight-line code is the subset the
//! compositor needs; structured control flow, buffers and subgroup and matrix ops extend `Op`
//! without breaking what exists. `no_std`: the same code can be the JIT library later.
//! OWNERS: @gpu @runtime
//! STATUS: Experimental (P2)
//! API_STABILITY: Unstable — versioned by [`VERSION`]
//! TEST_COVERAGE: `tests/shader_ir.rs` — the validator's rejects, the interpreter against the
//!   CPU rasterizer's formulas, the epilogs.
//!
//! - [`builder`] — building a module; the epilogs (replace, straight and premultiplied over).
//! - [`validate`] — the rules a module must meet before any backend sees it.
//! - [`interp`] — the reference interpreter: one invocation at a time.

pub mod builder;
pub mod interp;
pub mod validate;

use alloc::vec::Vec;

/// The IR's version; a module of another version is refused.
pub const VERSION: u32 = 1;
/// The most instructions a module may hold (a bound for every pass and for the JIT).
pub const MAX_INSTRS: usize = 4096;
/// Interface bounds.
pub const MAX_INPUTS: usize = 16;
pub const MAX_OUTPUTS: usize = 8;
pub const MAX_RESOURCES: usize = 16;
pub const MAX_PUSH_BYTES: u32 = 256;
/// A vertex program's clip-space position (WGSL's `@builtin(position)`): the output at this
/// location, four floats; no other stage writes it.
pub const POSITION: u8 = u8::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Vertex,
    Fragment,
    Compute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scalar {
    F32,
    F16,
    I32,
    U32,
    Bool,
}

impl Scalar {
    pub const fn is_float(self) -> bool {
        matches!(self, Self::F32 | Self::F16)
    }
}

/// A value's type: a scalar kind and one to four lanes (zero for an instruction without a
/// result).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Type {
    pub scalar: Scalar,
    pub lanes: u8,
}

impl Type {
    pub const fn new(scalar: Scalar, lanes: u8) -> Self {
        Self { scalar, lanes }
    }
    pub const NONE: Self = Self::new(Scalar::Bool, 0);
    pub const F32: Self = Self::new(Scalar::F32, 1);
    pub const VEC2: Self = Self::new(Scalar::F32, 2);
    pub const VEC3: Self = Self::new(Scalar::F32, 3);
    pub const VEC4: Self = Self::new(Scalar::F32, 4);
    pub const fn scalar_of(self) -> Self {
        Self::new(self.scalar, 1)
    }
}

/// A value: the result of the instruction at this index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValueId(pub u32);

/// How an input varies across a primitive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interp {
    Perspective,
    Linear,
    Flat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    pub location: u8,
    pub ty: Type,
    pub interp: Interp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Output {
    pub location: u8,
    pub ty: Type,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TexDim {
    D1,
    D2,
    D3,
}

impl TexDim {
    pub const fn coords(self) -> u8 {
        match self {
            Self::D1 => 1,
            Self::D2 => 2,
            Self::D3 => 3,
        }
    }
}

/// A resource the program binds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resource {
    Texture { binding: u8, dim: TexDim },
    Sampler { binding: u8 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interface {
    pub stage: Stage,
    pub inputs: Vec<Input>,
    pub outputs: Vec<Output>,
    pub resources: Vec<Resource>,
    /// The push-constant block's size in bytes (a multiple of four).
    pub push_bytes: u32,
}

/// A sample's level of detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lod {
    /// From the screen-space derivatives (fragment programs only).
    Implicit,
    Level(ValueId),
    Bias(ValueId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Abs,
    Floor,
    Fract,
    Rcp,
    Rsqrt,
    Sqrt,
    Exp2,
    Log2,
    /// Clamp to 0..1.
    Sat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpOp {
    Lt,
    Le,
    Eq,
    Ne,
    Ge,
    Gt,
}

/// An operation; the instruction's type is its result's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// The lanes' bits, read as the instruction's type.
    Const([u32; 4]),
    /// An input, by its index in the interface.
    LoadInput(u8),
    /// From the push-constant block, at a byte offset.
    LoadPush(u32),
    /// The tile's current value of an output (fragment epilogs).
    LoadDst(u8),
    Unary(UnOp, ValueId),
    Binary(BinOp, ValueId, ValueId),
    /// `a * b + c`.
    Fma(ValueId, ValueId, ValueId),
    Compare(CmpOp, ValueId, ValueId),
    /// `cond ? a : b`, lane-wise (a one-lane condition applies to every lane).
    Select(ValueId, ValueId, ValueId),
    /// Into the instruction's scalar kind.
    Convert(ValueId),
    /// A vector from scalars and vectors, in order.
    Construct(Vec<ValueId>),
    Extract(ValueId, u8),
    /// Lanes of a value; the first `ty.lanes` entries are used.
    Swizzle(ValueId, [u8; 4]),
    /// One lane repeated.
    Splat(ValueId),
    /// Sample a texture (resource indices), returning four lanes.
    Sample {
        texture: u8,
        sampler: u8,
        coords: ValueId,
        lod: Lod,
    },
    /// Write an output (no result).
    StoreOutput(u8, ValueId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instr {
    pub op: Op,
    pub ty: Type,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    pub version: u32,
    pub interface: Interface,
    pub body: Vec<Instr>,
}

impl Module {
    /// The type of a value; `None` for an unknown one.
    pub fn ty(&self, v: ValueId) -> Option<Type> {
        self.body.get(v.0 as usize).map(|i| i.ty)
    }
}

/// Why a module is refused or cannot run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IrError {
    Version(u32),
    TooLarge {
        what: &'static str,
    },
    /// Instruction `at` breaks a rule.
    Invalid {
        at: usize,
        what: &'static str,
    },
    /// The interface breaks a rule.
    Interface {
        what: &'static str,
    },
    /// The interpreter lacks what the invocation must provide.
    Missing {
        what: &'static str,
    },
}
