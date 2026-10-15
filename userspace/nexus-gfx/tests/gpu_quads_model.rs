// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The quad emitter's goldens (TASK-0215 5b, RFC-0101 §8d; drawn with a library pipeline since
//! 5c-2 P4d): one quad at the capture's offsets is the one-quad capture byte for byte but for the
//! pipeline's own words — the pass type, the setup's size word, the state load's two loads where
//! the capture had four — and a second quad the two-quad capture's growth and blocks, both draws
//! naming the pipeline's setup programs placed once; the bump placer puts every pass in fresh
//! lines (H10); overflow and malformed templates are refused. The captured words:
//! `docs/board/measurements/2026-10-13-gpu-render-queue/capture/` (`render1`, `variants/r_2`).

use nexus_gfx::backend::gpu::csb::{pds, ppp};
use nexus_gfx::backend::gpu::pipelines::Placed;
use nexus_gfx::backend::gpu::quads::*;
use nexus_gfx::shader::ir::{Input, Interface, Interp, Output, Resource, Stage, TexDim, Type};
use nexus_gfx::shader::rogue::codegen::{Layout, Program};
use nexus_gfx::shader::rogue::reference::{PASS_OPAQUE, PASS_TRANSLUCENT};
use nexus_gfx::shader::rogue::setup::{fragment, Varyings};

fn push_words(out: &mut Vec<u8>, words: &[u32]) {
    for w in words {
        out.extend_from_slice(&w.to_le_bytes());
    }
}

/// The first-quad render capture's shape (`capture/render1`, `capture/variants/r_2`).
const PDS_STATE: [u32; 3] = [0x2001_0001, 0x0000_28c0, 0x0000_2800];
const VDM_STATE: [u32; 5] = [0x4c00_0f80, 0x0000_2840, 0x0000_2740, 0x0000_0007, 0x2000_0444];
const PPP26: [u32; 26] = [
    0x007f_0ee7,
    0x0005_0000,
    0x022e_0f00,
    0x0e00_0000,
    0,
    0x2900,
    0x2700,
    0x0180_0411,
    0x0100_4000,
    0x2940,
    0x2880,
    0x8000_0077,
    0x0000_0043,
    0x4470_0000,
    0x4470_0000,
    0x4407_0000,
    0x4407_0000,
    0,
    0x3f80_0000,
    0x29e1_2e13,
    0x0705_0000,
    0x0000_0002,
    0,
    0,
    0x0000_06a0,
    0,
];
const VS_PDS_DATA: [u32; 16] = [
    0x0000_7ac0,
    0x8,
    0,
    0x1000_0000,
    0x008d_a000,
    0x80,
    0x2,
    0,
    0,
    0,
    0,
    0x1000_0000,
    0x008d_a008,
    0x80,
    0x8000_4002,
    0,
];
const UNIFORM_DATA: [u32; 12] = [
    0x0004_b080,
    0x0000_0080,
    0x0004_b100,
    0x0000_0080,
    0x0004_f140,
    0x0000_0080,
    0x0004_f180,
    0x0000_0080,
    0x1000_8014,
    0x1003_0004,
    0x1003_8003,
    0x9000_0001,
];

fn bytes(words: &[u32]) -> Vec<u8> {
    let mut b = Vec::new();
    push_words(&mut b, words);
    b
}

struct Blobs {
    ppp26: Vec<u8>,
    vs: Vec<u8>,
}

fn blobs() -> Blobs {
    Blobs { ppp26: bytes(&PPP26), vs: bytes(&VS_PDS_DATA) }
}

fn template(b: &Blobs) -> Template<'_> {
    Template {
        pds_state_words: PDS_STATE,
        vdm_state_words: VDM_STATE,
        ppp26: &b.ppp26,
        terminate_ppp: (0x80_0004_b040, 3),
        vs_pds_data: &b.vs,
        vs_patched: [16, 48],
        texture_va: 0x80_0004_b080,
        general_va: 0x80_0004_b000,
        vertex_va: 0x80_008d_a000,
        stream_va: 0x80_010d_1000,
        cam_size: 32,
        topology: 6,
    }
}

/// The layer pipeline, its setup placed where the capture had the fragment programs.
fn pipeline(pass_type: u8) -> Placed {
    let interface = Interface {
        stage: Stage::Fragment,
        inputs: vec![Input { location: 0, ty: Type::VEC2, interp: Interp::Perspective }],
        outputs: vec![Output { location: 0, ty: Type::VEC4 }],
        resources: vec![
            Resource::Texture { binding: 0, dim: TexDim::D2 },
            Resource::Sampler { binding: 1 },
        ],
        push_bytes: 16,
    };
    let program = Program { groups: Vec::new(), temps: 1, pass_type };
    Placed {
        id: 2,
        name: "layer".into(),
        groups: 11,
        pass_type,
        exe_off: 0x7b80,
        kick: 0x2900,
        iterate: 0x2940,
        state_code: 0x2700,
        setup: fragment(&program, &interface, Layout::BUNDLE, Varyings::BUNDLE).unwrap(),
    }
}

/// The captured 26-word block with the pipeline's pass type and size word.
fn ppp26_with(pass_type: u8) -> Vec<u8> {
    let mut w = PPP26;
    w[2] = ppp::with_pass_type(w[2], pass_type);
    w[7] = ppp::sizeinfo1(2, 0, 1, 1, 1);
    bytes(&w)
}

fn bump() -> Bump {
    Bump {
        general_free: 0x1_0000,
        general_len: 0x2_0000,
        pds_free: 0x3000,
        pds_len: 0x2_0000,
        stream_free: 0x100,
        stream_len: 0x1000,
        vertex_free: 0x200,
        vertex_len: 0x1000,
    }
}

fn quad(x: f32, y: f32, tint: [f32; 4]) -> Quad {
    Quad { x, y, w: 512.0, h: 512.0, tint }
}

/// The captures' placement: the first draw's blocks where the one-quad capture had them,
/// the second draw's where the two-quad capture had them.
struct Captured;
impl Placer for Captured {
    fn place(&mut self, draw: usize, need: Need) -> Option<u32> {
        Some(match (draw, need) {
            (0, Need::Stream(_)) => 0,
            (0, Need::Vertices(_)) => 0,
            (0, Need::VsPdsData) => 0x2840,
            (0, Need::Ppp26) => 0x8200,
            (0, Need::Tint) => 0x100,
            (0, Need::StateData(32)) => 0x2880,
            (1, Need::Tint) => 0x8280,
            (1, Need::Ppp) => 0xc340,
            (1, Need::StateData(32)) => 0x2980,
            _ => return None,
        })
    }
}

#[test]
fn one_quad_at_the_captured_offsets_is_the_capture_but_for_the_pipelines_words() {
    let b = blobs();
    let t = template(&b);
    let e = emit(&t, &pipeline(PASS_TRANSLUCENT), &[quad(704.0, 284.0, [1.0; 4])], &mut Captured)
        .unwrap();
    // The captured 60-byte stream.
    let mut expect = bytes(&PDS_STATE);
    push_words(&mut expect, &[0x0000_1a80, 0x0005_3200]);
    push_words(&mut expect, &VDM_STATE);
    push_words(&mut expect, &[0x6800_0600, 6, 0x0000_0380, 0x0004_b040, 0xc000_0000]);
    assert_eq!(e.stream, expect);
    assert_eq!((e.stream_offset, e.vertex_offset, e.vdm_ctrl_stream_base), (0, 0, 0x80_010d_1000));
    assert_eq!(stream_bytes(1), 60);
    assert_eq!(e.vertices.len(), 96);
    assert_eq!(&e.vertices[0..8], &[0, 0, 0x30, 0x44, 0, 0, 0x8e, 0x43], "704.0, 284.0");
    // The tint at the capture's offset; the 26-word block the capture's but for the pass type
    // and the size word; the program copies, the descriptor and the constant are gone.
    assert_eq!(e.general[0], Write { offset: 0x100, bytes: bytes(&[0x3f80_0000; 4]) });
    assert_eq!(e.general[1], Write { offset: 0x8200, bytes: ppp26_with(PASS_TRANSLUCENT) });
    assert_eq!(e.general.len(), 2);
    assert_eq!(e.pds[0], Write { offset: 0x2840, bytes: b.vs.clone() }, "vertex PDS data");
    // The state load's data: the capture's texture and tint sources and controls, the tint's
    // control now the last.
    let mut data = UNIFORM_DATA[..4].to_vec();
    data.extend_from_slice(&[UNIFORM_DATA[8], UNIFORM_DATA[9] | 0x8000_0000, 0, 0]);
    assert_eq!(e.pds[1], Write { offset: 0x2880, bytes: bytes(&data) }, "state-load data");
    assert_eq!(e.pds.len(), 2);
}

#[test]
fn a_second_quad_at_the_captured_offsets_is_the_two_quad_capture_naming_one_setup() {
    let b = blobs();
    let t = template(&b);
    let q1 = quad(304.0, 134.0, [1.0, 0.5, 0.5, 1.0]);
    let p = pipeline(PASS_OPAQUE);
    let e = emit(&t, &p, &[quad(704.0, 284.0, [1.0; 4]), q1], &mut Captured).unwrap();
    let tail = &e.stream[48..];
    let mut expect = Vec::new();
    push_words(
        &mut expect,
        &[
            0x0000_0780,
            0x0005_7340,
            0x4000_0f80,
            0x6a00_0600,
            6,
            6,
            0x0000_0380,
            0x0004_b040,
            0xc000_0000,
        ],
    );
    assert_eq!(tail, &expect[..], "the captured 24-byte growth and the tail");
    assert_eq!(stream_bytes(2), 84);
    assert_eq!(&e.vertices[96..104], &[0, 0, 0x98, 0x43, 0, 0, 0x06, 0x43], "304.0, 134.0");
    assert_eq!(e.general[1], Write { offset: 0x8200, bytes: ppp26_with(PASS_OPAQUE) });
    // The second draw: its tint as captured, its block naming the same setup programs.
    assert_eq!(
        e.general[2],
        Write {
            offset: 0x8280,
            bytes: bytes(&[0x3f80_0000, 0x3f00_0000, 0x3f00_0000, 0x3f80_0000])
        }
    );
    let sizeinfo1 = ppp::sizeinfo1(2, 0, 1, 1, 1);
    assert_eq!(
        e.general[3],
        Write {
            offset: 0xc340,
            bytes: bytes(&[0x2c0, 0x2900, 0x2700, sizeinfo1, 0x0100_4000, 0x2940, 0x2980])
        }
    );
    assert_eq!(
        e.pds[2],
        Write {
            offset: 0x2980,
            bytes: bytes(&[0x0004_b080, 0x80, 0x0005_3280, 0x80, 0x1000_8014, 0x9003_0004, 0, 0])
        }
    );
    assert_eq!((e.general.len(), e.pds.len()), (4, 3));
}

#[test]
fn the_bump_placer_puts_a_pass_in_fresh_lines() {
    let b = blobs();
    let t = template(&b);
    let qs = [quad(0.0, 0.0, [1.0; 4]), quad(1.0, 1.0, [1.0; 4]), quad(2.0, 2.0, [1.0; 4])];
    let p = pipeline(PASS_TRANSLUCENT);
    let mut place = bump();
    let e = emit(&t, &p, &qs, &mut place).unwrap();
    assert_eq!((e.stream_offset, e.vertex_offset), (0x100, 0x200));
    assert_eq!(e.vdm_ctrl_stream_base, 0x80_010d_1100);
    assert_eq!(e.stream.len(), stream_bytes(3));
    // The vertex PDS data copy names the fresh vertices.
    assert_eq!(pds::doutd_sbase(&e.pds[0].bytes[16..]), 0x80_008d_a200);
    assert_eq!(pds::doutd_sbase(&e.pds[0].bytes[48..]), 0x80_008d_a208);
    // The 26-word block names the pipeline's kick and the first draw's fresh data.
    let ppp26 = &e.general[1];
    assert_eq!(ppp26.bytes.len(), 104);
    assert_eq!(&ppp26.bytes[5 * 4..6 * 4], &0x2900u32.to_le_bytes(), "shaderbase: the kick");
    assert_eq!(&ppp26.bytes[10 * 4..11 * 4], &0x3040u32.to_le_bytes(), "uniformdatabase");
    // Per draw only its state-load data in the PDS heap; a second pass lands behind the first.
    assert_eq!(
        e.pds.iter().map(|w| w.offset).collect::<Vec<_>>(),
        [0x3000, 0x3040, 0x3080, 0x30c0]
    );
    let e2 = emit(&t, &p, &qs, &mut place).unwrap();
    assert_eq!((e2.stream_offset, e2.vertex_offset), (0x100 + 0x80, 0x200 + 0x140));
    assert!(e2.pds.iter().all(|w| w.offset >= 0x3100));
}

#[test]
fn test_reject_overflow_and_empty() {
    let b = blobs();
    let t = template(&b);
    let p = pipeline(PASS_OPAQUE);
    let two = [quad(0.0, 0.0, [1.0; 4]), quad(1.0, 1.0, [1.0; 4])];
    assert_eq!(emit(&t, &p, &[], &mut bump()), Err(EmitError::NoQuads));
    let mut small = Bump { vertex_len: 0x200 + 100, ..bump() };
    assert_eq!(emit(&t, &p, &two, &mut small), Err(EmitError::Space), "vertex buffer");
    let mut short = Bump { stream_len: 0x100 + 64, ..bump() };
    assert_eq!(emit(&t, &p, &two, &mut short), Err(EmitError::Space), "stream buffer");
    let mut tight = Bump { general_len: 0x1_0000 + 128, ..bump() };
    assert_eq!(emit(&t, &p, &two, &mut tight), Err(EmitError::Space), "arena");
    let bad = Template { vs_patched: [16, 60], ..t };
    assert_eq!(emit(&bad, &p, &two[..1], &mut bump()), Err(EmitError::Template));
    let bad = Template { ppp26: &b.ppp26[..100], ..t };
    assert_eq!(emit(&bad, &p, &two[..1], &mut bump()), Err(EmitError::Template));
}
