// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! What the render rungs share (TASK-0215 5b/5c, RFC-0101 §8d): the captured pass's blocks read
//! back from the render bundle's live bindings (the stream's first records, the 26-word PPP
//! block, the vertex PDS data — the fragment half is the library pipeline's since 5c-2 P4d), the
//! places the pipeline library and the rungs' passes take in the bundle's buffers, the writes
//! into bound buffers with their cache clean (a pass's blocks through [`PassSink`] as the emitter
//! makes them, nothing allocated), the three render streams of the bundle, and the
//! sample formulas — texel × tint as a premultiplied source over the clear; texel × tint alone
//! for an opaque program.

use nexus_gfx::backend::gpu::quads::{Bump, Region, Sink, Template};
use nexus_gfx::backend::gpu::scene::{JobKind, Scene};
use nexus_gfx::backend::v2::{ClientId, RenderStreams};

use super::device_v2::Device;

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
/// (heap offsets) — the quads rung's PDS lines begin after them.
pub(super) const LIBRARY_CODE: (u32, u32) = (0x8000, 0x1_0000);
pub(super) const LIBRARY_PDS: (u32, u32) = (0x3000, 0x4000);
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
    pds_free: LIBRARY_PDS.1,
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

/// Copies `bytes` into the bound buffer at `va + at` and cleans it (CPU → device).
pub(super) fn write_bound(
    dev: &mut Device,
    client: ClientId,
    va: u64,
    at: u32,
    bytes: &[u8],
) -> Result<(), alloc::string::String> {
    let bo = dev
        .bo_at(va, u64::from(at) + bytes.len() as u64)
        .ok_or_else(|| alloc::format!("{va:#x}+{at:#x} not bound"))?;
    let view = dev.bo_bytes_mut(client, bo).ok_or("no cpu view")?;
    let dst =
        view.get_mut(at as usize..at as usize + bytes.len()).ok_or("write past the buffer")?;
    dst.copy_from_slice(bytes);
    dev.clean(client, bo);
    Ok(())
}

/// A pass's sink: each block written into its bound buffer and cleaned as the emitter makes it;
/// the first refusal kept for the rung's line.
pub(super) struct PassSink<'a> {
    pub dev: &'a mut Device,
    pub client: ClientId,
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
        match write_bound(self.dev, self.client, va, at, bytes) {
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
    va: u64,
    at: u32,
    len: usize,
) -> Option<alloc::vec::Vec<u8>> {
    let bo = dev.bo_at(va, u64::from(at) + len as u64)?;
    let view = dev.bo_bytes_mut(client, bo)?;
    view.get(at as usize..at as usize + len).map(|s| s.to_vec())
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

/// The captured first draw's vertex half and blocks, read back from the render bundle's
/// bindings.
pub(super) struct Captured {
    pds_state: [u32; 3],
    vdm_state: [u32; 5],
    ppp26: alloc::vec::Vec<u8>,
    vs: alloc::vec::Vec<u8>,
}

impl Captured {
    pub(super) fn read(dev: &mut Device, client: ClientId) -> Result<Self, &'static str> {
        let stream0 = read_bound(dev, client, STREAM_BUFFER, 0, 0x30)
            .ok_or("the stream buffer is not bound (the render rung did not run)")?;
        let w: alloc::vec::Vec<u32> =
            stream0.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
        let vs = read_bound(dev, client, PDS_ARENA, pds_at(PDS_ARENA, VS_PDS_DATA_HEAP as u32), 64)
            .ok_or("the PDS arena is not bound")?;
        let ppp26 = read_bound(dev, client, GENERAL_ARENA, PPP26_OFFSET, 104)
            .ok_or("the general arena is not bound")?;
        Ok(Self {
            pds_state: [w[0], w[1], w[2]],
            vdm_state: [w[5], w[6], w[7], w[8], w[9]],
            ppp26,
            vs,
        })
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
