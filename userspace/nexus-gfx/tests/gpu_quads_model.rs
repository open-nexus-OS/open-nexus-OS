// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The quad emitter's goldens (TASK-0215 5b, RFC-0101 §8d; drawn with a library pipeline since
//! 5c-2 P4d): one quad at the capture's offsets is the one-quad capture byte for byte but for the
//! pipeline's own words — the pass type, the setup's size word, the state load's two loads where
//! the capture had four — and a second quad the two-quad capture's growth and blocks, both draws
//! naming the pipeline's setup programs placed once; the bump placer puts every pass in fresh
//! lines (H10); overflow and malformed templates are refused. The captured words:
//! `docs/board/measurements/2026-10-13-gpu-render-queue/capture/` (`render1`, `variants/r_2`).

use std::alloc::{GlobalAlloc, Layout as AllocLayout, System};
use std::cell::Cell;

use nexus_gfx::backend::gpu::csb::{pds, ppp};
use nexus_gfx::backend::gpu::pipelines::Placed;
use nexus_gfx::backend::gpu::quads::*;
use nexus_gfx::shader::ir::{Input, Interface, Interp, Output, Resource, Stage, TexDim, Type};
use nexus_gfx::shader::rogue::codegen::{Layout, Program};
use nexus_gfx::shader::rogue::reference::{PASS_OPAQUE, PASS_TRANSLUCENT};
use nexus_gfx::shader::rogue::setup::{fragment, Varyings};

thread_local! {
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}

/// Counts this thread's allocations (tests run on their own threads).
struct Counting;

// SAFETY: every method forwards to `System`, which upholds the GlobalAlloc contract; the only
// addition is a non-allocating counter bump.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: AllocLayout) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: AllocLayout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: AllocLayout, new_size: usize) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

fn allocs() -> usize {
    ALLOCS.try_with(Cell::get).unwrap_or(0)
}

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
    let program = Program { groups: Vec::new(), temps: 1, pass_type, constants: Vec::new() };
    Placed {
        id: 2,
        name: "layer".into(),
        groups: 11,
        pass_type,
        exe_off: 0x7b80,
        kick: 0x2900,
        iterate: 0x2940,
        state_code: 0x2700,
        constants: None,
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
    Quad::layer(x, y, 512.0, 512.0, tint, 0.0)
}

/// The pass emitted into a recorder: what `emit` returned and every block it wrote.
fn emit_recorded(
    t: &Template<'_>,
    p: &Placed,
    qs: &[Quad],
    place: &mut dyn Placer,
) -> Result<(Emitted, Recorder), EmitError> {
    let mut r = Recorder::default();
    emit(t, p, qs, place, &mut r).map(|e| (e, r))
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
            (0, Need::Push(16)) => 0x100,
            (0, Need::StateData(32)) => 0x2880,
            (1, Need::Push(16)) => 0x8280,
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
    let (e, r) = emit_recorded(
        &t,
        &pipeline(PASS_TRANSLUCENT),
        &[quad(704.0, 284.0, [1.0; 4])],
        &mut Captured,
    )
    .unwrap();
    // The captured 60-byte stream.
    let mut expect = bytes(&PDS_STATE);
    push_words(&mut expect, &[0x0000_1a80, 0x0005_3200]);
    push_words(&mut expect, &VDM_STATE);
    push_words(&mut expect, &[0x6800_0600, 6, 0x0000_0380, 0x0004_b040, 0xc000_0000]);
    assert_eq!(r.stream, expect);
    assert_eq!((e.stream_offset, e.vertex_offset, e.vdm_ctrl_stream_base), (0, 0, 0x80_010d_1000));
    assert_eq!(stream_bytes(1), 60);
    assert_eq!(r.vertices.len(), 96);
    assert_eq!(&r.vertices[0..8], &[0, 0, 0x30, 0x44, 0, 0, 0x8e, 0x43], "704.0, 284.0");
    // The tint at the capture's offset; the 26-word block the capture's but for the pass type
    // and the size word; the program copies, the descriptor and the constant are gone.
    assert_eq!(r.general[0], Write { offset: 0x100, bytes: bytes(&[0x3f80_0000; 4]) });
    assert_eq!(r.general[1], Write { offset: 0x8200, bytes: ppp26_with(PASS_TRANSLUCENT) });
    assert_eq!(r.general.len(), 2);
    assert_eq!(r.pds[0], Write { offset: 0x2840, bytes: b.vs.clone() }, "vertex PDS data");
    // The state load's data: the capture's texture and tint sources and controls, the tint's
    // control now the last.
    let mut data = UNIFORM_DATA[..4].to_vec();
    data.extend_from_slice(&[UNIFORM_DATA[8], UNIFORM_DATA[9] | 0x8000_0000, 0, 0]);
    assert_eq!(r.pds[1], Write { offset: 0x2880, bytes: bytes(&data) }, "state-load data");
    assert_eq!(r.pds.len(), 2);
}

#[test]
fn a_second_quad_at_the_captured_offsets_is_the_two_quad_capture_naming_one_setup() {
    let b = blobs();
    let t = template(&b);
    let q1 = quad(304.0, 134.0, [1.0, 0.5, 0.5, 1.0]);
    let p = pipeline(PASS_OPAQUE);
    let (_, r) = emit_recorded(&t, &p, &[quad(704.0, 284.0, [1.0; 4]), q1], &mut Captured).unwrap();
    let tail = &r.stream[48..];
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
    assert_eq!(&r.vertices[96..104], &[0, 0, 0x98, 0x43, 0, 0, 0x06, 0x43], "304.0, 134.0");
    assert_eq!(r.general[1], Write { offset: 0x8200, bytes: ppp26_with(PASS_OPAQUE) });
    // The second draw: its tint as captured, its block naming the same setup programs.
    assert_eq!(
        r.general[2],
        Write {
            offset: 0x8280,
            bytes: bytes(&[0x3f80_0000, 0x3f00_0000, 0x3f00_0000, 0x3f80_0000])
        }
    );
    let sizeinfo1 = ppp::sizeinfo1(2, 0, 1, 1, 1);
    assert_eq!(
        r.general[3],
        Write {
            offset: 0xc340,
            bytes: bytes(&[0x2c0, 0x2900, 0x2700, sizeinfo1, 0x0100_4000, 0x2940, 0x2980])
        }
    );
    assert_eq!(
        r.pds[2],
        Write {
            offset: 0x2980,
            bytes: bytes(&[0x0004_b080, 0x80, 0x0005_3280, 0x80, 0x1000_8014, 0x9003_0004, 0, 0])
        }
    );
    assert_eq!((r.general.len(), r.pds.len()), (4, 3));
}

#[test]
fn the_bump_placer_puts_a_pass_in_fresh_lines() {
    let b = blobs();
    let t = template(&b);
    let qs = [quad(0.0, 0.0, [1.0; 4]), quad(1.0, 1.0, [1.0; 4]), quad(2.0, 2.0, [1.0; 4])];
    let p = pipeline(PASS_TRANSLUCENT);
    let mut place = bump();
    let (e, r) = emit_recorded(&t, &p, &qs, &mut place).unwrap();
    assert_eq!((e.stream_offset, e.vertex_offset), (0x100, 0x200));
    assert_eq!(e.vdm_ctrl_stream_base, 0x80_010d_1100);
    assert_eq!(r.stream.len(), stream_bytes(3));
    // The vertex PDS data copy names the fresh vertices.
    assert_eq!(pds::doutd_sbase(&r.pds[0].bytes[16..]), 0x80_008d_a200);
    assert_eq!(pds::doutd_sbase(&r.pds[0].bytes[48..]), 0x80_008d_a208);
    // The 26-word block names the pipeline's kick and the first draw's fresh data.
    let ppp26 = &r.general[1];
    assert_eq!(ppp26.bytes.len(), 104);
    assert_eq!(&ppp26.bytes[5 * 4..6 * 4], &0x2900u32.to_le_bytes(), "shaderbase: the kick");
    assert_eq!(&ppp26.bytes[10 * 4..11 * 4], &0x3040u32.to_le_bytes(), "uniformdatabase");
    // Per draw only its state-load data in the PDS heap; a second pass lands behind the first.
    assert_eq!(
        r.pds.iter().map(|w| w.offset).collect::<Vec<_>>(),
        [0x3000, 0x3040, 0x3080, 0x30c0]
    );
    let (e2, r2) = emit_recorded(&t, &p, &qs, &mut place).unwrap();
    assert_eq!((e2.stream_offset, e2.vertex_offset), (0x100 + 0x80, 0x200 + 0x140));
    assert!(r2.pds.iter().all(|w| w.offset >= 0x3100));
}

#[test]
fn test_reject_overflow_and_empty() {
    let b = blobs();
    let t = template(&b);
    let p = pipeline(PASS_OPAQUE);
    let two = [quad(0.0, 0.0, [1.0; 4]), quad(1.0, 1.0, [1.0; 4])];
    assert_eq!(emit_recorded(&t, &p, &[], &mut bump()).err(), Some(EmitError::NoQuads));
    let mut small = Bump { vertex_len: 0x200 + 100, ..bump() };
    assert_eq!(
        emit_recorded(&t, &p, &two, &mut small).err(),
        Some(EmitError::Space),
        "vertex buffer"
    );
    let mut short = Bump { stream_len: 0x100 + 64, ..bump() };
    assert_eq!(
        emit_recorded(&t, &p, &two, &mut short).err(),
        Some(EmitError::Space),
        "stream buffer"
    );
    let mut tight = Bump { general_len: 0x1_0000 + 128, ..bump() };
    assert_eq!(emit_recorded(&t, &p, &two, &mut tight).err(), Some(EmitError::Space), "arena");
    let bad = Template { vs_patched: [16, 60], ..t };
    assert_eq!(emit_recorded(&bad, &p, &two[..1], &mut bump()).err(), Some(EmitError::Template));
    let bad = Template { ppp26: &b.ppp26[..100], ..t };
    assert_eq!(emit_recorded(&bad, &p, &two[..1], &mut bump()).err(), Some(EmitError::Template));
}

/// The layer pipeline with a constant block of two values, placed at `constants`.
fn weighted(constants: Option<u32>) -> Placed {
    let mut p = pipeline(PASS_TRANSLUCENT);
    let program = Program {
        groups: Vec::new(),
        temps: 1,
        pass_type: PASS_TRANSLUCENT,
        constants: vec![0.7f32.to_bits(), 0.3f32.to_bits()],
    };
    let i = Interface {
        stage: Stage::Fragment,
        inputs: vec![Input { location: 0, ty: Type::VEC2, interp: Interp::Perspective }],
        outputs: vec![Output { location: 0, ty: Type::VEC4 }],
        resources: vec![
            Resource::Texture { binding: 0, dim: TexDim::D2 },
            Resource::Sampler { binding: 1 },
        ],
        push_bytes: 16,
    };
    p.setup = fragment(&program, &i, Layout::BUNDLE, Varyings::BUNDLE).unwrap();
    p.constants = constants;
    p
}

#[test]
fn a_draws_state_load_reads_the_constant_block_where_the_table_placed_it() {
    let b = blobs();
    let t = template(&b);
    let p = weighted(Some(0x2a00));
    let (_, r) = emit_recorded(&t, &p, &[quad(0.0, 0.0, [1.0; 4])], &mut bump()).unwrap();
    // Three loads: three sources, three controls (twelve dwords); the third source the block in
    // the PDS heap, nothing written for it per pass.
    let data = &r.pds[1].bytes;
    assert_eq!(data.len(), 48);
    let word = |i: usize| {
        u32::from_le_bytes([data[4 * i], data[4 * i + 1], data[4 * i + 2], data[4 * i + 3]])
    };
    assert_eq!(u64::from(word(4)) | (u64::from(word(5)) << 32), 0xda_0000_2a00);
    assert_eq!(r.general.len(), 2, "the push constants and the 26-word block");
}

#[test]
fn test_reject_a_constant_block_the_table_did_not_place() {
    let b = blobs();
    let t = template(&b);
    let e = emit_recorded(&t, &weighted(None), &[quad(0.0, 0.0, [1.0; 4])], &mut bump());
    assert_eq!(e.err(), Some(EmitError::Pipeline));
}

#[test]
fn a_pass_descriptor_takes_fresh_lines_of_the_general_arena() {
    // Twenty dwords: two 64-byte lines, behind whatever the pass placed before.
    let mut place = bump();
    let first = place.place(0, Need::Descriptor).unwrap();
    let second = place.place(0, Need::Descriptor).unwrap();
    assert_eq!((first, second), (0x1_0000, 0x1_0080));
    let mut full = Bump { general_len: 0x1_0000 + 64, ..bump() };
    assert_eq!(full.place(0, Need::Descriptor), None);
}

/// A sink that only counts what it is given.
struct Tally(usize);

impl Sink for Tally {
    fn write(&mut self, _: Region, _: u32, bytes: &[u8]) -> bool {
        self.0 += bytes.len();
        true
    }
}

#[test]
fn a_pass_is_emitted_without_allocating() {
    // gpud's heap never frees and a frame emits passes for its whole life: three quads with the
    // pipeline's setup, every block made in place and handed to the sink, not one allocation.
    let b = blobs();
    let t = template(&b);
    let p = weighted(Some(0x2a00));
    let qs = [quad(0.0, 0.0, [1.0; 4]), quad(1.0, 1.0, [1.0; 4]), quad(2.0, 2.0, [1.0; 4])];
    let (mut place, mut tally) = (bump(), Tally(0));
    let before = allocs();
    let e = emit(&t, &p, &qs, &mut place, &mut tally).unwrap();
    assert_eq!(allocs() - before, 0, "allocations while emitting");
    assert_eq!(e.draws, 3);
    // Everything the recorder sees went through the tally too.
    let (_, r) = emit_recorded(&t, &p, &qs, &mut bump()).unwrap();
    let recorded = r.vertices.len()
        + r.stream.len()
        + r.general.iter().chain(&r.pds).map(|w| w.bytes.len()).sum::<usize>();
    assert_eq!(tally.0, recorded);
}
