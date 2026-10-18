// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The glass layer's backdrop blur on virgl (TASK-0215 5c-2 P6 — the board's chain of
//! P5d on the QEMU path): what the frame composited beneath a glass layer, snapshotted with its
//! halo, blurred at half resolution by the library's two passes (`blur.wgsl` — the horizontal
//! one reads the full-resolution snapshot, each tap one bilinear fetch at the corner of four
//! texels, which is the downsample; the vertical one reads the half-resolution result), drawn
//! back over the glass with its corners cut by the client-surface pipeline. CACHED (the frame
//! planner decides, `nexus_gfx::frame::glass`): an unchanged backdrop draws its blurred rectangle
//! from the cache texture — one
//! layer draw — instead of the snapshot and both passes; a changed one blurs into its cache rows.
//! OWNERS: @ui @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the chain's numbers on the host (`tools/nxgpu/tests/blur.rs`, within three
//!   levels of the full-resolution gaussian); `gpud: rt backdrop dst ok` + the visible lanes.
//!
//! The half-resolution result covers the glass and one texel more on every side: the layer's
//! bilinear fetches at the glass's edges read blurred backdrop, never a neighbour's cache rows
//! (the board names the result's rectangle in its texture descriptor, which clamps; a virgl
//! texture is the whole cache).

#![cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]

use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::gpu::quads::Quad;
use nexus_gfx::frame::{CacheStep, FrameLayer, Glass};

use super::VirtioGpuBackend;
use crate::protocol::{
    VirtioGpuCtxAttachResource, VirtioGpuResourceCreate3d, VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE,
    VIRTIO_GPU_CMD_RESOURCE_CREATE_3D,
};
use crate::virgl::{
    Submit3d, PIPE_BIND_RENDER_TARGET, PIPE_BIND_SAMPLER_VIEW, PIPE_FORMAT_B8G8R8A8_UNORM,
    PIPE_TEXTURE_2D,
};
use crate::virgl_pipelines::BlurPair;

/// The cache texture's height (display-wide) in half-resolution rows: two windows of pane glass
/// fit; what does not blurs live — the height bounds cost, never correctness.
pub(crate) const BLUR_CACHE_TEX_H: u32 = 2048;

/// The snapshot of the frame beneath a glass layer and its halo (display-sized, GPU-only: no
/// guest backing, no transfers) — copied from the back RT before the glass's passes, so the blur
/// reads the DESTINATION-SO-FAR (lower windows, chrome; TASK-0070 Phase 4). Also the vertical
/// pass's target when no cache rows are left.
const H_BACKDROP_TEX: u32 = 0xE5;
const H_SV_BACKDROP: u32 = 0x4B;
const H_BACKDROP_SURF: u32 = 0x50;
/// The horizontal pass's half-resolution result (display-sized, GPU-only).
const H_BLUR_TMP_TEX: u32 = 0xE6;
const H_BLUR_TMP_SURF: u32 = 0x4C;
const H_SV_BLUR_TMP: u32 = 0x4D;
/// The cache: vertical results packed in rows at column 0 (display-wide, GPU-only).
const H_BLUR_CACHE_TEX: u32 = 0xE8;
const H_SV_BLUR_CACHE: u32 = 0x4F;
const H_BLUR_CACHE_SURF: u32 = 0x51;

impl VirtioGpuBackend {
    /// One-shot: the snapshot, scratch and cache textures with their views and surfaces. Issues
    /// create/attach ctrl commands — call OUTSIDE a present batch (like `cursor_tex_init`).
    /// Idempotent after the first success.
    pub(crate) fn glass_tex_init(&mut self) -> Result<(), GfxError> {
        if self.frame.glass.ready {
            return Ok(());
        }
        let textures = [
            (H_BACKDROP_TEX, H_SV_BACKDROP, H_BACKDROP_SURF, self.display_h),
            (H_BLUR_TMP_TEX, H_SV_BLUR_TMP, H_BLUR_TMP_SURF, self.display_h),
            (H_BLUR_CACHE_TEX, H_SV_BLUR_CACHE, H_BLUR_CACHE_SURF, BLUR_CACHE_TEX_H),
        ];
        let mut s = Submit3d::new();
        for (res, view, surface, height) in textures {
            let create = VirtioGpuResourceCreate3d {
                hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
                resource_id: res,
                target: PIPE_TEXTURE_2D,
                format: PIPE_FORMAT_B8G8R8A8_UNORM,
                bind: PIPE_BIND_RENDER_TARGET | PIPE_BIND_SAMPLER_VIEW,
                width: self.display_w,
                height,
                depth: 1,
                array_size: 1,
                last_level: 0,
                nr_samples: 0,
                flags: 0,
                _padding: 0,
            };
            self.ctrl_submit_struct(&create)?;
            let attach = VirtioGpuCtxAttachResource {
                hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
                resource_id: res,
                _padding: 0,
            };
            self.ctrl_submit_struct(&attach)?;
            s.emit_create_sampler_view(view, res, PIPE_FORMAT_B8G8R8A8_UNORM);
            s.emit_create_surface(surface, res, PIPE_FORMAT_B8G8R8A8_UNORM);
        }
        self.submit3d(&s)?;
        self.frame.glass.ready = true;
        Ok(())
    }

    /// One glass layer of the plan (`nexus_gfx::frame`, its cache step decided): the cache's
    /// read, the blur into the cache rows, or — no rows left — the blur alone, then the blurred
    /// rectangle drawn over the glass.
    pub(crate) fn glass_draw(&mut self, l: &FrameLayer, glass: Glass) {
        let (x, y, w, h) = (l.dst_x, l.dst_y, l.width, l.height);
        // The plan asks only for radii the library blurs.
        let Some(pair) = self.pipelines.blur(l.backdrop_blur) else { return };
        let mut s = Submit3d::new();
        let step = glass.cache;
        let (view, tex_h, row) = match step {
            CacheStep::Read(row) => (H_SV_BLUR_CACHE, BLUR_CACHE_TEX_H, row),
            CacheStep::Write(row) => {
                self.emit_blur_chain(&mut s, pair, (x, y, w, h), H_BLUR_CACHE_SURF, row);
                (H_SV_BLUR_CACHE, BLUR_CACHE_TEX_H, row)
            }
            CacheStep::Off => {
                self.emit_blur_chain(&mut s, pair, (x, y, w, h), H_BACKDROP_SURF, 0);
                (H_SV_BACKDROP, self.display_h, 0)
            }
        };
        // The glass's half-resolution rectangle (inside the margin) over the glass, its corners
        // cut, premultiplied over what is beneath.
        let r = l.corner_radius.min(w / 2).min(h / 2) as f32;
        let (tw, th) = (self.display_w as f32, tex_h as f32);
        let q = Quad::layer(x as f32, y as f32, w as f32, h as f32, [1.0; 4], r).with_texture([
            w as f32 / 2.0 / tw,
            h as f32 / 2.0 / th,
            1.0 / tw,
            (row + 1) as f32 / th,
        ]);
        self.emit_draw(&mut s, self.pipelines.layer_atlas, self.rt_back_surface(), &q, view);
        let submitted = self.submit3d(&s).is_ok();
        // Honest one-shot: a destination-so-far snapshot and blur reached the ring.
        if submitted && !matches!(step, CacheStep::Read(_)) && !self.rt_backdrop_marker_done {
            self.rt_backdrop_marker_done = true;
            let _ = nexus_abi::debug_println(crate::markers::GPUD_RT_BACKDROP_DST);
        }
    }

    /// The glass (`x`, `y`, `w`, `h`) blurred at half resolution with one texel of margin into
    /// `target` at (0, `row`): the back RT beneath it and the halo the passes reach copied into
    /// the snapshot; the horizontal pass into the scratch — its taps a two-texel step, each fetch
    /// at the corner of four texels, `taps` half-resolution rows more above and below for the
    /// vertical pass; the vertical pass into `target`.
    fn emit_blur_chain(
        &self,
        s: &mut Submit3d,
        pair: BlurPair,
        (x, y, w, h): (u32, u32, u32, u32),
        target: u32,
        row: u32,
    ) {
        let taps = pair.radius / 2;
        let (vw, vh) = (w.div_ceil(2) + 2, h.div_ceil(2) + 2);
        // The result's origin in the frame: the glass less its margin (two full pixels).
        let (ox, oy) = (x as f32 - 2.0, y as f32 - 2.0);
        // The snapshot: every texel the horizontal pass's fetches reach.
        let reach = 2 * taps + 2;
        let (x0, y0) = (x.saturating_sub(reach), y.saturating_sub(reach));
        let x1 = (x + 2 * vw + 2 * taps - 2).min(self.display_w);
        let y1 = (y + 2 * vh + 2 * taps - 2).min(self.display_h);
        if x1 > x0 && y1 > y0 {
            let back = self.rt_back_res();
            s.emit_resource_copy_region(H_BACKDROP_TEX, x0, y0, back, x0, y0, x1 - x0, y1 - y0);
        }
        let (fw, fh) = (self.display_w as f32, self.display_h as f32);
        let hh = vh + 2 * taps;
        let halo = (2 * taps) as f32;
        // Horizontal: a half-resolution pixel's centre is the corner of the four backdrop
        // texels it averages, `2 taps` full rows above the result on; a tap two texels across.
        let source = [2.0 * vw as f32 / fw, 2.0 * hh as f32 / fh, ox / fw, (oy - halo) / fh];
        let q = Quad::blur(0.0, 0.0, vw as f32, hh as f32, source, [2.0 / fw, 0.0], 1.0);
        self.emit_draw(s, pair.horizontal, H_BLUR_TMP_SURF, &q, H_SV_BACKDROP);
        // Vertical: the horizontal result's texel under the output pixel, `taps` rows down its
        // halo; a tap one row down; the saturation left as it is.
        let source = [vw as f32 / fw, vh as f32 / fh, 0.0, taps as f32 / fh];
        let q = Quad::blur(0.0, row as f32, vw as f32, vh as f32, source, [0.0, 1.0 / fh], 1.0);
        self.emit_draw(s, pair.vertical, target, &q, H_SV_BLUR_TMP);
    }
}
