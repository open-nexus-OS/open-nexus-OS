// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! What the render rungs share (TASK-0215 5b/5c, RFC-0101 §8d): the captured pass's blocks read
//! back from the render bundle's live bindings (the stream's first records, the 26-word PPP
//! block, the vertex PDS data, the end-of-tile program and its pixel event data — the fragment
//! half is the library pipeline's since 5c-2 P4d), the places the pipeline library, the passes'
//! targets (5c-3 P1: an end-of-tile program copy per target, [`Targets`]) and the rungs' passes
//! take in the bundle's buffers, the writes into bound buffers with their cache clean (a pass's
//! blocks through [`PassSink`] as the emitter makes them, nothing allocated), the three render
//! streams of the bundle, and the sample formulas — texel × tint as a premultiplied source over
//! the clear; texel × tint alone for an opaque program.

use nexus_gfx::backend::gpu::eot;
use nexus_gfx::backend::gpu::quads::{Bump, Region, Sink, Template};
use nexus_gfx::backend::gpu::scene::{JobKind, Scene};
use nexus_gfx::backend::v2::{ClientId, ContextId, RenderStreams};

use super::device_v2::Device;
use super::scene_load::Loaded;

/// The render rungs' context: its client, its number, the render bundle loaded into it — made by
/// the render rung, drawn in by the rungs after it, destroyed by the contexts rung (5c-3 P2b).
pub(crate) struct RungContext {
    pub client: ClientId,
    pub ctx: ContextId,
    pub scene: Loaded,
}

/// The captured pass's arenas (`docs/board/measurements/2026-10-13-gpu-render-queue/README.md`, 5b).
pub(super) const GENERAL_ARENA: u64 = 0x80_0004_b000;
pub(super) const PDS_ARENA: u64 = 0xda_0000_1000;
pub(super) const PDS_HEAP_BASE: u64 = 0xda_0000_0000;
pub(super) const VERTEX_BUFFER: u64 = 0x80_008d_a000;
pub(super) const STREAM_BUFFER: u64 = 0x80_010d_1000;
/// The code heap's base and the bundle's code arena (its second page, 128 KiB, the capture's
/// programs in its first 0x6ea8 bytes).
pub(super) const CODE_HEAP_BASE: u64 = 0xe0_0000_0000;
pub(super) const CODE_ARENA: u64 = 0xe0_0000_1000;
/// The pipeline library's places, written once when the context takes its library and never
/// again: the code arena's free tail (arena offsets) and the head of the PDS arena's free lines
/// (heap offsets).
pub(super) const LIBRARY_CODE: (u32, u32) = (0x8000, 0x1_0000);
pub(super) const LIBRARY_PDS: (u32, u32) = (0x3000, 0x3c00);
/// The passes' targets' places (5c-3 P1), each written once when a pass first names its target:
/// the end-of-tile program copies after the library's code (arena offsets), their pixel event
/// data after the library's setup programs (heap offsets) — the quads rung's PDS lines begin
/// after them.
pub(super) const TARGET_CODE: (u32, u32) = (0x1_0000, 0x1_1000);
pub(super) const TARGET_PDS: (u32, u32) = (0x3c00, 0x4000);
/// The bundle's end-of-tile program (code-heap offset) and its pixel event data (PDS-heap
/// offset) — the captured target's.
const EOT_PROGRAM_HEAP: u64 = 0x7e80;
const PIXEL_EVENT_HEAP: u32 = 0x2980;
/// A plane's rows: the captured target spans one.
const PLANE_ROWS: u32 = nexus_display_proto::layout::PLANE_ROWS;
/// The captured first draw's blocks: the vertex PDS data (PDS heap offset), the 26-word PPP
/// block (general arena offset); the texture's descriptor block the captured state load names
/// (texture state, its sampler state at dword 8).
const VS_PDS_DATA_HEAP: u64 = 0x2840;
const PPP26_OFFSET: u32 = 0x8200;
const TEXTURE_DESCRIPTOR: u64 = GENERAL_ARENA + 0x80;
const TERMINATE_PPP: (u64, u32) = (GENERAL_ARENA + 0x40, 3);
/// The bundle's buffers' tails past the captured pass, split between the two rungs that draw
/// in them — every pass in lines no pass has read (§8d): the quads rung's lower halves, the cost
/// rung's upper halves (stream and vertex buffers 4 KiB, the general arena 128 KiB, the PDS
/// arena to heap offset 0x21000). No rung creates a buffer of its own: every buffer is a region
/// of gpud's address space, which holds 64, and four more for the cost rung left none for the
/// display's cursor layer (measurement README, 5c-1 second cycle).
pub(super) const QUADS_TAILS: Bump = Bump {
    general_free: 0x1_0000,
    general_len: 0x1_8000,
    pds_free: TARGET_PDS.1,
    pds_len: 0x1_2000,
    stream_free: 0x100,
    stream_len: 0x800,
    vertex_free: 0x200,
    vertex_len: 0x800,
};
pub(super) const COST_TAILS: Bump = Bump {
    general_free: 0x1_8000,
    general_len: 0x2_0000,
    pds_free: 0x1_2000,
    pds_len: 0x2_1000,
    stream_free: 0x800,
    stream_len: 0x1000,
    vertex_free: 0x800,
    vertex_len: 0x1000,
};
/// The clear colour in memory order (B, G, R).
const CLEAR_BGR: [f32; 3] = [41.0, 31.0, 26.0];
/// The texel 53.125 % into the checker on both axes, (136, 136): colour (40, 90, 200), alpha
/// (136 + 136) × 255 / 510 = 136 — inside block (4, 4), clear of the bilinear mix at a block
/// boundary (5b's quad centres sat on one).
pub(super) const SAMPLE_FRACTION: f32 = 136.0 / 256.0;
const SAMPLE_TEXEL: [f32; 4] = [40.0, 90.0, 200.0, 136.0];

/// The expected memory bytes (B, G, R) at a sample of an opaque program: texel × tint.
pub(super) fn opaque_bgr(tint: [f32; 4]) -> [u8; 3] {
    let c = |i: usize| (SAMPLE_TEXEL[i] * tint[i] + 0.5) as u8;
    [c(2), c(1), c(0)]
}

/// The expected memory bytes (B, G, R) at a sample of a premultiplied-over program: the
/// texel × tint as a premultiplied source over the clear, `src + dst·(1 − a)`.
pub(super) fn premultiplied_bgr(tint: [f32; 4]) -> [u8; 3] {
    let a = SAMPLE_TEXEL[3] / 255.0 * tint[3];
    let src = |i: usize| SAMPLE_TEXEL[i] * tint[i];
    let mix = |s: f32, d: f32| (s + d * (1.0 - a) + 0.5) as u8;
    [mix(src(2), CLEAR_BGR[0]), mix(src(1), CLEAR_BGR[1]), mix(src(0), CLEAR_BGR[2])]
}

/// The clear's bytes (B, G, R): a pixel inside a pass's area that no draw covered.
pub(super) fn clear_bgr() -> [u8; 3] {
    CLEAR_BGR.map(|c| c as u8)
}

/// The expected bytes at a sample of a straight-alpha layer: the texel × tint premultiplied by
/// its own alpha, over the clear.
pub(super) fn straight_bgr(tint: [f32; 4]) -> [u8; 3] {
    let a = SAMPLE_TEXEL[3] / 255.0 * tint[3];
    let src = |i: usize| SAMPLE_TEXEL[i] * tint[i] * a;
    let mix = |s: f32, d: f32| (s + d * (1.0 - a) + 0.5) as u8;
    [mix(src(2), CLEAR_BGR[0]), mix(src(1), CLEAR_BGR[1]), mix(src(0), CLEAR_BGR[2])]
}

/// The expected bytes of a straight colour (0..1) at alpha `a` over the clear.
pub(super) fn colour_over_clear_bgr(colour: [f32; 4], a: f32) -> [u8; 3] {
    let mix = |s: f32, d: f32| (s * 255.0 * a + d * (1.0 - a) + 0.5) as u8;
    [mix(colour[2], CLEAR_BGR[0]), mix(colour[1], CLEAR_BGR[1]), mix(colour[0], CLEAR_BGR[2])]
}

/// Copies `bytes` into the memory bound at `va + at` in the context and cleans it (CPU →
/// device).
pub(super) fn write_bound(
    dev: &mut Device,
    client: ClientId,
    ctx: ContextId,
    va: u64,
    at: u32,
    bytes: &[u8],
) -> Result<(), alloc::string::String> {
    let addr = va + u64::from(at);
    let dst = dev
        .bound_bytes_mut(client, ctx, addr, bytes.len())
        .ok_or_else(|| alloc::format!("{va:#x}+{at:#x} not bound to the cpu"))?;
    dst.copy_from_slice(bytes);
    dev.clean_bound(client, ctx, addr, bytes.len());
    Ok(())
}

/// A pass's sink: each block written into its bound buffer and cleaned as the emitter makes it;
/// the first refusal kept for the rung's line.
pub(super) struct PassSink<'a> {
    pub dev: &'a mut Device,
    pub client: ClientId,
    pub ctx: ContextId,
    pub refused: Option<alloc::string::String>,
}

impl Sink for PassSink<'_> {
    fn write(&mut self, region: Region, offset: u32, bytes: &[u8]) -> bool {
        let (va, at) = match region {
            Region::General => (GENERAL_ARENA, offset),
            Region::Pds => (PDS_ARENA, pds_at(PDS_ARENA, offset)),
            Region::Stream => (STREAM_BUFFER, offset),
            Region::Vertex => (VERTEX_BUFFER, offset),
        };
        match write_bound(self.dev, self.client, self.ctx, va, at, bytes) {
            Ok(()) => true,
            Err(e) => {
                self.refused.get_or_insert(e);
                false
            }
        }
    }
}

pub(super) fn read_bound(
    dev: &mut Device,
    client: ClientId,
    ctx: ContextId,
    va: u64,
    at: u32,
    len: usize,
) -> Option<alloc::vec::Vec<u8>> {
    dev.bound_bytes_mut(client, ctx, va + u64::from(at), len).map(|s| s.to_vec())
}

/// A PDS heap offset as an offset into the PDS buffer bound at `bo_va`.
pub(super) fn pds_at(bo_va: u64, heap_offset: u32) -> u32 {
    (PDS_HEAP_BASE + u64::from(heap_offset) - bo_va) as u32
}

/// The bundle's three render streams with their submit flags.
pub(super) fn render_streams<'a>(scene: &Scene<'a>) -> Option<RenderStreams<'a>> {
    let (mut geom, mut frag_pr, mut frag) = (None, None, None);
    for job in scene.jobs() {
        match job.kind {
            JobKind::Geometry => geom = Some(job),
            JobKind::FragmentPartial => frag_pr = Some(job),
            JobKind::Fragment => frag = Some(job),
            JobKind::Transfer => {}
        }
    }
    let (g, pr, f) = (geom?, frag_pr?, frag?);
    Some(RenderStreams {
        geom: g.stream,
        geom_flags: g.flags,
        frag_pr: pr.stream,
        frag_pr_flags: pr.flags,
        frag: f.stream,
        frag_flags: f.flags,
    })
}

/// A pass's target: its address, the rows it spans from there (a plane's stride each — what a
/// pass may write must be bound), the PDS-heap offset of the pixel event data naming its
/// end-of-tile program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Target {
    pub va: u64,
    pub rows: u32,
    pub pixel_event: u32,
}

/// The targets passes have named, each with its end-of-tile program copy placed once.
pub(super) struct Targets {
    placed: [Option<Target>; MAX_TARGETS],
    code_free: u32,
    pds_free: u32,
}

/// Targets a context's passes name (the ring's slots, the frame's scratch and cache rows).
const MAX_TARGETS: usize = 16;

impl Targets {
    pub(super) const fn new() -> Self {
        Self { placed: [None; MAX_TARGETS], code_free: TARGET_CODE.0, pds_free: TARGET_PDS.0 }
    }

    /// The target at `va`: its end-of-tile program — the captured one retargeted — and the
    /// pixel event data naming it written the first time a pass names it, never again.
    pub(super) fn at(
        &mut self,
        dev: &mut Device,
        (client, ctx): (ClientId, ContextId),
        c: &Captured,
        va: u64,
        rows: u32,
    ) -> Result<Target, alloc::string::String> {
        if let Some(t) = self.placed.iter().flatten().find(|t| t.va == va) {
            return Ok(Target { rows, ..*t });
        }
        let slot = self.placed.iter().position(Option::is_none).ok_or("no target slot left")?;
        let program = eot::retarget(&c.eot, va).map_err(|e| alloc::format!("end-of-tile {e:?}"))?;
        let code_at = self.code_free;
        let pds_at_heap = self.pds_free;
        if code_at + 64 > TARGET_CODE.1 || pds_at_heap + 16 > TARGET_PDS.1 {
            return Err(alloc::string::String::from("no place for a target"));
        }
        let code_heap = (CODE_ARENA - CODE_HEAP_BASE) as u32 + code_at;
        let data = eot::data(&c.pixel_event, code_heap).map_err(|e| alloc::format!("{e:?}"))?;
        write_bound(dev, client, ctx, CODE_ARENA, code_at, &program)?;
        write_bound(dev, client, ctx, PDS_ARENA, pds_at(PDS_ARENA, pds_at_heap), &data)?;
        (self.code_free, self.pds_free) = (code_at + 64, pds_at_heap + 16);
        let t = Target { va, rows, pixel_event: pds_at_heap };
        self.placed[slot] = Some(t);
        Ok(t)
    }
}

/// The captured first draw's vertex half and blocks, and the captured end-of-tile program and
/// its pixel event data, read back from the render bundle's bindings.
pub(super) struct Captured {
    pds_state: [u32; 3],
    vdm_state: [u32; 5],
    ppp26: alloc::vec::Vec<u8>,
    vs: alloc::vec::Vec<u8>,
    eot: alloc::vec::Vec<u8>,
    pixel_event: alloc::vec::Vec<u8>,
    /// The address the captured end-of-tile program writes.
    eot_target: u64,
}

impl Captured {
    pub(super) fn read(
        dev: &mut Device,
        client: ClientId,
        ctx: ContextId,
    ) -> Result<Self, &'static str> {
        let stream0 = read_bound(dev, client, ctx, STREAM_BUFFER, 0, 0x30)
            .ok_or("the stream buffer is not bound (the render rung did not run)")?;
        let w: alloc::vec::Vec<u32> =
            stream0.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        let vs_at = pds_at(PDS_ARENA, VS_PDS_DATA_HEAP as u32);
        let vs = read_bound(dev, client, ctx, PDS_ARENA, vs_at, 64)
            .ok_or("the PDS arena is not bound")?;
        let ppp26 = read_bound(dev, client, ctx, GENERAL_ARENA, PPP26_OFFSET, 104)
            .ok_or("the general arena is not bound")?;
        let eot_at = (CODE_HEAP_BASE + EOT_PROGRAM_HEAP - CODE_ARENA) as u32;
        let eot = read_bound(dev, client, ctx, CODE_ARENA, eot_at, eot::PROGRAM_BYTES)
            .ok_or("the code arena is not bound")?;
        let pixel_event =
            read_bound(dev, client, ctx, PDS_ARENA, pds_at(PDS_ARENA, PIXEL_EVENT_HEAP), 8)
                .ok_or("the PDS arena is not bound")?;
        let eot_target =
            eot::target(&eot).map_err(|_| "the end-of-tile program is not the captured one")?;
        Ok(Self {
            pds_state: [w[0], w[1], w[2]],
            vdm_state: [w[5], w[6], w[7], w[8], w[9]],
            ppp26,
            vs,
            eot,
            pixel_event,
            eot_target,
        })
    }

    /// The captured target: the bundle's own end-of-tile program and its pixel event data — the
    /// plane the rungs bind at the captured address.
    pub(super) fn target(&self) -> Target {
        Target { va: self.eot_target, rows: PLANE_ROWS, pixel_event: PIXEL_EVENT_HEAP }
    }

    /// The emitter's template, its per-pass blocks going to the buffers at the three addresses.
    pub(super) fn template(&self, general_va: u64, vertex_va: u64, stream_va: u64) -> Template<'_> {
        Template {
            pds_state_words: self.pds_state,
            vdm_state_words: self.vdm_state,
            ppp26: &self.ppp26,
            terminate_ppp: TERMINATE_PPP,
            vs_pds_data: &self.vs,
            vs_patched: [16, 48],
            texture_va: TEXTURE_DESCRIPTOR,
            general_va,
            vertex_va,
            stream_va,
            cam_size: 32,
            topology: 6,
        }
    }
}
