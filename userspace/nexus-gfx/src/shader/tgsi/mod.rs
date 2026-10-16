// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The virtual GPU's backend (TASK-0215 5c-2 P6, ADR-0072 §4): a module of our IR
//! compiled into the shader text the QEMU path's host renderer parses (TGSI) — so the QEMU path
//! draws with the same WGSL programs as the board, and the hand-written shader text retires.
//! Every IR value lives in a register's components: a temporary an instruction writes, an input,
//! a push constant's slot of the constant buffer, an immediate; a lane, a swizzle or a splat is a
//! swizzle of its value's register, never an instruction. Blending is the pipeline's state on
//! this target (the virtual GPU has a blend unit and no tile read), so a program reading its
//! output (`LoadDst`) is refused: the library lowers a pipeline's main part and names its blend.
//! OWNERS: @gpu @runtime
//! STATUS: Experimental (P6)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/tgsi.rs` — the compositor's programs compiled to text, declarations and
//!   instructions as the host renderer reads them; the refusals.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;

use super::ir::builder::Blend;
use super::ir::validate::validate;
use super::ir::{
    BinOp, CmpOp, Interp, IrError, Lod, Module, Op, Resource, Scalar, Stage, TexDim, UnOp, ValueId,
    POSITION,
};

mod text;

use text::{Dst, File, Src};

/// The code target a library of this backend's programs is built for: the virtual GPU's shader
/// text, version 1.
pub const CODE_TARGET: u64 = u64::from_be_bytes(*b"TGSI\0\0\0\x01");

/// The most bytes of shader text an entry may carry.
pub const MAX_TEXT_BYTES: usize = 32 * 1024;

/// What a pipeline of this target fixes beside its program: the blend unit's mode (the program
/// computes the source colour; the unit composes it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    pub blend: Blend,
}

impl State {
    pub const BYTES: usize = 4;

    /// The library's state section: the blend (0 replace, 1 premultiplied over, 2 straight over),
    /// three zero bytes.
    pub fn to_bytes(&self) -> [u8; Self::BYTES] {
        let code = match self.blend {
            Blend::Replace => 0,
            Blend::OverPremultiplied => 1,
            Blend::OverStraight => 2,
        };
        [code, 0, 0, 0]
    }

    /// The state an entry carries, every byte the form [`State::to_bytes`] writes.
    pub fn from_bytes(b: &[u8]) -> Result<Self, TgsiError> {
        let blend = match b {
            [0, 0, 0, 0] => Blend::Replace,
            [1, 0, 0, 0] => Blend::OverPremultiplied,
            [2, 0, 0, 0] => Blend::OverStraight,
            _ => return Err(unsupported(0, "a state other than a blend")),
        };
        Ok(Self { blend })
    }
}

/// An entry's program as the device takes it: printable text within its bound, its header the
/// interface's stage, its last line `END` — the host renderer parses the rest and refuses what it
/// cannot (a program it refuses draws nothing).
pub fn check_text(bytes: &[u8], stage: Stage) -> Result<&str, TgsiError> {
    let bad = |what| Err(unsupported(0, what));
    if bytes.len() > MAX_TEXT_BYTES {
        return bad("shader text past its bound");
    }
    if !bytes.iter().all(|b| *b == b'\n' || (0x20..0x7f).contains(b)) {
        return bad("shader text outside printable characters");
    }
    let text = core::str::from_utf8(bytes).map_err(|_| unsupported(0, "shader text"))?;
    let header = match stage {
        Stage::Vertex => "VERT\n",
        Stage::Fragment => "FRAG\n",
        Stage::Compute => return bad("a compute program"),
    };
    if !text.starts_with(header) || !text.ends_with("\nEND\n") {
        return bad("shader text without its stage's header or its end");
    }
    Ok(text)
}

/// The instructions of compiled text: its lines past the header that declare nothing (its
/// budget's unit on this target).
pub fn instructions(text: &str) -> usize {
    text.lines()
        .skip(1)
        .filter(|l| !l.starts_with("DCL ") && !l.starts_with("IMM[") && *l != "END")
        .count()
}

/// Why a module did not become shader text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TgsiError {
    Ir(IrError),
    /// Instruction `at` is outside what this backend writes.
    Unsupported {
        at: usize,
        what: &'static str,
    },
}

/// Where a value lives: a register's components, the value's lanes at `swz[0..lanes]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Loc {
    file: File,
    index: u16,
    swz: [u8; 4],
    lanes: u8,
}

impl Loc {
    const fn whole(file: File, index: u16, lanes: u8) -> Self {
        Self { file, index, swz: [0, 1, 2, 3], lanes }
    }

    /// The value as a source: its lanes, the last repeated over the rest.
    fn src(&self) -> Src {
        let last = self.swz[usize::from(self.lanes.max(1)) - 1];
        let mut swz = [last; 4];
        swz[..usize::from(self.lanes)].copy_from_slice(&self.swz[..usize::from(self.lanes)]);
        Src { file: self.file, index: self.index, swz, neg: false, abs: false }
    }

    /// Lane `l` everywhere.
    fn lane(&self, l: u8) -> Src {
        let c = self.swz[usize::from(l.min(self.lanes.saturating_sub(1)))];
        Src { file: self.file, index: self.index, swz: [c; 4], neg: false, abs: false }
    }
}

struct Gen<'a> {
    m: &'a Module,
    locs: Vec<Option<Loc>>,
    temps: u16,
    /// Immediates, four lanes of bits each, with their kind.
    imms: Vec<([u32; 4], Scalar)>,
    body: String,
}

fn unsupported(at: usize, what: &'static str) -> TgsiError {
    TgsiError::Unsupported { at, what }
}

/// The opcode of a binary operation the virtual GPU has (subtraction is an add of the negation;
/// division a reciprocal and a product).
fn binop(op: BinOp) -> Option<&'static str> {
    Some(match op {
        BinOp::Add => "ADD",
        BinOp::Mul => "MUL",
        BinOp::Min => "MIN",
        BinOp::Max => "MAX",
        BinOp::Sub | BinOp::Div => return None,
    })
}

impl Gen<'_> {
    fn loc(&self, at: usize, v: ValueId) -> Result<Loc, TgsiError> {
        self.locs
            .get(v.0 as usize)
            .copied()
            .flatten()
            .ok_or(unsupported(at, "a value without a register"))
    }

    fn temp(&mut self, lanes: u8) -> Loc {
        let l = Loc::whole(File::Temp, self.temps, lanes);
        self.temps += 1;
        l
    }

    fn imm(&mut self, bits: [u32; 4], scalar: Scalar) -> u16 {
        let found = self.imms.iter().position(|(b, s)| *b == bits && *s == scalar);
        let i = found.unwrap_or_else(|| {
            self.imms.push((bits, scalar));
            self.imms.len() - 1
        });
        i as u16
    }

    fn emit(&mut self, op: &str, dst: Dst, srcs: &[Src]) {
        text::instr(&mut self.body, op, dst, srcs);
    }

    /// A one-source scalar opcode over each lane (the reciprocal, root, exponent, logarithm
    /// units read one component and replicate it).
    fn per_lane(&mut self, op: &str, a: Loc, lanes: u8) -> Loc {
        let d = self.temp(lanes);
        for l in 0..lanes {
            self.emit(op, Dst::lane(d.index, l), &[a.lane(l)]);
        }
        d
    }

    fn instr(&mut self, at: usize) -> Result<Option<Loc>, TgsiError> {
        let ins = &self.m.body[at];
        let (op, t) = (ins.op.clone(), ins.ty);
        let lanes = t.lanes;
        let arith = matches!(op, Op::Unary(..) | Op::Binary(..) | Op::Fma(..));
        if arith && t.scalar != Scalar::F32 {
            return Err(unsupported(at, "arithmetic on other than 32-bit floats"));
        }
        Ok(Some(match op {
            Op::Const(bits) => {
                let i = self.imm(bits, t.scalar);
                Loc::whole(File::Imm, i, lanes)
            }
            Op::LoadInput(i) => Loc::whole(File::Input, self.input_register(at, i)?, lanes),
            Op::LoadPush(off) => {
                let (slot, first) = (off / 16, (off % 16) / 4);
                if off % 4 != 0 || first + u32::from(lanes) > 4 {
                    return Err(unsupported(at, "a push constant across a constant slot"));
                }
                let mut swz = [0u8; 4];
                for (k, s) in swz.iter_mut().enumerate().take(usize::from(lanes)) {
                    *s = (first as u8) + k as u8;
                }
                Loc { file: File::Const, index: slot as u16, swz, lanes }
            }
            Op::LoadDst(_) => {
                return Err(unsupported(at, "a tile read (blending is the pipeline's state here)"))
            }
            Op::Unary(u, a) => {
                let a = self.loc(at, a)?;
                let mut s = a.src();
                let op = match u {
                    UnOp::Neg => {
                        s.neg = true;
                        "MOV"
                    }
                    UnOp::Abs => {
                        s.abs = true;
                        "MOV"
                    }
                    UnOp::Sat => "MOV_SAT",
                    UnOp::Floor => "FLR",
                    UnOp::Fract => "FRC",
                    UnOp::Rcp => return Ok(Some(self.per_lane("RCP", a, lanes))),
                    UnOp::Rsqrt => return Ok(Some(self.per_lane("RSQ", a, lanes))),
                    UnOp::Sqrt => return Ok(Some(self.per_lane("SQRT", a, lanes))),
                    UnOp::Exp2 => return Ok(Some(self.per_lane("EX2", a, lanes))),
                    UnOp::Log2 => return Ok(Some(self.per_lane("LG2", a, lanes))),
                };
                let d = self.temp(lanes);
                self.emit(op, Dst::lanes(d.index, lanes), &[s]);
                d
            }
            Op::Binary(b, x, y) => {
                let (x, y) = (self.loc(at, x)?, self.loc(at, y)?);
                match (binop(b), b) {
                    (Some(op), _) => {
                        let d = self.temp(lanes);
                        self.emit(op, Dst::lanes(d.index, lanes), &[x.src(), y.src()]);
                        d
                    }
                    (None, BinOp::Sub) => {
                        let d = self.temp(lanes);
                        let mut ny = y.src();
                        ny.neg = true;
                        self.emit("ADD", Dst::lanes(d.index, lanes), &[x.src(), ny]);
                        d
                    }
                    (None, _) => {
                        // x / y = x · (1 / y), the reciprocal a lane at a time.
                        let r = self.per_lane("RCP", y, lanes);
                        let d = self.temp(lanes);
                        self.emit("MUL", Dst::lanes(d.index, lanes), &[x.src(), r.src()]);
                        d
                    }
                }
            }
            Op::Fma(a, b, c) => {
                let (a, b, c) = (self.loc(at, a)?, self.loc(at, b)?, self.loc(at, c)?);
                let d = self.temp(lanes);
                self.emit("MAD", Dst::lanes(d.index, lanes), &[a.src(), b.src(), c.src()]);
                d
            }
            Op::Compare(c, x, y) => {
                // A condition is a float, 1 where it holds and 0 where not.
                let (x, y) = (self.loc(at, x)?, self.loc(at, y)?);
                let (op, a, b) = match c {
                    CmpOp::Lt => ("SLT", x, y),
                    CmpOp::Gt => ("SLT", y, x),
                    CmpOp::Ge => ("SGE", x, y),
                    CmpOp::Le => ("SGE", y, x),
                    CmpOp::Eq => ("SEQ", x, y),
                    CmpOp::Ne => ("SNE", x, y),
                };
                let d = self.temp(lanes);
                self.emit(op, Dst::lanes(d.index, lanes), &[a.src(), b.src()]);
                d
            }
            Op::Select(c, x, y) => {
                // CMP takes its second source where the first is negative: −condition.
                let (c, x, y) = (self.loc(at, c)?, self.loc(at, x)?, self.loc(at, y)?);
                let mut nc = if c.lanes == 1 { c.lane(0) } else { c.src() };
                nc.neg = true;
                let d = self.temp(lanes);
                self.emit("CMP", Dst::lanes(d.index, lanes), &[nc, x.src(), y.src()]);
                d
            }
            Op::Convert(v) => {
                let from = self.m.ty(v).map(|ty| ty.scalar).ok_or(unsupported(at, "a type"))?;
                let v = self.loc(at, v)?;
                let op = match (from, t.scalar) {
                    (a, b) if a == b => "MOV",
                    // A condition is already 1.0 or 0.0.
                    (Scalar::Bool, Scalar::F32) => "MOV",
                    (Scalar::F32, Scalar::I32) => "F2I",
                    (Scalar::F32, Scalar::U32) => "F2U",
                    (Scalar::I32, Scalar::F32) => "I2F",
                    (Scalar::U32, Scalar::F32) => "U2F",
                    _ => return Err(unsupported(at, "a conversion outside floats and integers")),
                };
                let d = self.temp(lanes);
                self.emit(op, Dst::lanes(d.index, lanes), &[v.src()]);
                d
            }
            Op::Construct(parts) => {
                // A vector of constants is one immediate.
                let locs = parts.iter().map(|p| self.loc(at, *p)).collect::<Result<Vec<_>, _>>()?;
                if locs.iter().all(|l| l.file == File::Imm) {
                    let mut bits = [0u32; 4];
                    let mut lane = 0usize;
                    for l in &locs {
                        let (imm, _) = self.imms[usize::from(l.index)];
                        for k in 0..usize::from(l.lanes) {
                            *bits.get_mut(lane).ok_or(unsupported(at, "a wide construction"))? =
                                imm[usize::from(l.swz[k])];
                            lane += 1;
                        }
                    }
                    let i = self.imm(bits, t.scalar);
                    return Ok(Some(Loc::whole(File::Imm, i, lanes)));
                }
                let d = self.temp(lanes);
                let mut lane = 0u8;
                for p in parts {
                    let p = self.loc(at, p)?;
                    let mut swz = [0u8; 4];
                    for k in 0..p.lanes {
                        swz[usize::from(lane + k)] = p.swz[usize::from(k)];
                    }
                    let src = Src { file: p.file, index: p.index, swz, neg: false, abs: false };
                    self.emit("MOV", Dst::range(d.index, lane, p.lanes), &[src]);
                    lane += p.lanes;
                }
                if lane != lanes {
                    return Err(unsupported(at, "a construction of other lanes than its type"));
                }
                d
            }
            Op::Extract(v, l) => {
                let v = self.loc(at, v)?;
                let mut out = v;
                out.swz = [v.swz[usize::from(l)]; 4];
                out.lanes = 1;
                out
            }
            Op::Swizzle(v, sel) => {
                let v = self.loc(at, v)?;
                let mut out = v;
                for (k, s) in sel.iter().enumerate().take(usize::from(lanes)) {
                    out.swz[k] = v.swz[usize::from(*s)];
                }
                out.lanes = lanes;
                out
            }
            Op::Splat(v) => {
                let v = self.loc(at, v)?;
                Loc { swz: [v.swz[0]; 4], lanes, ..v }
            }
            Op::Sample { texture, sampler: _, coords, lod } => {
                let slot = self.texture_slot(at, texture)?;
                let c = self.loc(at, coords)?;
                let d = self.temp(4);
                let (op, coords) = match lod {
                    Lod::Implicit => ("TEX", c.src()),
                    Lod::Level(l) | Lod::Bias(l) => {
                        // The level or bias rides in the coordinates' fourth component.
                        let l = self.loc(at, l)?;
                        let t = self.temp(4);
                        self.emit("MOV", Dst::range(t.index, 0, c.lanes), &[c.src()]);
                        self.emit("MOV", Dst::lane(t.index, 3), &[l.lane(0)]);
                        (if matches!(lod, Lod::Level(_)) { "TXL" } else { "TXB" }, t.src())
                    }
                };
                text::sample(&mut self.body, op, d.index, coords, slot);
                d
            }
            Op::StoreOutput(o, v) => {
                let v = self.loc(at, v)?;
                self.emit("MOV", Dst::output(u16::from(o), v.lanes), &[v.src()]);
                return Ok(None);
            }
        }))
    }

    /// An input's register: a vertex program's attribute is its location; a fragment program's
    /// varying its place in the interface (its location is its semantic's index).
    fn input_register(&self, at: usize, i: u8) -> Result<u16, TgsiError> {
        let inp = self.m.interface.inputs.get(usize::from(i)).ok_or(unsupported(at, "an input"))?;
        Ok(match self.m.interface.stage {
            Stage::Vertex => u16::from(inp.location),
            _ => u16::from(i),
        })
    }

    /// The sampler slot a texture's samples name: the texture's place among the textures.
    fn texture_slot(&self, at: usize, texture: u8) -> Result<u16, TgsiError> {
        let res = &self.m.interface.resources;
        match res.get(usize::from(texture)) {
            Some(Resource::Texture { dim: TexDim::D2, .. }) => Ok(res[..usize::from(texture)]
                .iter()
                .filter(|r| matches!(r, Resource::Texture { .. }))
                .count() as u16),
            _ => Err(unsupported(at, "a sample of other than a 2D texture")),
        }
    }
}

/// Compiles a vertex or fragment module into shader text.
pub fn compile(m: &Module) -> Result<String, TgsiError> {
    validate(m).map_err(TgsiError::Ir)?;
    let vertex = match m.interface.stage {
        Stage::Vertex => true,
        Stage::Fragment => false,
        Stage::Compute => return Err(unsupported(0, "a compute program")),
    };
    let n = m.body.len();
    let mut g =
        Gen { m, locs: alloc::vec![None; n], temps: 0, imms: Vec::new(), body: String::new() };
    for at in 0..n {
        g.locs[at] = g.instr(at)?;
    }
    let mut out = String::with_capacity(g.body.len() + 256);
    out.push_str(if vertex { "VERT\n" } else { "FRAG\n" });
    let i = &m.interface;
    for (k, inp) in i.inputs.iter().enumerate() {
        if vertex {
            let _ = writeln!(out, "DCL IN[{}]", inp.location);
            continue;
        }
        let interp = match inp.interp {
            Interp::Perspective => "PERSPECTIVE",
            Interp::Linear => "LINEAR",
            Interp::Flat => "CONSTANT",
        };
        let _ = writeln!(out, "DCL IN[{k}], GENERIC[{}], {interp}", inp.location);
    }
    for (k, o) in i.outputs.iter().enumerate() {
        let _ = match (vertex, o.location) {
            (true, POSITION) => writeln!(out, "DCL OUT[{k}], POSITION"),
            (true, n) => writeln!(out, "DCL OUT[{k}], GENERIC[{n}]"),
            (false, 0) => writeln!(out, "DCL OUT[{k}], COLOR"),
            (false, n) => writeln!(out, "DCL OUT[{k}], COLOR[{n}]"),
        };
    }
    let textures = i.resources.iter().filter(|r| matches!(r, Resource::Texture { .. })).count();
    for s in 0..textures {
        let _ = writeln!(out, "DCL SAMP[{s}]");
        let _ = writeln!(out, "DCL SVIEW[{s}], 2D, FLOAT");
    }
    if i.push_bytes > 0 {
        let _ = writeln!(out, "DCL CONST[0..{}]", i.push_bytes.div_ceil(16) - 1);
    }
    if g.temps > 0 {
        let _ = writeln!(out, "DCL TEMP[0..{}]", g.temps - 1);
    }
    for (k, (bits, scalar)) in g.imms.iter().enumerate() {
        text::immediate(&mut out, k, bits, *scalar).map_err(|what| unsupported(0, what))?;
    }
    out.push_str(&g.body);
    out.push_str("END\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_reads_back_and_test_reject_any_other_state() {
        for blend in [Blend::Replace, Blend::OverPremultiplied, Blend::OverStraight] {
            assert_eq!(State::from_bytes(&State { blend }.to_bytes()), Ok(State { blend }));
        }
        for bad in [&[3u8, 0, 0, 0][..], &[0, 1, 0, 0], &[0, 0, 0], &[0, 0, 0, 0, 0]] {
            assert!(State::from_bytes(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn test_reject_text_the_device_does_not_take() {
        let good = b"FRAG\nDCL OUT[0], COLOR\nEND\n";
        assert!(check_text(good, Stage::Fragment).is_ok());
        // Another stage's header, no end, a byte outside printable text, past the bound.
        assert!(check_text(good, Stage::Vertex).is_err());
        assert!(check_text(b"FRAG\nDCL OUT[0], COLOR\n", Stage::Fragment).is_err());
        assert!(check_text(b"FRAG\nMOV\tOUT[0]\nEND\n", Stage::Fragment).is_err());
        let mut long = b"FRAG\n".to_vec();
        long.resize(MAX_TEXT_BYTES, b' ');
        long.extend_from_slice(b"\nEND\n");
        assert!(check_text(&long, Stage::Fragment).is_err());
    }
}
