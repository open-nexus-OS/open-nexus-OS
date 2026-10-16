// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The shader text's operands and lines (TASK-0215 5c-2 P6): a register as `FILE[index]`, a
//! source with its swizzle and its negate and absolute modifiers, a destination with its write
//! mask, an immediate's four lanes — the dialect the QEMU path's host renderer parses, as the
//! hand-written shaders it replaces wrote it.

use alloc::string::String;
use core::fmt::Write as _;

use crate::shader::ir::Scalar;

/// A register file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum File {
    Temp,
    Input,
    Output,
    Const,
    Imm,
}

impl File {
    fn name(self) -> &'static str {
        match self {
            Self::Temp => "TEMP",
            Self::Input => "IN",
            Self::Output => "OUT",
            Self::Const => "CONST",
            Self::Imm => "IMM",
        }
    }
}

const XYZW: [char; 4] = ['x', 'y', 'z', 'w'];

/// A source operand: four components picked by the swizzle, then the modifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Src {
    pub file: File,
    pub index: u16,
    pub swz: [u8; 4],
    pub neg: bool,
    pub abs: bool,
}

/// A destination: a register and the components written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Dst {
    file: File,
    index: u16,
    mask: [bool; 4],
}

impl Dst {
    /// Components `first..first + n` of a temporary.
    pub fn range(index: u16, first: u8, n: u8) -> Self {
        let mut mask = [false; 4];
        for m in mask.iter_mut().skip(usize::from(first)).take(usize::from(n)) {
            *m = true;
        }
        Self { file: File::Temp, index, mask }
    }

    /// A temporary's first `lanes` components.
    pub fn lanes(index: u16, lanes: u8) -> Self {
        Self::range(index, 0, lanes)
    }

    /// One component of a temporary.
    pub fn lane(index: u16, lane: u8) -> Self {
        Self::range(index, lane, 1)
    }

    /// An output's first `lanes` components.
    pub fn output(index: u16, lanes: u8) -> Self {
        Self { file: File::Output, ..Self::lanes(index, lanes) }
    }
}

fn write_src(out: &mut String, s: &Src) {
    if s.neg {
        out.push('-');
    }
    if s.abs {
        out.push('|');
    }
    let _ = write!(out, "{}[{}].", s.file.name(), s.index);
    for c in s.swz {
        out.push(XYZW[usize::from(c & 3)]);
    }
    if s.abs {
        out.push('|');
    }
}

fn write_dst(out: &mut String, d: &Dst) {
    let _ = write!(out, "{}[{}]", d.file.name(), d.index);
    if d.mask != [true; 4] {
        out.push('.');
        for (k, m) in d.mask.iter().enumerate() {
            if *m {
                out.push(XYZW[k]);
            }
        }
    }
}

/// `OP DST, SRC, …`.
pub(super) fn instr(out: &mut String, op: &str, dst: Dst, srcs: &[Src]) {
    out.push_str(op);
    out.push(' ');
    write_dst(out, &dst);
    for s in srcs {
        out.push_str(", ");
        write_src(out, s);
    }
    out.push('\n');
}

/// A 2D sample into a temporary: `OP TEMP[d], COORDS, SAMP[s], 2D`.
pub(super) fn sample(out: &mut String, op: &str, dst: u16, coords: Src, slot: u16) {
    let _ = write!(out, "{op} TEMP[{dst}], ");
    write_src(out, &coords);
    let _ = writeln!(out, ", SAMP[{slot}], 2D");
}

/// `IMM[k] FLT32 { a, b, c, d}` — a float as the shortest decimal that reads back to its bits;
/// conditions are floats here (1 or 0). Integers as they are.
pub(super) fn immediate(
    out: &mut String,
    k: usize,
    bits: &[u32; 4],
    scalar: Scalar,
) -> Result<(), &'static str> {
    let kind = match scalar {
        Scalar::F32 | Scalar::Bool => "FLT32",
        Scalar::I32 => "INT32",
        Scalar::U32 => "UINT32",
        Scalar::F16 => return Err("a half-float immediate"),
    };
    let _ = write!(out, "IMM[{k}] {kind} {{ ");
    for (i, b) in bits.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        match scalar {
            Scalar::F32 => {
                let x = f32::from_bits(*b);
                if !x.is_finite() {
                    return Err("a non-finite immediate");
                }
                let _ = write!(out, "{x}");
            }
            Scalar::Bool => out.push_str(if *b != 0 { "1.0" } else { "0.0" }),
            Scalar::I32 => {
                let _ = write!(out, "{}", *b as i32);
            }
            _ => {
                let _ = write!(out, "{b}");
            }
        }
    }
    out.push_str("}\n");
    Ok(())
}
