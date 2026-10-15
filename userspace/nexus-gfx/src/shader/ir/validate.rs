// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The rules a module meets before any backend or the interpreter sees it: its version, its
//! bounds, an interface of distinct locations and bindings, and per instruction that every
//! operand is an earlier value of the type the operation needs, that loads stay inside the
//! interface, and that a fragment program writes each output at most once. A frontend's output
//! and a client's library both pass through here; nothing downstream re-checks.

use super::{IrError, Lod, Module, Op, Resource, Scalar, Stage, Type, UnOp, ValueId};
use super::{MAX_INPUTS, MAX_INSTRS, MAX_OUTPUTS, MAX_PUSH_BYTES, MAX_RESOURCES, VERSION};

fn valid_type(t: Type) -> bool {
    (1..=4).contains(&t.lanes)
}

/// Whether every location or binding is below 64 and appears once.
fn distinct(mut locs: impl Iterator<Item = u8>) -> bool {
    let mut seen = 0u64;
    locs.all(|l| {
        let bit = 1u64 << (l & 63);
        let fresh = l < 64 && seen & bit == 0;
        seen |= bit;
        fresh
    })
}

/// Checks a module.
pub fn validate(m: &Module) -> Result<(), IrError> {
    if m.version != VERSION {
        return Err(IrError::Version(m.version));
    }
    if m.body.len() > MAX_INSTRS {
        return Err(IrError::TooLarge { what: "instructions" });
    }
    let i = &m.interface;
    if i.inputs.len() > MAX_INPUTS
        || i.outputs.len() > MAX_OUTPUTS
        || i.resources.len() > MAX_RESOURCES
    {
        return Err(IrError::TooLarge { what: "interface" });
    }
    if i.push_bytes > MAX_PUSH_BYTES || i.push_bytes % 4 != 0 {
        return Err(IrError::Interface { what: "push-constant size" });
    }
    if !distinct(i.inputs.iter().map(|x| x.location))
        || !distinct(i.outputs.iter().map(|x| x.location))
    {
        return Err(IrError::Interface { what: "a location twice" });
    }
    if !distinct(i.resources.iter().map(|r| match r {
        Resource::Texture { binding, .. } | Resource::Sampler { binding } => *binding,
    })) {
        return Err(IrError::Interface { what: "a binding twice" });
    }
    if !i.inputs.iter().map(|x| x.ty).chain(i.outputs.iter().map(|x| x.ty)).all(valid_type) {
        return Err(IrError::Interface { what: "an input or output type" });
    }
    let mut stored = 0u32;
    for (at, ins) in m.body.iter().enumerate() {
        let bad = |what| IrError::Invalid { at, what };
        let ty = |v: ValueId| -> Result<Type, IrError> {
            if (v.0 as usize) < at {
                m.ty(v).filter(|t| t.lanes > 0).ok_or(bad("operand without a value"))
            } else {
                Err(bad("operand not defined before its use"))
            }
        };
        let t = ins.ty;
        if !matches!(ins.op, Op::StoreOutput(..)) && !valid_type(t) {
            return Err(bad("result type"));
        }
        match &ins.op {
            Op::Const(_) => {}
            Op::LoadInput(k) => {
                let inp = i.inputs.get(usize::from(*k)).ok_or(bad("input index"))?;
                if inp.ty != t {
                    return Err(bad("input type"));
                }
            }
            Op::LoadPush(off) => {
                let end = off.checked_add(4 * u32::from(t.lanes)).ok_or(bad("push offset"))?;
                if off % 4 != 0
                    || end > i.push_bytes
                    || !matches!(t.scalar, Scalar::F32 | Scalar::I32 | Scalar::U32)
                {
                    return Err(bad("push load"));
                }
            }
            Op::LoadDst(k) => {
                let out = i.outputs.get(usize::from(*k)).ok_or(bad("output index"))?;
                if i.stage != Stage::Fragment || out.ty != t {
                    return Err(bad("destination load"));
                }
            }
            Op::Unary(op, a) => {
                let a = ty(*a)?;
                let float_only = !matches!(op, UnOp::Neg | UnOp::Abs);
                if a != t
                    || t.scalar == Scalar::Bool
                    || (float_only && !t.scalar.is_float())
                    || (!float_only && t.scalar == Scalar::U32)
                {
                    return Err(bad("unary operand"));
                }
            }
            Op::Binary(_, a, b) => {
                if ty(*a)? != t || ty(*b)? != t || t.scalar == Scalar::Bool {
                    return Err(bad("binary operands"));
                }
            }
            Op::Fma(a, b, c) => {
                if ty(*a)? != t || ty(*b)? != t || ty(*c)? != t || !t.scalar.is_float() {
                    return Err(bad("fma operands"));
                }
            }
            Op::Compare(_, a, b) => {
                let (ta, tb) = (ty(*a)?, ty(*b)?);
                if ta != tb || t != Type::new(Scalar::Bool, ta.lanes) || ta.scalar == Scalar::Bool {
                    return Err(bad("compare operands"));
                }
            }
            Op::Select(c, a, b) => {
                let tc = ty(*c)?;
                if tc.scalar != Scalar::Bool
                    || (tc.lanes != 1 && tc.lanes != t.lanes)
                    || ty(*a)? != t
                    || ty(*b)? != t
                {
                    return Err(bad("select operands"));
                }
            }
            Op::Convert(a) => {
                if ty(*a)?.lanes != t.lanes {
                    return Err(bad("convert lanes"));
                }
            }
            Op::Construct(parts) => {
                let mut lanes = 0u8;
                for p in parts {
                    let tp = ty(*p)?;
                    if tp.scalar != t.scalar {
                        return Err(bad("construct part kind"));
                    }
                    lanes = lanes.saturating_add(tp.lanes);
                }
                if lanes != t.lanes {
                    return Err(bad("construct lanes"));
                }
            }
            Op::Extract(a, lane) => {
                let ta = ty(*a)?;
                if *lane >= ta.lanes || t != ta.scalar_of() {
                    return Err(bad("extract"));
                }
            }
            Op::Swizzle(a, lanes) => {
                let ta = ty(*a)?;
                if ta.scalar != t.scalar
                    || lanes[..usize::from(t.lanes)].iter().any(|l| *l >= ta.lanes)
                {
                    return Err(bad("swizzle"));
                }
            }
            Op::Splat(a) => {
                if ty(*a)? != t.scalar_of() {
                    return Err(bad("splat"));
                }
            }
            Op::Sample { texture, sampler, coords, lod } => {
                let dim = match i.resources.get(usize::from(*texture)) {
                    Some(Resource::Texture { dim, .. }) => *dim,
                    _ => return Err(bad("sample texture")),
                };
                if !matches!(i.resources.get(usize::from(*sampler)), Some(Resource::Sampler { .. }))
                {
                    return Err(bad("sample sampler"));
                }
                if ty(*coords)? != Type::new(Scalar::F32, dim.coords()) || t != Type::VEC4 {
                    return Err(bad("sample coordinates or result"));
                }
                match lod {
                    Lod::Implicit if i.stage != Stage::Fragment => {
                        return Err(bad("implicit lod outside a fragment program"))
                    }
                    Lod::Level(l) | Lod::Bias(l) if ty(*l)? != Type::F32 => {
                        return Err(bad("lod operand"))
                    }
                    _ => {}
                }
            }
            Op::StoreOutput(k, v) => {
                let out = i.outputs.get(usize::from(*k)).ok_or(bad("output index"))?;
                if ty(*v)? != out.ty || t.lanes != 0 {
                    return Err(bad("output store"));
                }
                let bit = 1u32 << k;
                if stored & bit != 0 {
                    return Err(bad("an output written twice"));
                }
                stored |= bit;
            }
        }
    }
    Ok(())
}
