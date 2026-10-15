// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The group assembler against the open compiler's groups (TASK-0215 5c-2 P3a): each helper,
//! given the operands of a group of the base program, builds the same bytes — the form choice,
//! the phase organisation, the repeat, the flags. What a backend emits through is anchored on
//! encodings the hardware has executed.

use nexus_gfx::shader::rogue::alu::{op, AluInstr};
use nexus_gfx::shader::rogue::asm::{
    ctrl, fam, fmad, mov, pack_unorm8x4, sample_2d, shared, temp, unpack_unorm8x4, Main, ONE,
    PIXOUT0, ZERO,
};
use nexus_gfx::shader::rogue::ctrl::{CtrlInstr, Ditr};
use nexus_gfx::shader::rogue::{Group, RogueError};

const BASE: &[u8] = include_bytes!("../../../resources/gpu/programs/frag-base.usc");

/// The base program's group at `offset`, `len` bytes.
fn captured(offset: usize, len: usize) -> &'static [u8] {
    &BASE[offset..offset + len]
}

fn bytes(g: Group) -> Vec<u8> {
    let mut out = Vec::new();
    g.encode(&mut out).expect("the group encodes");
    out
}

fn built(m: Main) -> Vec<u8> {
    bytes(m.build().expect("the group builds"))
}

#[test]
fn the_iterate_and_the_data_fence_are_the_compilers() {
    // 0009: ditrp.pixel r0..1, drc0, cf4..11, cf0..3, 0x2
    let d = Ditr { dest: 0, coff: 1, persp: 1, woff: 0, count: 2, ..Ditr::default() };
    assert_eq!(bytes(ctrl(CtrlInstr::Ditr(d), 0)), captured(0x68, 8));
    // 0013: wdf drc0
    assert_eq!(bytes(ctrl(CtrlInstr::Wdf, 0)), captured(0x8e, 4));
}

#[test]
fn the_sample_is_the_compilers() {
    // 0028: smp.2d.fcnorm r0..3 <- texture state sh4..7, coordinates r0..1, sampler state sh12..15
    assert_eq!(
        built(sample_2d(temp(0), shared(4), temp(0), shared(12), 4, 0)),
        captured(0x102, 14)
    );
}

#[test]
fn the_multiply_add_and_fused_multiply_add_are_the_compilers() {
    // 0034: fmul r4 = r0 * r4
    assert_eq!(built(fam(op::FMUL, temp(4), temp(0), temp(4))), captured(0x13c, 10));
    // 0047: fadd.sat r11 = r5 + 0.0
    let mut m = fam(op::FADD, temp(11), temp(5), ZERO);
    m.alu[0].1 = AluInstr::Fam {
        op: op::FADD,
        sat: true,
        s0neg: false,
        s0abs: false,
        s1abs: false,
        s0flr: false,
    };
    assert_eq!(built(m), captured(0x1e0, 10));
    // 0049: fadd r12 = -r11 + 1.0
    let mut m = fam(op::FADD, temp(12), temp(11), ONE);
    m.alu[0].1 = AluInstr::Fam {
        op: op::FADD,
        sat: false,
        s0neg: true,
        s0abs: false,
        s1abs: false,
        s0flr: false,
    };
    assert_eq!(built(m), captured(0x1f4, 10));
    // 0051: fmad r4 = r4 * r11 + r5
    assert_eq!(built(fmad(temp(4), temp(4), temp(11), temp(5))), captured(0x208, 12));
}

#[test]
fn the_tile_read_the_unpack_the_pack_and_the_output_are_the_compilers() {
    // 0045: r6 = pixel output 0 (the tile's colour), output-lock checked
    let mut m = mov(temp(6), PIXOUT0);
    m.olchk = true;
    assert_eq!(built(m), captured(0x1ce, 8));
    // 0046: unpck.scale.u8888, repeat 4: r0..3 <- r6
    assert_eq!(built(unpack_unorm8x4(temp(0), temp(6))), captured(0x1d6, 10));
    // 0062: pck.scale.u8888, repeat 4: r0 <- r4..7
    assert_eq!(built(pack_unorm8x4(temp(0), temp(4))), captured(0x294, 14));
    // 0067: pixel output 0 = r0, output-lock checked, the program's end
    let mut m = mov(PIXOUT0, temp(0));
    m.olchk = true;
    m.end = true;
    assert_eq!(built(m), captured(0x2d8, 8));
}

#[test]
fn test_reject_a_phase_set_without_an_organisation_and_a_bad_repeat() {
    let mut m = Main::new();
    m.alu.push((nexus_gfx::shader::rogue::Phase::P1, AluInstr::Sngl { op: 7, ext: None }));
    assert!(matches!(
        m.build(),
        Err(RogueError::Malformed { what: "a phase set no organisation names", .. })
    ));
    let mut m = mov(temp(0), temp(1));
    m.repeat = 5;
    assert!(matches!(m.build(), Err(RogueError::TooWide { what: "repeat", .. })));
}
