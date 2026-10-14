// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The quad emitter's goldens (TASK-0215 5b, RFC-0101 §8d): one quad at the capture's offsets
//! is the one-quad capture byte for byte, a second quad the two-quad capture's growth and
//! blocks; the bump placer puts every pass in fresh lines (H10); overflow and malformed
//! templates are refused. The captured words: `docs/board/measurements/2026-10-13-gpu-render-
//! queue/capture/` (`render1`, `variants/r_2`).

use nexus_gfx::backend::gpu::csb::{pds, ppp};
use nexus_gfx::backend::gpu::quads::*;

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
const CONSTANT_BLOCK: [u8; 16] = [0xfe, 0xff, 0xff, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
const SHADER_PROGRAM: [u32; 16] =
    [0x0000_7b80, 0x0000_0028, 0, 0, 0, 0, 0, 0, 0xf400_0002, 0, 0, 0, 0, 0, 0, 0];
const VARYING_PROGRAM: [u32; 16] = [
    0x0300_0000,
    0,
    0x0305_0202,
    0x8040_0000,
    0xf000_0004,
    0xf400_0104,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
    0,
];

fn bytes(words: &[u32]) -> Vec<u8> {
    let mut b = Vec::new();
    push_words(&mut b, words);
    b
}

struct Blobs {
    ppp26: Vec<u8>,
    vs: Vec<u8>,
    uniform: Vec<u8>,
    shader: Vec<u8>,
    varying: Vec<u8>,
}

fn blobs() -> Blobs {
    Blobs {
        ppp26: bytes(&PPP26),
        vs: bytes(&VS_PDS_DATA),
        uniform: bytes(&UNIFORM_DATA),
        shader: bytes(&SHADER_PROGRAM),
        varying: bytes(&VARYING_PROGRAM),
    }
}

fn template(b: &Blobs) -> Template<'_> {
    Template {
        pds_state_words: PDS_STATE,
        vdm_state_words: VDM_STATE,
        ppp26: &b.ppp26,
        terminate_ppp: (0x80_0004_b040, 3),
        vs_pds_data: &b.vs,
        vs_patched: [16, 48],
        uniform_data: &b.uniform,
        patched: [8, 16, 24],
        constant_block: &CONSTANT_BLOCK,
        shader_program: &b.shader,
        varying_program: &b.varying,
        general_va: 0x80_0004_b000,
        vertex_va: 0x80_008d_a000,
        stream_va: 0x80_010d_1000,
        pds: ppp::PdsState {
            shaderbase: 0x2900,
            texunicodebase: 0x2700,
            varyingbase: 0x2940,
            uniformdatabase: 0x2880,
            sizeinfo1: 0x0180_0411,
            sizeinfo2: 0x0100_4000,
        },
        cam_size: 32,
        topology: 6,
    }
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
            (0, Need::Zero) => 0x140,
            (0, Need::Descriptor) => 0x4140,
            (0, Need::Constant) => 0x4180,
            (0, Need::Uniform) => 0x2880,
            (0, Need::Shader) => 0x2900,
            (0, Need::Varying) => 0x2940,
            (1, Need::Tint) => 0x8280,
            (1, Need::Zero) => 0x82c0,
            (1, Need::Descriptor) => 0xc2c0,
            (1, Need::Constant) => 0xc300,
            (1, Need::Ppp) => 0xc340,
            (1, Need::Uniform) => 0x2980,
            (1, Need::Shader) => 0x29c0,
            (1, Need::Varying) => 0x2a00,
            _ => return None,
        })
    }
}

#[test]
fn one_quad_at_the_captured_offsets_is_the_capture_byte_for_byte() {
    let b = blobs();
    let t = template(&b);
    let e = emit(&t, &[quad(704.0, 284.0, [1.0; 4])], &mut Captured, false).unwrap();
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
    // Every block equals the capture's at the capture's offset: the patched words are the
    // captured values.
    assert_eq!(e.general[0], Write { offset: 0x100, bytes: bytes(&[0x3f80_0000; 4]) });
    assert_eq!(e.general[1], Write { offset: 0x140, bytes: vec![0u8; 16] });
    assert_eq!(e.general[2], Write { offset: 0x4140, bytes: bytes(&[0x0004_b140, 0x80, 4, 0]) });
    assert_eq!(e.general[3], Write { offset: 0x4180, bytes: CONSTANT_BLOCK.to_vec() });
    assert_eq!(e.general[4], Write { offset: 0x8200, bytes: b.ppp26.clone() });
    assert_eq!(e.general.len(), 5);
    assert_eq!(e.pds[0], Write { offset: 0x2840, bytes: b.vs.clone() }, "vertex PDS data");
    assert_eq!(e.pds[1], Write { offset: 0x2880, bytes: b.uniform.clone() }, "uniform data");
    assert_eq!(e.pds[2], Write { offset: 0x2900, bytes: b.shader.clone() });
    assert_eq!(e.pds[3], Write { offset: 0x2940, bytes: b.varying.clone() });
    assert_eq!(e.pds.len(), 4);
}

#[test]
fn a_second_quad_at_the_captured_offsets_is_the_two_quad_capture() {
    let b = blobs();
    let t = template(&b);
    let q1 = quad(304.0, 134.0, [1.0, 0.5, 0.5, 1.0]);
    let e = emit(&t, &[quad(704.0, 284.0, [1.0; 4]), q1], &mut Captured, false).unwrap();
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
    // The second draw's blocks, byte for byte the two-quad capture's.
    assert_eq!(
        e.general[5],
        Write {
            offset: 0x8280,
            bytes: bytes(&[0x3f80_0000, 0x3f00_0000, 0x3f00_0000, 0x3f80_0000])
        }
    );
    assert_eq!(e.general[6], Write { offset: 0x82c0, bytes: vec![0u8; 16] });
    assert_eq!(e.general[7], Write { offset: 0xc2c0, bytes: bytes(&[0x0005_32c0, 0x80, 4, 0]) });
    assert_eq!(e.general[8], Write { offset: 0xc300, bytes: CONSTANT_BLOCK.to_vec() });
    assert_eq!(
        e.general[9],
        Write {
            offset: 0xc340,
            bytes: bytes(&[0x2c0, 0x29c0, 0x2700, 0x0180_0411, 0x0100_4000, 0x2a00, 0x2980])
        }
    );
    assert_eq!(
        e.pds[4],
        Write {
            offset: 0x2980,
            bytes: bytes(&[
                0x0004_b080,
                0x80,
                0x0005_3280,
                0x80,
                0x0005_72c0,
                0x80,
                0x0005_7300,
                0x80,
                0x1000_8014,
                0x1003_0004,
                0x1003_8003,
                0x9000_0001
            ])
        }
    );
    assert_eq!(e.pds[5], Write { offset: 0x29c0, bytes: b.shader.clone() });
    assert_eq!(e.pds[6], Write { offset: 0x2a00, bytes: b.varying.clone() });
    assert_eq!(e.pds.len(), 7);
}

#[test]
fn the_bump_placer_puts_a_pass_in_fresh_lines() {
    let b = blobs();
    let t = template(&b);
    let qs = [quad(0.0, 0.0, [1.0; 4]), quad(1.0, 1.0, [1.0; 4]), quad(2.0, 2.0, [1.0; 4])];
    let mut place = bump();
    let e = emit(&t, &qs, &mut place, true).unwrap();
    assert_eq!((e.stream_offset, e.vertex_offset), (0x100, 0x200));
    assert_eq!(e.vdm_ctrl_stream_base, 0x80_010d_1100);
    assert_eq!(e.stream.len(), stream_bytes(3));
    // The vertex PDS data copy names the fresh vertices.
    assert_eq!(pds::doutd_sbase(&e.pds[0].bytes[16..]), 0x80_008d_a200);
    assert_eq!(pds::doutd_sbase(&e.pds[0].bytes[48..]), 0x80_008d_a208);
    // The 26-word block names the fresh copies.
    let ppp26 = &e.general[4];
    assert_eq!(ppp26.bytes.len(), 104);
    assert_eq!(&ppp26.bytes[5 * 4..6 * 4], &0x3080u32.to_le_bytes(), "shaderbase: the first copy");
    assert_eq!(&ppp26.bytes[10 * 4..11 * 4], &0x3040u32.to_le_bytes(), "uniformdatabase");
    // Shared programs: one copy; a second pass lands behind the first.
    assert_eq!(
        e.pds.iter().map(|w| w.offset).collect::<Vec<_>>(),
        [0x3000, 0x3040, 0x3080, 0x30c0, 0x3100, 0x3140]
    );
    let e2 = emit(&t, &qs, &mut place, true).unwrap();
    assert_eq!((e2.stream_offset, e2.vertex_offset), (0x100 + 0x80, 0x200 + 0x140));
    assert!(e2.pds.iter().all(|w| w.offset >= 0x3180));
}

#[test]
fn test_reject_overflow_and_empty() {
    let b = blobs();
    let t = template(&b);
    let two = [quad(0.0, 0.0, [1.0; 4]), quad(1.0, 1.0, [1.0; 4])];
    assert_eq!(emit(&t, &[], &mut bump(), true), Err(EmitError::NoQuads));
    let mut small = Bump { vertex_len: 0x200 + 100, ..bump() };
    assert_eq!(emit(&t, &two, &mut small, true), Err(EmitError::Space), "vertex buffer");
    let mut short = Bump { stream_len: 0x100 + 64, ..bump() };
    assert_eq!(emit(&t, &two, &mut short, true), Err(EmitError::Space), "stream buffer");
    let mut tight = Bump { general_len: 0x1_0000 + 256, ..bump() };
    assert_eq!(emit(&t, &two, &mut tight, true), Err(EmitError::Space), "arena");
    let bad = Template { patched: [8, 16, 44], ..t };
    assert_eq!(emit(&bad, &two[..1], &mut bump(), true), Err(EmitError::Template));
    let bad = Template { ppp26: &b.ppp26[..100], ..t };
    assert_eq!(emit(&bad, &two[..1], &mut bump(), true), Err(EmitError::Template));
}
