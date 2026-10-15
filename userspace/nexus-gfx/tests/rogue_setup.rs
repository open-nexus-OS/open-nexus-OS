// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A fragment pipeline's setup programs (TASK-0215 5c-2 P4b) against the render bundle's: the
//! iteration word for word, the kick byte for byte once placed at the captured program, the
//! state load's texture and tint loads as captured — the open driver's descriptor plumbing (two
//! further loads) not carried — and the size words; the library sections decode back, and every
//! setup the generator would not write is refused.

use nexus_gfx::backend::gpu::csb::ppp;
use nexus_gfx::backend::gpu::scene::Scene;
use nexus_gfx::backend::gpu::CoreId;
use nexus_gfx::shader::ir::{Input, Interface, Interp, Output, Resource, Stage, TexDim, Type};
use nexus_gfx::shader::rogue::codegen::{Layout, Program};
use nexus_gfx::shader::rogue::pds::{DoutDst, PdsInstr};
use nexus_gfx::shader::rogue::reference::{PASS_OPAQUE, PASS_TRANSLUCENT};
use nexus_gfx::shader::rogue::setup::{fragment, Dma, Load, Setup, Varyings};

const RENDER: &[u8] = include_bytes!("../../../resources/gpu/scenes/render-quad.scene");
const PDS_HEAP: u64 = 0xda_0000_0000;
const PDS_ARENA: u64 = 0xda_0000_1000;
/// The bundle's texture descriptor and tint block, as its state load names them.
const TEXTURE: u64 = 0x80_0004_b080;
const TINT: u64 = 0x80_0004_b100;

fn captured(heap: u64, n: usize) -> Vec<u32> {
    let scene = Scene::parse(RENDER, CoreId::BOARD).expect("the render bundle parses");
    let buf = scene.buffers().find(|b| b.va == PDS_ARENA).expect("the PDS arena");
    let mut bytes = vec![0u8; buf.len as usize];
    for run in buf.runs() {
        bytes[run.off as usize..run.off as usize + run.bytes.len()].copy_from_slice(run.bytes);
    }
    let at = (PDS_HEAP + heap - PDS_ARENA) as usize;
    bytes[at..at + 4 * n]
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

fn layer() -> Interface {
    Interface {
        stage: Stage::Fragment,
        inputs: vec![Input { location: 0, ty: Type::VEC2, interp: Interp::Perspective }],
        outputs: vec![Output { location: 0, ty: Type::VEC4 }],
        resources: vec![
            Resource::Texture { binding: 0, dim: TexDim::D2 },
            Resource::Sampler { binding: 1 },
        ],
        push_bytes: 16,
    }
}

fn program(temps: u8, pass_type: u8) -> Program {
    Program { groups: Vec::new(), temps, pass_type }
}

fn setup(temps: u8) -> Setup {
    fragment(&program(temps, PASS_OPAQUE), &layer(), Layout::BUNDLE, Varyings::BUNDLE)
        .expect("the layer's setup")
}

#[test]
fn the_iteration_is_the_bundles_word_for_word() {
    let s = setup(1);
    let mut ours = s.iterate.data.clone();
    ours.extend(s.iterate.code.iter().map(|i| i.encode().unwrap()));
    let mut theirs = captured(0x2940, 4);
    theirs.extend(captured(0x2950, 2));
    assert_eq!(ours, theirs);
    assert_eq!(s.iterate.code_offset(), 0x10);
}

#[test]
fn the_kick_placed_at_the_captured_program_is_the_bundles_byte_for_byte() {
    let kick = setup(5).kick_at(0x7b80).unwrap();
    assert_eq!(kick.code_offset(), 0x20, "the fixed eight-dword data segment");
    let mut theirs = captured(0x2900, 8);
    theirs.extend(captured(0x2920, 1));
    assert_eq!(words(&kick.encode().unwrap()), theirs);
    // Unplaced, the library carries offset zero.
    assert_eq!(setup(5).kick.data[0], 0);
}

#[test]
fn the_state_load_keeps_the_bundles_texture_and_tint_loads() {
    let s = setup(1);
    assert_eq!(
        s.dmas,
        vec![
            Dma { load: Load::TextureDescriptor { resource: 0 }, dest: 4, dwords: 20 },
            Dma { load: Load::Push { from: 0 }, dest: 24, dwords: 4 },
        ]
    );
    let data = s
        .state_data(|l| match l {
            Load::TextureDescriptor { .. } => TEXTURE,
            Load::Push { .. } => TINT,
        })
        .unwrap();
    let theirs = captured(0x2880, 12);
    // The two sources as captured; the texture's control as captured, the tint's now the last.
    assert_eq!(data[..4], theirs[..4]);
    assert_eq!(data[4], theirs[8]);
    assert_eq!(data[5], theirs[9] | 0x8000_0000);
    assert_eq!((data.len(), &data[6..]), (8, &[0, 0][..]));
    assert_eq!(
        s.state_code,
        vec![
            PdsInstr::Dout { cc: false, end: false, src1: 4, src0: 0, dst: DoutDst::D },
            PdsInstr::Dout { cc: false, end: true, src1: 5, src0: 1, dst: DoutDst::D },
        ]
    );
}

#[test]
fn the_size_words_follow_the_programs() {
    let (one, two) = setup(1).sizeinfo();
    // Uniform data in two units (the bundle's three carried two more loads), varyings one unit
    // of PDS data and one of coefficients (cf0–11), one unit of setup temporaries; shared
    // registers two units (sh0–27), triangle merging off as the open driver keeps it.
    assert_eq!(one, ppp::sizeinfo1(2, 0, 1, 1, 1));
    assert_eq!(two, 0x0100_4000, "the bundle's word");
}

#[test]
fn the_sections_decode_back_and_the_premultiplied_layer_differs_in_its_state() {
    let s = setup(1);
    let (sections, state) = s.sections().unwrap();
    let refs: Vec<&[u8]> = sections.iter().map(Vec::as_slice).collect();
    assert_eq!(Setup::from_sections(&refs, &state, &layer()).unwrap(), s);
    let p = fragment(&program(3, PASS_TRANSLUCENT), &layer(), Layout::BUNDLE, Varyings::BUNDLE)
        .unwrap();
    assert_eq!((p.iterate.clone(), p.dmas.clone()), (s.iterate.clone(), s.dmas.clone()));
    assert_eq!((p.state.temps, p.state.pass_type), (3, PASS_TRANSLUCENT));
}

#[test]
fn test_reject_setup_the_generator_would_not_write() {
    let s = setup(1);
    let (sections, state) = s.sections().unwrap();
    let decode = |sections: &[Vec<u8>], state: &[u8], i: &Interface| {
        let refs: Vec<&[u8]> = sections.iter().map(Vec::as_slice).collect();
        Setup::from_sections(&refs, state, i)
    };
    let with = |which: usize, f: &dyn Fn(&mut Vec<u8>)| {
        let mut s = sections.clone();
        f(&mut s[which]);
        decode(&s, &state, &layer()).is_err()
    };
    // A kick that names a program already, or whose code is another instruction.
    assert!(with(0, &|k| k[4] = 0x80));
    assert!(with(0, &|k| {
        let at = 4 + 4 * 8;
        k[at..at + 4].copy_from_slice(&0xf400_0000u32.to_le_bytes());
    }));
    // An iteration word that loads instead of iterating.
    assert!(with(1, &|it| {
        let at = 4 + 4 * 4;
        it[at..at + 4].copy_from_slice(&0xf000_0000u32.to_le_bytes());
    }));
    // A state load reading the sampler as a texture, the push constants past their block, or
    // beyond its shared registers.
    assert!(with(2, &|st| st[4 + 8 + 1] = 1));
    assert!(with(2, &|st| st[4 + 8 + 4 + 3] = 5));
    assert!(with(2, &|st| st[4 + 8 + 4 + 2] = 30));
    // A section's kind, a fourth section, a state of other temporaries or length.
    assert!(with(1, &|it| it[0] = 3));
    let mut four = sections.clone();
    four.push(sections[0].clone());
    assert!(decode(&four, &state, &layer()).is_err());
    let mut zero_temps = state.clone();
    zero_temps[0] = 0;
    assert!(decode(&sections, &zero_temps, &layer()).is_err());
    assert!(decode(&sections, &state[..7], &layer()).is_err());
    // Against an interface without push constants, the tint load is outside it.
    let bare = Interface { push_bytes: 0, ..layer() };
    assert!(decode(&sections, &state, &bare).is_err());
}

#[test]
fn test_reject_a_setup_the_board_layout_cannot_carry() {
    let p = program(1, PASS_OPAQUE);
    let flat = Interface {
        inputs: vec![Input { location: 0, ty: Type::VEC2, interp: Interp::Flat }],
        ..layer()
    };
    assert!(fragment(&p, &flat, Layout::BUNDLE, Varyings::BUNDLE).is_err());
    let two = Interface {
        resources: vec![
            Resource::Texture { binding: 0, dim: TexDim::D2 },
            Resource::Texture { binding: 1, dim: TexDim::D2 },
        ],
        ..layer()
    };
    assert!(fragment(&p, &two, Layout::BUNDLE, Varyings::BUNDLE).is_err());
    let apart = Layout { sampler_state: 16, ..Layout::BUNDLE };
    assert!(fragment(&p, &layer(), apart, Varyings::BUNDLE).is_err());
    let vertex = Interface { stage: Stage::Vertex, ..layer() };
    assert!(fragment(&p, &vertex, Layout::BUNDLE, Varyings::BUNDLE).is_err());
}
