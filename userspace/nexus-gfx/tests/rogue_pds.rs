// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The setup programs' encoding against the render bundle (TASK-0215 5c-2 P1b): the draw's
//! programs the open driver generated — the fragment kick, the varying iteration, the texture
//! and constant loads, the two vertex fetches — decode into instructions and constants of ours
//! and re-encode word for word; the constants carry what the shader program reads (its offset,
//! its coefficient registers, its shared registers).

use nexus_gfx::backend::gpu::scene::Scene;
use nexus_gfx::backend::gpu::CoreId;
use nexus_gfx::shader::rogue::pds::{DmaCtl, DmaSrc, DoutDst, Douti, Doutu, PdsInstr, PdsProgram};
use nexus_gfx::shader::rogue::RogueError;

const RENDER: &[u8] = include_bytes!("../../../resources/gpu/scenes/render-quad.scene");
/// The PDS heap's base and the bundle's PDS arena (the heap's second page).
const PDS_HEAP: u64 = 0xda_0000_0000;
const PDS_ARENA: u64 = 0xda_0000_1000;

fn arena() -> Vec<u8> {
    let scene = Scene::parse(RENDER, CoreId::BOARD).expect("the render bundle parses");
    let buf = scene.buffers().find(|b| b.va == PDS_ARENA).expect("the PDS arena");
    let mut bytes = vec![0u8; buf.len as usize];
    for run in buf.runs() {
        bytes[run.off as usize..run.off as usize + run.bytes.len()].copy_from_slice(run.bytes);
    }
    bytes
}

/// Dwords at a PDS heap offset.
fn dwords(a: &[u8], heap: u64, n: usize) -> Vec<u32> {
    let at = (PDS_HEAP + heap - PDS_ARENA) as usize;
    a[at..at + n * 4]
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn qword(d: &[u32], i: usize) -> u64 {
    u64::from(d[i]) | (u64::from(d[i + 1]) << 32)
}

fn code(a: &[u8], heap: u64, n: usize) -> Vec<PdsInstr> {
    let words = dwords(a, heap, n);
    let instrs: Vec<PdsInstr> = words
        .iter()
        .enumerate()
        .map(|(i, w)| PdsInstr::decode(*w, i * 4).expect("an instruction"))
        .collect();
    let back: Vec<u32> = instrs.iter().map(|i| i.encode().expect("encodes")).collect();
    assert_eq!(back, words, "word for word");
    instrs
}

#[test]
fn the_fragment_kick_names_the_program_and_its_temporaries() {
    let a = arena();
    let data = dwords(&a, 0x2900, 2);
    let k = Doutu::decode(qword(&data, 0));
    assert_eq!(k, Doutu { exe_off: 0x7b80, temps: 5, sample_rate: 0, dual_phase: false });
    assert_eq!(k.encode().unwrap(), qword(&data, 0));
    assert_eq!(
        code(&a, 0x2920, 1),
        [PdsInstr::Dout { cc: false, end: true, src1: 0, src0: 0, dst: DoutDst::U }],
        "one kick that halts"
    );
    let p = PdsProgram { data: data.clone(), code: code(&a, 0x2920, 1) };
    assert_eq!((p.code_offset(), p.encode().unwrap().len()), (0x10, 0x14));
}

#[test]
fn the_varying_iteration_fills_the_coefficients_the_shader_reads() {
    let a = arena();
    let data = dwords(&a, 0x2940, 4);
    let w = Douti::decode(qword(&data, 0), 0).unwrap();
    let uv = Douti::decode(qword(&data, 2), 8).unwrap();
    // cf0..3: the W iteration; cf4..11: the uv varying, two components, perspective-correct.
    assert_eq!((w.dest, w.size, w.perspective, w.shademodel, w.last), (0, 0, false, 3, false));
    assert_eq!((uv.dest, uv.size, uv.perspective, uv.f32_offset, uv.last), (1, 1, true, 2, true));
    assert_eq!((w.encode().unwrap(), uv.encode().unwrap()), (qword(&data, 0), qword(&data, 2)));
    assert_eq!(
        code(&a, 0x2950, 2),
        [
            PdsInstr::Dout { cc: false, end: false, src1: 0, src0: 0, dst: DoutDst::I },
            PdsInstr::Dout { cc: false, end: true, src1: 0, src0: 1, dst: DoutDst::I },
        ]
    );
}

#[test]
fn the_state_load_dmas_the_texture_state_and_the_tint_into_shared_registers() {
    let a = arena();
    let instrs = code(&a, 0x2700, 4);
    assert!(instrs.iter().all(|i| matches!(i, PdsInstr::Dout { dst: DoutDst::D, .. })));
    let data = dwords(&a, 0x2880, 12);
    let srcs: Vec<DmaSrc> =
        (0..4).map(|i| DmaSrc::decode(qword(&data, 2 * i), i).unwrap()).collect();
    let ctls: Vec<DmaCtl> = data[8..12].iter().map(|w| DmaCtl::decode(*w)).collect();
    assert_eq!(srcs[0].sbase, 0x80_0004_b080, "the texture state in the general arena");
    // sh24..27 is the tint the shader multiplies with; the last DMA ends the program.
    assert_eq!((ctls[1].dest, ctls[1].a0, ctls[1].bsize), (1, 24, 4));
    assert!(ctls[3].last && !ctls[0].last);
    for (i, s) in srcs.iter().enumerate() {
        assert_eq!(s.encode().unwrap(), qword(&data, 2 * i));
    }
    for (c, w) in ctls.iter().zip(&data[8..12]) {
        assert_eq!(c.encode().unwrap(), *w);
    }
}

#[test]
fn the_vertex_fetches_dma_each_attribute_by_stride_times_index() {
    let a = arena();
    for heap in [0x2740, 0x2780] {
        let instrs = code(&a, heap, if heap == 0x2740 { 4 } else { 5 });
        assert_eq!(instrs.iter().filter(|i| matches!(i, PdsInstr::Ddmad { .. })).count(), 2);
        assert!(matches!(instrs.last(), Some(PdsInstr::Halt { cc: false })));
    }
    let data = dwords(&a, 0x2840, 16);
    assert_eq!(Doutu::decode(qword(&data, 0)).exe_off, 0x7ac0, "the vertex program");
    let pos = DmaCtl::decode(data[6]);
    let uv = DmaCtl::decode(data[14]);
    assert_eq!((data[3], pos.a0, pos.bsize, pos.last), (16, 0, 2, false), "position: two dwords");
    assert_eq!((data[11], uv.a0, uv.bsize, uv.last), (16, 2, 2, true), "uv: two dwords after it");
}

#[test]
fn test_reject_an_unknown_setup_instruction_and_a_too_wide_operand() {
    assert!(matches!(PdsInstr::decode(0x7000_0000, 0), Err(RogueError::Unsupported { .. })));
    assert!(matches!(
        PdsInstr::Dout { cc: false, end: false, src1: 0, src0: 0x80, dst: DoutDst::D }.encode(),
        Err(RogueError::TooWide { what: "dout src0", .. })
    ));
    assert!(matches!(
        Doutu { exe_off: 0x7b81, ..Doutu::default() }.encode(),
        Err(RogueError::TooWide { .. })
    ));
}
