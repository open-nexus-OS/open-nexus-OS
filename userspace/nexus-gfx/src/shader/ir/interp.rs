// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The reference interpreter: one invocation of a module at a time, the meaning every backend
//! must reproduce. Lanes hold their bits; 16-bit floats are kept as the 32-bit floats they
//! round to (round to nearest even) after every operation that produces one. Textures are the
//! caller's (a trait over normalized coordinates); `LoadDst` reads the tile's value the caller
//! hands in, so an epilog's blend is checked like any other code.

use alloc::vec;
use alloc::vec::Vec;

use super::validate::validate;
use super::{BinOp, CmpOp, IrError, Lod, Module, Op, Scalar, UnOp, ValueId};

/// A texture the interpreter samples at normalized coordinates.
pub trait Texture {
    /// The filtered texel at `coords` (unused lanes zero); `lod` an explicit level, if any.
    fn sample(&self, coords: [f32; 3], lod: Option<f32>) -> [f32; 4];
}

/// What one invocation reads.
pub struct Invocation<'a> {
    /// The interface's inputs, in its order (integer inputs are converted from these floats).
    pub inputs: &'a [[f32; 4]],
    pub push: &'a [u8],
    /// Textures by resource index (`None` where the resource is not one).
    pub textures: &'a [Option<&'a dyn Texture>],
    /// The tile's current value per output.
    pub dst: &'a [[f32; 4]],
}

/// A float's nearest binary16 value, as a float (round to nearest, ties to even).
pub fn round_f16(x: f32) -> f32 {
    let a = libm::fabsf(x);
    if a.is_nan() || a.is_infinite() {
        return x;
    }
    if a >= 65520.0 {
        return if x < 0.0 { f32::NEG_INFINITY } else { f32::INFINITY };
    }
    // The quantum: 2^-24 for subnormals, else 2^(e-10) for the value's exponent e.
    let q = if a < 6.103_515_6e-5 {
        5.960_464_5e-8
    } else {
        f32::from_bits(((a.to_bits() >> 23).saturating_sub(10)) << 23)
    };
    let r = libm::rintf(a / q) * q;
    if x < 0.0 {
        -r
    } else {
        r
    }
}

fn f(bits: u32) -> f32 {
    f32::from_bits(bits)
}

fn store_float(scalar: Scalar, x: f32) -> u32 {
    match scalar {
        Scalar::F16 => round_f16(x).to_bits(),
        _ => x.to_bits(),
    }
}

/// Runs one invocation; the outputs in the interface's order (unwritten ones zero).
pub fn run(m: &Module, inv: &Invocation<'_>) -> Result<Vec<[f32; 4]>, IrError> {
    validate(m)?;
    let mut vals: Vec<[u32; 4]> = Vec::with_capacity(m.body.len());
    let mut outs = vec![[0f32; 4]; m.interface.outputs.len()];
    for ins in &m.body {
        let t = ins.ty;
        let get = |v: &ValueId| vals[v.0 as usize];
        let ty_of = |v: &ValueId| m.body[v.0 as usize].ty;
        let lanes = usize::from(t.lanes);
        let mut r = [0u32; 4];
        match &ins.op {
            Op::Const(bits) => {
                for (d, s) in r.iter_mut().zip(bits).take(lanes) {
                    *d = if t.scalar.is_float() { store_float(t.scalar, f(*s)) } else { *s };
                }
            }
            Op::LoadInput(k) => {
                let src =
                    inv.inputs.get(usize::from(*k)).ok_or(IrError::Missing { what: "an input" })?;
                for (d, s) in r.iter_mut().zip(src).take(lanes) {
                    *d = match t.scalar {
                        Scalar::F32 | Scalar::F16 => store_float(t.scalar, *s),
                        Scalar::I32 => (*s as i32) as u32,
                        Scalar::U32 => *s as u32,
                        Scalar::Bool => u32::from(*s != 0.0),
                    };
                }
            }
            Op::LoadPush(off) => {
                for (i, d) in r.iter_mut().enumerate().take(lanes) {
                    let at = *off as usize + 4 * i;
                    let b = inv
                        .push
                        .get(at..at + 4)
                        .ok_or(IrError::Missing { what: "push constants" })?;
                    *d = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                }
            }
            Op::LoadDst(k) => {
                let src = inv
                    .dst
                    .get(usize::from(*k))
                    .ok_or(IrError::Missing { what: "the tile's value" })?;
                for (d, s) in r.iter_mut().zip(src).take(lanes) {
                    *d = s.to_bits();
                }
            }
            Op::Unary(op, a) => {
                let a = get(a);
                for i in 0..lanes {
                    r[i] = match t.scalar {
                        Scalar::F32 | Scalar::F16 => {
                            let x = f(a[i]);
                            let y = match op {
                                UnOp::Neg => -x,
                                UnOp::Abs => libm::fabsf(x),
                                UnOp::Floor => libm::floorf(x),
                                UnOp::Fract => x - libm::floorf(x),
                                UnOp::Rcp => 1.0 / x,
                                UnOp::Rsqrt => 1.0 / libm::sqrtf(x),
                                UnOp::Sqrt => libm::sqrtf(x),
                                UnOp::Exp2 => libm::exp2f(x),
                                UnOp::Log2 => libm::log2f(x),
                                UnOp::Sat => x.clamp(0.0, 1.0),
                            };
                            store_float(t.scalar, y)
                        }
                        _ => {
                            let x = a[i] as i32;
                            (match op {
                                UnOp::Neg => x.wrapping_neg(),
                                _ => x.wrapping_abs(),
                            }) as u32
                        }
                    };
                }
            }
            Op::Binary(op, a, b) => {
                let (a, b) = (get(a), get(b));
                for i in 0..lanes {
                    r[i] = match t.scalar {
                        Scalar::F32 | Scalar::F16 => {
                            let (x, y) = (f(a[i]), f(b[i]));
                            let z = match op {
                                BinOp::Add => x + y,
                                BinOp::Sub => x - y,
                                BinOp::Mul => x * y,
                                BinOp::Div => x / y,
                                BinOp::Min => libm::fminf(x, y),
                                BinOp::Max => libm::fmaxf(x, y),
                            };
                            store_float(t.scalar, z)
                        }
                        Scalar::I32 => {
                            let (x, y) = (a[i] as i32, b[i] as i32);
                            (match op {
                                BinOp::Add => x.wrapping_add(y),
                                BinOp::Sub => x.wrapping_sub(y),
                                BinOp::Mul => x.wrapping_mul(y),
                                BinOp::Div => x.checked_div(y).unwrap_or(0),
                                BinOp::Min => x.min(y),
                                BinOp::Max => x.max(y),
                            }) as u32
                        }
                        _ => {
                            let (x, y) = (a[i], b[i]);
                            match op {
                                BinOp::Add => x.wrapping_add(y),
                                BinOp::Sub => x.wrapping_sub(y),
                                BinOp::Mul => x.wrapping_mul(y),
                                BinOp::Div => x.checked_div(y).unwrap_or(0),
                                BinOp::Min => x.min(y),
                                BinOp::Max => x.max(y),
                            }
                        }
                    };
                }
            }
            Op::Fma(a, b, c) => {
                let (a, b, c) = (get(a), get(b), get(c));
                for i in 0..lanes {
                    r[i] = store_float(t.scalar, libm::fmaf(f(a[i]), f(b[i]), f(c[i])));
                }
            }
            Op::Compare(op, a, b) => {
                let ta = ty_of(a);
                let (a, b) = (get(a), get(b));
                for i in 0..lanes {
                    let ord = match ta.scalar {
                        Scalar::F32 | Scalar::F16 => f(a[i]).partial_cmp(&f(b[i])),
                        Scalar::I32 => Some((a[i] as i32).cmp(&(b[i] as i32))),
                        _ => Some(a[i].cmp(&b[i])),
                    };
                    use core::cmp::Ordering::*;
                    let yes = match (op, ord) {
                        (_, None) => matches!(op, CmpOp::Ne),
                        (CmpOp::Lt, Some(o)) => o == Less,
                        (CmpOp::Le, Some(o)) => o != Greater,
                        (CmpOp::Eq, Some(o)) => o == Equal,
                        (CmpOp::Ne, Some(o)) => o != Equal,
                        (CmpOp::Ge, Some(o)) => o != Less,
                        (CmpOp::Gt, Some(o)) => o == Greater,
                    };
                    r[i] = u32::from(yes);
                }
            }
            Op::Select(c, a, b) => {
                let one = ty_of(c).lanes == 1;
                let (c, a, b) = (get(c), get(a), get(b));
                for i in 0..lanes {
                    r[i] = if c[if one { 0 } else { i }] != 0 { a[i] } else { b[i] };
                }
            }
            Op::Convert(a) => {
                let from = ty_of(a).scalar;
                let a = get(a);
                for i in 0..lanes {
                    let x = a[i];
                    r[i] = match (from, t.scalar) {
                        (Scalar::F32 | Scalar::F16, Scalar::F32 | Scalar::F16) => {
                            store_float(t.scalar, f(x))
                        }
                        (Scalar::F32 | Scalar::F16, Scalar::I32) => {
                            (libm::truncf(f(x)) as i32) as u32
                        }
                        (Scalar::F32 | Scalar::F16, Scalar::U32) => libm::truncf(f(x)) as u32,
                        (Scalar::I32, Scalar::F32 | Scalar::F16) => {
                            store_float(t.scalar, (x as i32) as f32)
                        }
                        (Scalar::U32, Scalar::F32 | Scalar::F16) => store_float(t.scalar, x as f32),
                        (Scalar::Bool, Scalar::F32 | Scalar::F16) => {
                            store_float(t.scalar, x as f32)
                        }
                        (_, Scalar::Bool) => {
                            u32::from(if from.is_float() { f(x) != 0.0 } else { x != 0 })
                        }
                        _ => x,
                    };
                }
            }
            Op::Construct(parts) => {
                let mut i = 0;
                for p in parts {
                    let (pv, pl) = (get(p), usize::from(ty_of(p).lanes));
                    for lane in pv.iter().take(pl) {
                        if i < 4 {
                            r[i] = *lane;
                        }
                        i += 1;
                    }
                }
            }
            Op::Extract(a, lane) => r[0] = get(a)[usize::from(*lane)],
            Op::Swizzle(a, sel) => {
                let a = get(a);
                for i in 0..lanes {
                    r[i] = a[usize::from(sel[i])];
                }
            }
            Op::Splat(a) => {
                let x = get(a)[0];
                r[..lanes].fill(x);
            }
            Op::Sample { texture, coords, lod, .. } => {
                let tex = inv
                    .textures
                    .get(usize::from(*texture))
                    .copied()
                    .flatten()
                    .ok_or(IrError::Missing { what: "a texture" })?;
                let (cv, cl) = (get(coords), usize::from(ty_of(coords).lanes));
                let mut c = [0f32; 3];
                for (d, s) in c.iter_mut().zip(cv).take(cl) {
                    *d = f(s);
                }
                let l = match lod {
                    Lod::Implicit => None,
                    Lod::Level(v) | Lod::Bias(v) => Some(f(get(v)[0])),
                };
                let texel = tex.sample(c, l);
                for (d, s) in r.iter_mut().zip(texel) {
                    *d = s.to_bits();
                }
            }
            Op::StoreOutput(k, v) => {
                let tv = ty_of(v);
                let v = get(v);
                let out = &mut outs[usize::from(*k)];
                for i in 0..usize::from(tv.lanes) {
                    out[i] = if tv.scalar.is_float() { f(v[i]) } else { v[i] as f32 };
                }
            }
        }
        vals.push(r);
    }
    Ok(outs)
}

/// The reference sampler of an RGBA8 image: normalized coordinates, texel centres at
/// `(i + 0.5) / size`, bilinear filtering, clamp to edge, channels as unorm floats — the
/// semantics a backend's texture state must give the hardware sampler.
pub struct Rgba8Texture {
    width: u32,
    height: u32,
    texels: Vec<u8>,
}

impl Rgba8Texture {
    /// A texture of `width × height` texels, tightly packed RGBA8 rows.
    pub fn new(width: u32, height: u32, texels: Vec<u8>) -> Option<Self> {
        let len = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
        (width > 0 && height > 0 && texels.len() == len).then_some(Self { width, height, texels })
    }

    fn texel(&self, x: i64, y: i64) -> [f32; 4] {
        let x = x.clamp(0, i64::from(self.width) - 1) as usize;
        let y = y.clamp(0, i64::from(self.height) - 1) as usize;
        let at = (y * self.width as usize + x) * 4;
        let mut out = [0f32; 4];
        for (o, b) in out.iter_mut().zip(&self.texels[at..at + 4]) {
            *o = f32::from(*b) / 255.0;
        }
        out
    }
}

impl Texture for Rgba8Texture {
    fn sample(&self, c: [f32; 3], _lod: Option<f32>) -> [f32; 4] {
        let u = c[0] * self.width as f32 - 0.5;
        let v = c[1] * self.height as f32 - 0.5;
        let (x0, y0) = (libm::floorf(u), libm::floorf(v));
        let (fx, fy) = (u - x0, v - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let mut out = [0f32; 4];
        for (dx, dy, w) in [
            (0, 0, (1.0 - fx) * (1.0 - fy)),
            (1, 0, fx * (1.0 - fy)),
            (0, 1, (1.0 - fx) * fy),
            (1, 1, fx * fy),
        ] {
            let t = self.texel(x0 + dx, y0 + dy);
            for (o, x) in out.iter_mut().zip(t) {
                *o += w * x;
            }
        }
        out
    }
}
