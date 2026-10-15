// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Reference programs, assembled by hand from the goldens-proven helpers (TASK-0215 5c-2 P3b):
//! the compositor's textured layer the shortest ways this core allows, each an answer to one
//! question the board decides, each later a golden the backend must reproduce. They read the
//! registers the render bundle's setup programs fill: the W coefficients at cf0, the uv
//! coefficients at cf4, the texture state at sh4, the sampler state at sh12, the tint at sh24.
//!
//! | Program | Groups | Question |
//! |---|---|---|
//! | `opaque-ditr` | 5 | the compiler's iterate and sample, without its prologue, feedback, coordinate zeroing and tile read-back; the pack straight into the pixel output |
//! | `opaque-itrsmp` | 3 | iterate-and-sample in one word (the six-byte form) |
//! | `opaque-itrsmp-count` | 3 | the same with an iteration count (the seven-byte form) |
//! | `tinted`, `tinted-itrsmp` | 6, 4 | a multiply repeated over four channels steps every register operand |
//! | `premultiplied`, `premultiplied-itrsmp` | 13, 11 | the compositor's blend: the tile read, unpacked, `src + dst·(1 − a)` |
//!
//! Registers follow the backend's rule (`codegen`): first fit, and an operation that steps lane
//! by lane writes over an operand that dies with it — so the backend compiling the layer's IR
//! must give these bytes exactly (the goldens of `tests/rogue_codegen.rs`).
use alloc::vec;
use alloc::vec::Vec;

use super::alu::{op, AluInstr};
use super::asm::{
    ctrl, fam, fmad, mov, pack_unorm8x4, sample_2d, shared, temp, unpack_unorm8x4, ONE, PIXOUT0,
};
use super::ctrl::{CtrlInstr, Ditr, Itrsmp, ItrsmpExt};
use super::group::{align_end, encode_program, Group};
use super::RogueError;

/// The ISP pass types a reference program asks for.
pub const PASS_OPAQUE: u8 = 0;
pub const PASS_TRANSLUCENT: u8 = 1;

/// A reference program and what its kick and draw state must say.
pub struct Reference {
    pub name: &'static str,
    pub groups: Vec<Group>,
    /// Temporaries in units of four (the kick word's field).
    pub temps: u8,
    pub pass_type: u8,
}

impl Reference {
    pub fn encode(&self) -> Result<Vec<u8>, RogueError> {
        encode_program(&self.groups)
    }
}

/// The compiler's way in: iterate uv into r0..1, wait, sample r0..3, wait.
fn iterate_then_sample() -> Result<Vec<Group>, RogueError> {
    let iterate = Ditr { dest: 0, coff: 1, persp: 1, woff: 0, count: 2, ..Ditr::default() };
    Ok(vec![
        ctrl(CtrlInstr::Ditr(iterate), 0),
        ctrl(CtrlInstr::Wdf, 0),
        sample_2d(temp(0), shared(4), temp(0), shared(12), 4, 0).build()?,
        ctrl(CtrlInstr::Wdf, 0),
    ])
}

/// Iterate-and-sample into r0..3, then wait.
fn iterate_and_sample(count: Option<u8>) -> Vec<Group> {
    let word = Itrsmp {
        dest: 0,
        coff: 1,
        persp: 1,
        texoff: 1,
        smpoff: 3,
        dmn: 2,
        chan: 3,
        fcnorm: true,
        ext: count.map(|count| ItrsmpExt { count, comparison: false, idx: None }),
        ..Itrsmp::default()
    };
    vec![ctrl(CtrlInstr::Itrsmp(word), 0), ctrl(CtrlInstr::Wdf, 0)]
}

/// Packs r(src)..+4 straight into the pixel output: the program's last group.
fn emit(src: u16) -> Result<Group, RogueError> {
    let mut m = pack_unorm8x4(PIXOUT0, temp(src));
    m.olchk = true;
    m.end = true;
    m.build()
}

fn finish(
    name: &'static str,
    mut groups: Vec<Group>,
    temps: u8,
    pass_type: u8,
) -> Result<Reference, RogueError> {
    align_end(&mut groups)?;
    Ok(Reference { name, groups, temps, pass_type })
}

pub fn opaque_ditr() -> Result<Reference, RogueError> {
    let mut g = iterate_then_sample()?;
    g.push(emit(0)?);
    finish("opaque-ditr", g, 1, PASS_OPAQUE)
}

pub fn opaque_itrsmp() -> Result<Reference, RogueError> {
    let mut g = iterate_and_sample(None);
    g.push(emit(0)?);
    finish("opaque-itrsmp", g, 1, PASS_OPAQUE)
}

pub fn opaque_itrsmp_count() -> Result<Reference, RogueError> {
    let mut g = iterate_and_sample(Some(2));
    g.push(emit(0)?);
    finish("opaque-itrsmp-count", g, 1, PASS_OPAQUE)
}

/// r0..3 = sh24..27 × r0..3 in place, one multiply repeated over the four channels. The shared
/// register is the first source: the second source's bank field has two bits and cannot name
/// bank 4 (the open compiler copies the tint into temporaries first, four groups).
fn tint() -> Result<Group, RogueError> {
    let mut m = fam(op::FMUL, temp(0), shared(24), temp(0));
    m.repeat = 4;
    m.build()
}

/// The texel into r0..3: iterate-and-sample, or iterate and sample.
fn texel(fused: bool) -> Result<Vec<Group>, RogueError> {
    if fused {
        Ok(iterate_and_sample(None))
    } else {
        iterate_then_sample()
    }
}

fn tinted_program(name: &'static str, fused: bool) -> Result<Reference, RogueError> {
    let mut g = texel(fused)?;
    g.push(tint()?);
    g.push(emit(0)?);
    finish(name, g, 1, PASS_OPAQUE)
}

pub fn tinted() -> Result<Reference, RogueError> {
    tinted_program("tinted", false)
}

pub fn tinted_itrsmp() -> Result<Reference, RogueError> {
    tinted_program("tinted-itrsmp", true)
}

fn premultiplied_program(name: &'static str, fused: bool) -> Result<Reference, RogueError> {
    let mut g = texel(fused)?;
    g.push(tint()?);
    // The tile's colour: the word in r4, unpacked to r5..8.
    let mut read = mov(temp(4), PIXOUT0);
    read.olchk = true;
    g.push(read.build()?);
    g.push(unpack_unorm8x4(temp(5), temp(4)).build()?);
    // r4 = 1 − a (the word's register is free again).
    let mut inv = fam(op::FADD, temp(4), temp(3), ONE);
    inv.alu[0].1 = AluInstr::Fam {
        op: op::FADD,
        sat: false,
        s0neg: true,
        s0abs: false,
        s1abs: false,
        s0flr: false,
    };
    g.push(inv.build()?);
    // r0+i = dst_i · (1 − a) + src_i, in place.
    for i in 0..4 {
        g.push(fmad(temp(i), temp(5 + i), temp(4), temp(i)).build()?);
    }
    g.push(emit(0)?);
    finish(name, g, 3, PASS_TRANSLUCENT)
}

pub fn premultiplied() -> Result<Reference, RogueError> {
    premultiplied_program("premultiplied", false)
}

pub fn premultiplied_itrsmp() -> Result<Reference, RogueError> {
    premultiplied_program("premultiplied-itrsmp", true)
}

/// Every reference program, in the order a board cycle runs them: the known encodings first,
/// iterate-and-sample (never executed by any capture) last, so a lost pass costs no other answer.
pub fn all() -> Result<Vec<Reference>, RogueError> {
    Ok(vec![
        opaque_ditr()?,
        tinted()?,
        premultiplied()?,
        opaque_itrsmp()?,
        opaque_itrsmp_count()?,
        tinted_itrsmp()?,
        premultiplied_itrsmp()?,
    ])
}
