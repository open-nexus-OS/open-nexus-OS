// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The board GPU's instruction encoding against captured programs (TASK-0215 5c-2 P1, H20):
//! each of the four fragment programs of `resources/gpu/programs` decodes into groups at the
//! offsets the open compiler's verbose listing printed for them, and re-encodes byte for byte —
//! every header, word, part and pad explained by a field of ours. Spot checks tie groups to the
//! listing's instructions; the rejects name truncation, reserved bits, padding and width.

use nexus_gfx::shader::rogue::alu::{sngl, AluInstr};
use nexus_gfx::shader::rogue::backend::BackendInstr;
use nexus_gfx::shader::rogue::bitwise::{Bitmask, BitwiseInstr};
use nexus_gfx::shader::rogue::ctrl::CtrlInstr;
use nexus_gfx::shader::rogue::header::{AluType, OpOrg};
use nexus_gfx::shader::rogue::{decode_program, encode_program, Group, Instr, Phase, RogueError};

const BASE: &[u8] = include_bytes!("../../../resources/gpu/programs/frag-base.usc");
const CHAIN: &[u8] = include_bytes!("../../../resources/gpu/programs/frag-chain.usc");
const EFT: &[u8] = include_bytes!("../../../resources/gpu/programs/frag-eft.usc");
const OPAQUE_EFT: &[u8] = include_bytes!("../../../resources/gpu/programs/frag-opaque-eft.usc");

// The group offsets of the open compiler's verbose listing (`PCO_DEBUG_PRINT=fs,verbose`).
const BASE_OFFSETS: [usize; 68] = [
    0x0, 0x8, 0x16, 0x26, 0x32, 0x3e, 0x4a, 0x58, 0x60, 0x68, 0x70, 0x7e, 0x86, 0x8e, 0x92, 0x9a,
    0xa2, 0xaa, 0xb2, 0xba, 0xc2, 0xca, 0xd2, 0xda, 0xe2, 0xea, 0xf2, 0xfa, 0x102, 0x110, 0x114,
    0x11e, 0x128, 0x132, 0x13c, 0x146, 0x150, 0x15a, 0x164, 0x172, 0x182, 0x194, 0x1a6, 0x1b4,
    0x1c0, 0x1ce, 0x1d6, 0x1e0, 0x1ea, 0x1f4, 0x1fe, 0x208, 0x214, 0x21e, 0x228, 0x234, 0x23e,
    0x248, 0x254, 0x262, 0x272, 0x284, 0x294, 0x2a2, 0x2b4, 0x2c4, 0x2ce, 0x2d8,
];
const CHAIN_OFFSETS: [usize; 116] = [
    0x0, 0x8, 0x16, 0x26, 0x32, 0x3e, 0x4a, 0x58, 0x60, 0x68, 0x70, 0x7e, 0x86, 0x8e, 0x92, 0x9a,
    0xa2, 0xaa, 0xb2, 0xba, 0xc2, 0xca, 0xd2, 0xda, 0xe2, 0xea, 0xf2, 0xfa, 0x102, 0x110, 0x114,
    0x11e, 0x128, 0x132, 0x13c, 0x146, 0x150, 0x15a, 0x164, 0x170, 0x17c, 0x188, 0x194, 0x1a0,
    0x1ac, 0x1b8, 0x1c4, 0x1d0, 0x1dc, 0x1e8, 0x1f4, 0x200, 0x20c, 0x218, 0x224, 0x230, 0x23c,
    0x248, 0x254, 0x260, 0x26c, 0x278, 0x284, 0x290, 0x29c, 0x2a8, 0x2b4, 0x2c0, 0x2cc, 0x2d8,
    0x2e4, 0x2f0, 0x2fc, 0x308, 0x314, 0x320, 0x32c, 0x338, 0x344, 0x350, 0x35c, 0x368, 0x374,
    0x380, 0x38c, 0x398, 0x3a4, 0x3b2, 0x3c2, 0x3d4, 0x3e6, 0x3f4, 0x400, 0x40e, 0x416, 0x420,
    0x42a, 0x434, 0x43e, 0x448, 0x454, 0x45e, 0x468, 0x474, 0x47e, 0x488, 0x494, 0x4a2, 0x4b2,
    0x4c4, 0x4d4, 0x4e2, 0x4f4, 0x504, 0x50e, 0x518,
];
const EFT_OFFSETS: [usize; 67] = [
    0x0, 0x8, 0x16, 0x26, 0x32, 0x3e, 0x4a, 0x58, 0x60, 0x68, 0x70, 0x7e, 0x86, 0x8e, 0x92, 0x9a,
    0xa2, 0xaa, 0xb2, 0xba, 0xc2, 0xca, 0xd2, 0xda, 0xe2, 0xea, 0xf2, 0xfa, 0x102, 0x110, 0x114,
    0x11e, 0x128, 0x132, 0x13c, 0x146, 0x150, 0x15a, 0x164, 0x172, 0x182, 0x194, 0x1a6, 0x1b4,
    0x1c0, 0x1ce, 0x1d6, 0x1e0, 0x1ea, 0x1f4, 0x1fe, 0x208, 0x214, 0x21e, 0x228, 0x234, 0x23e,
    0x248, 0x254, 0x262, 0x272, 0x284, 0x294, 0x2a2, 0x2aa, 0x2bc, 0x2d0,
];
const OPAQUE_EFT_OFFSETS: [usize; 55] = [
    0x0, 0x8, 0x16, 0x26, 0x32, 0x3e, 0x4a, 0x58, 0x66, 0x6e, 0x76, 0x7e, 0x8c, 0x94, 0x9c, 0xa0,
    0xa8, 0xb0, 0xb8, 0xc0, 0xc8, 0xd0, 0xd8, 0xe0, 0xe8, 0xf0, 0xf8, 0x100, 0x108, 0x110, 0x11e,
    0x122, 0x12c, 0x136, 0x140, 0x14a, 0x154, 0x15e, 0x168, 0x172, 0x180, 0x190, 0x1a2, 0x1b4,
    0x1c2, 0x1ce, 0x1dc, 0x1ea, 0x1fa, 0x20c, 0x21c, 0x22a, 0x232, 0x244, 0x258,
];

fn offsets(groups: &[Group]) -> Vec<usize> {
    let mut at = 0;
    groups
        .iter()
        .map(|g| {
            let o = at;
            at += g.size();
            o
        })
        .collect()
}

fn round_trip(program: &[u8], want_offsets: &[usize]) -> Vec<Group> {
    let groups = decode_program(program).expect("the program decodes");
    assert_eq!(offsets(&groups), want_offsets, "groups at the compiler's offsets");
    assert_eq!(encode_program(&groups).expect("the groups encode"), program, "byte for byte");
    groups
}

#[test]
fn the_base_program_round_trips() {
    round_trip(BASE, &BASE_OFFSETS);
}

#[test]
fn the_chain_program_round_trips() {
    round_trip(CHAIN, &CHAIN_OFFSETS);
}

#[test]
fn the_early_tests_program_round_trips() {
    round_trip(EFT, &EFT_OFFSETS);
}

#[test]
fn the_opaque_program_round_trips() {
    round_trip(OPAQUE_EFT, &OPAQUE_EFT_OFFSETS);
}

fn instr(g: &Group, phase: Phase) -> Instr {
    g.instrs.iter().find(|(p, _)| *p == phase).map(|(_, i)| *i).expect("the phase is in the group")
}

#[test]
fn the_base_programs_groups_are_the_listings_instructions() {
    let g = round_trip(BASE, &BASE_OFFSETS);
    // 0000: backend: savmsk.vm w0, w1; w0 = r0
    assert_eq!(
        (g[0].header.alutype, OpOrg::from_bits(g[0].header.ops)),
        (AluType::Main, OpOrg::Be)
    );
    assert_eq!(instr(&g[0], Phase::Backend), Instr::Backend(BackendInstr::SavMsk { mode: 0 }));
    assert!(g[0].header.w0p && g[0].dests.is_some());
    // 0001: bitwise p0 msk_bbyp0s1, p1 logical.or, p2 shift.shr
    assert_eq!(g[1].header.alutype, AluType::Bitwise);
    assert!(matches!(
        instr(&g[1], Phase::P0),
        Instr::Bitwise(BitwiseInstr::Phase0 { bitmask: Bitmask::Src(1), .. })
    ));
    assert!(matches!(
        instr(&g[1], Phase::P1),
        Instr::Bitwise(BitwiseInstr::Phase1 { logical_op: 0, .. })
    ));
    assert!(matches!(
        instr(&g[1], Phase::P2),
        Instr::Bitwise(BitwiseInstr::Phase2 { shift2: 1, .. })
    ));
    // 0002: p0 mbyp, p1 mbyp, p2_tst tst.u32.z.phase2end, p2_mov movc
    assert_eq!(instr(&g[2], Phase::P0), Instr::Alu(AluInstr::Sngl { op: sngl::BYP, ext: None }));
    assert!(matches!(instr(&g[2], Phase::P2Tst), Instr::Alu(AluInstr::Tst { op: 0, .. })));
    assert!(matches!(instr(&g[2], Phase::P2Mov), Instr::Alu(AluInstr::Movc { .. })));
    // 0007 / 0008: if(p0) / if(!p0) br.next
    assert!(matches!(instr(&g[7], Phase::Ctrl), Instr::Ctrl(CtrlInstr::Branch { .. })));
    assert_ne!(g[7].header.cc, g[8].header.cc, "the two branches' conditions differ");
    // 0009: ditrp.pixel r0..1, drc0, cf4..11, cf0..3, 0x2
    match instr(&g[9], Phase::Ctrl) {
        Instr::Ctrl(CtrlInstr::Ditr(d)) => {
            assert_eq!((d.dest, d.count, d.drc, d.mode), (0, 2, 0, 0))
        }
        other => panic!("not an iterate: {other:?}"),
    }
    // 0013: wdf drc0
    assert_eq!(instr(&g[13], Phase::Ctrl), Instr::Ctrl(CtrlInstr::Wdf));
    // 0028: smp.2d.fcnorm s4, drc0, ...
    match instr(&g[28], Phase::Backend) {
        Instr::Backend(BackendInstr::Smp { fcnorm, drc, dmn, exta, .. }) => {
            assert_eq!((fcnorm, drc, dmn, exta), (true, 0, 2, None))
        }
        other => panic!("not a sample: {other:?}"),
    }
    // 0065: if(p0) alphaf — the punch-through feedback
    assert!(matches!(
        instr(&g[65], Phase::Backend),
        Instr::Backend(BackendInstr::VisTest { atst: true, .. })
    ));
    // 0067: mbyp ft0, r0 → po0, .olchk.end
    let last = &g[67];
    assert!(last.header.end && last.header.olchk, "the last group ends the program");
}

#[test]
fn the_early_tests_program_has_no_feedback_and_the_base_one_has_it() {
    let has_vistest = |groups: &[Group]| {
        groups
            .iter()
            .flat_map(|g| g.instrs.iter())
            .any(|(_, i)| matches!(i, Instr::Backend(BackendInstr::VisTest { .. })))
    };
    assert!(has_vistest(&decode_program(BASE).unwrap()));
    assert!(!has_vistest(&decode_program(EFT).unwrap()));
}

#[test]
fn test_reject_a_truncated_group() {
    assert_eq!(decode_program(&BASE[..BASE_OFFSETS[1] - 1]), Err(RogueError::Truncated { at: 0 }));
}

#[test]
fn test_reject_a_reserved_header_bit() {
    let mut p = BASE.to_vec();
    // Group 0 has a three-byte main header; byte 2 bit 4 is reserved.
    p[2] |= 1 << 4;
    assert_eq!(decode_program(&p), Err(RogueError::Reserved { at: 0, what: "header" }));
}

#[test]
fn test_reject_bad_padding() {
    // The compiler aligns the program's last group by padding the group before it (0066: three
    // bytes of parts, a word pad and six alignment bytes).
    let groups = decode_program(BASE).unwrap();
    let (i, g) =
        groups.iter().enumerate().find(|(_, g)| g.align_pad > 0).expect("an aligned group");
    assert_eq!((i, g.align_pad), (groups.len() - 2, 6));
    let mut p = BASE.to_vec();
    let last_pad = BASE_OFFSETS[i] + g.size() - 1;
    p[last_pad] = 0x00;
    assert!(matches!(
        decode_program(&p),
        Err(RogueError::Malformed { what: "padding", at }) if at > BASE_OFFSETS[i] && at < last_pad
    ));
}

#[test]
fn test_reject_a_source_index_wider_than_its_form() {
    let mut groups = decode_program(BASE).unwrap();
    let s = groups[0].lower.as_mut().expect("a main group's lower sources");
    s.regs[0].index = 1 << 6; // the one-byte form holds six bits
    assert_eq!(
        encode_program(&groups),
        Err(RogueError::TooWide { what: "source index", value: 64 })
    );
}
