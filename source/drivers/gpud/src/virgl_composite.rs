// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: GPU layer compositor (G2) — the production-grade composite op, the
//! retained render-node / compositor-layer model: a layer = a content texture + a transform + per-layer GPU
//! effects (opacity, rounded-corner mask, soft drop shadow), composited on the
//! GPU into the scanout render target.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0063-ui-v5b-scene-graph-gpu-pipeline-virtual-list-theme-contract.md
//!
//! [`VirtioGpuBackend::layer_pass`] draws ONE layer into a target surface with a
//! library pipeline (`virgl_pipelines`, TASK-0215 5c-2 P6): a rectangle of a
//! content texture spread over the destination rectangle (bilinear when the
//! sizes differ), its alpha times the layer opacity and the rounded rectangle's
//! coverage, premultiplied over the target — the client-surface pipeline
//! (`layer-atlas`) for straight-alpha content, the sprite pipeline
//! (`layer-premultiplied`) for the premultiplied pointer.
//! `virgl_composite_selftest` proves it by readback at bringup (no framebuffer
//! handoff needed) → `gpud: layer composite ok`.
//!
//! DriverKit/NexusGfx boundary: the portable contract is `Command::CompositeLayer`
//! (texture region + transform + opacity + radius + shadow); the board GPU draws
//! the same programs from its own library.

#![cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]

use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::gpu::quads::Quad;
use nexus_gfx::command::buffer::RgbaColor;

use crate::backend::VirtioGpuBackend;
use crate::protocol::{
    VirtioGpuCtxAttachResource, VirtioGpuResourceCreate3d, VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE,
    VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING, VIRTIO_GPU_CMD_RESOURCE_CREATE_3D,
};
use crate::virgl::{
    Submit3d, PIPE_BIND_RENDER_TARGET, PIPE_BIND_SAMPLER_VIEW, PIPE_FORMAT_B8G8R8A8_UNORM,
    PIPE_TEXTURE_2D,
};
use crate::virgl_pipelines::Pipeline;

// Live layer compositing: the chat/window content is rendered by windowd into
// the shared framebuffer atlas (rows 3200..9599). We alias it as a GPU texture
// and sample it as the layer content, compositing straight onto the scanout RT.
const ATLAS_RES: u32 = 0xF3;
const ATLAS_SVIEW: u32 = 0x48;
const FB_STRIDE: u32 = nexus_display_proto::layout::STRIDE_BYTES;
// The atlas rows are `nexus_display_proto::layout`'s — the same constants windowd hands rows
// out from, so every row it can hand out is inside this GL alias (the two drifted once when
// each crate kept its own number: bands past the alias sampled outside the texture = garbage
// rows after maximizing).
const ATLAS_ROW: u32 = nexus_display_proto::layout::ATLAS_ROW;
const ATLAS_ROWS: u32 = nexus_display_proto::layout::ATLAS_ROWS;
const ATLAS_W: u32 = nexus_display_proto::layout::LAYOUT_MAX.0;

// Cursor sprite as a layer: its own self-backed sampler texture (BGRA), so the
// pointer composites through the generic `layer_pass` like any layer —
// no procedural rect. Resource 0xE3 (free: wallpaper 0xE2),
// sampler view 0x49 (free: 0x45 wallpaper, 0x48 atlas).
const H_CURSOR_TEX: u32 = 0xE3;
const H_CURSOR_SVIEW: u32 = 0x49;
// Real icon sprite texture + sampler view (next free ids after the cursor's).
const H_ICON_TEX: u32 = 0xE4;
const H_ICON_SVIEW: u32 = 0x4A;

/// A layer's content: its texture (a sampler view and its size) and the rectangle of it the
/// layer shows (`x`, `row`, `w`, `h` texels).
#[derive(Clone, Copy)]
pub(crate) struct Content {
    pub(crate) view: u32,
    pub(crate) tex: (u32, u32),
    pub(crate) rect: (u32, u32, u32, u32),
}

impl VirtioGpuBackend {
    /// Composite one layer with pipeline `p`: `content`'s rectangle spread over the destination
    /// rectangle (`x`, `y`, `w`, `h`) of `target` — bilinear when the sizes differ (downscale for
    /// supersampled sprites, upscale while a window resizes) — at `opacity` (0..255), its corners
    /// cut at `radius`.
    pub(crate) fn layer_pass(
        &mut self,
        p: Pipeline,
        target: u32,
        content: Content,
        (x, y, w, h): (u32, u32, u32, u32),
        opacity: u32,
        radius: u32,
    ) -> Result<(), GfxError> {
        let (tw, th) = content.tex;
        let (sx, sy, sw, sh) = content.rect;
        if !self.pipelines.ready || w == 0 || h == 0 || sw == 0 || sh == 0 || tw == 0 || th == 0 {
            return Err(GfxError::InvalidArgument);
        }
        let r = radius.min(w / 2).min(h / 2) as f32;
        let (tw, th) = (tw as f32, th as f32);
        let tint = [1.0, 1.0, 1.0, opacity.min(255) as f32 / 255.0];
        let q = Quad::layer(x as f32, y as f32, w as f32, h as f32, tint, r).with_texture([
            sw as f32 / tw,
            sh as f32 / th,
            sx as f32 / tw,
            sy as f32 / th,
        ]);
        self.draw(p, target, &q, content.view)
    }

    /// Lazily alias the framebuffer atlas (rows 3200..9599) as a GPU texture +
    /// sampler view, so window/layer content windowd renders there can be
    /// sampled. Needs the framebuffer handoff (the scanout resource's phys base).
    fn composite_atlas_init(&mut self) -> Result<(), GfxError> {
        if self.virgl_atlas_ready {
            return Ok(());
        }
        let scanout = self.scanout_resource.ok_or(GfxError::DeviceNotFound)?;
        let record = self.find_resource(scanout).ok_or(GfxError::DeviceNotFound)?;
        // The atlas rows of windowd's framebuffer, as that VMO's runs (RFC-0098 C4).
        let alias_off = ATLAS_ROW as usize * FB_STRIDE as usize;
        let alias_len = ATLAS_ROWS as usize * FB_STRIDE as usize;

        let create = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: ATLAS_RES,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_RENDER_TARGET | PIPE_BIND_SAMPLER_VIEW,
            width: ATLAS_W,
            height: ATLAS_ROWS,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&create)?;
        let hdr = self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING);
        self.attach_backing_runs(hdr, ATLAS_RES, record.dma_vmo, alias_off, alias_len)?;
        let ctx_attach = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: ATLAS_RES,
            _padding: 0,
        };
        self.ctrl_submit_struct(&ctx_attach)?;
        let mut s = Submit3d::new();
        s.emit_create_sampler_view(ATLAS_SVIEW, ATLAS_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
        self.submit3d(&s)?;
        self.virgl_atlas_ready = true;
        Ok(())
    }

    /// The layer composite: the layer straight onto the scanout render target, over
    /// what the frame composited before it — its shadow, then its content (the RT is
    /// the final surface, nothing is written back). Glass layers get their backdrop
    /// from the glass blur (destination-so-far snapshot + GPU blur,
    /// `backend::glass_blur`), run by `composite_pending_rt_layers` before this
    /// composite. Coordinates are screen-space, matching the RT.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composite_layer_rt(
        &mut self,
        src_row_abs: u32,
        src_x: u32,
        width: u32,
        height: u32,
        // Content SOURCE sub-size (`0` = same as `width`/`height`). When set and
        // different from the dest, the content band is bilinear-SCALED up to the
        // `width`×`height` frame — the window body grows live during a resize.
        content_w: u32,
        content_h: u32,
        dst_x: u32,
        dst_y: u32,
        opacity: u32,
        corner_radius: u32,
        shadow_blur: u32,
        shadow_offset_y: i32,
        shadow_alpha: u32,
        // WebRender scroll band (`scroll_band_h == 0` = not scrollable). When set,
        // the UPLOAD covers the WHOLE band `[scroll_band_top_abs, +scroll_band_h)`
        // ONCE so the `src_row` override (the SAMPLE row below) can shift within
        // already-uploaded rows — otherwise only `height` visible rows are
        // uploaded and a shifted `src_row` samples never-uploaded rows.
        scroll_band_top_abs: u32,
        scroll_band_h: u32,
        upload: bool,
    ) -> Result<(), GfxError> {
        if !self.virgl_capable || !self.virgl_draw_ok || !self.pipelines.ready {
            return Err(GfxError::DeviceNotFound);
        }
        self.composite_atlas_init()?;
        // Shadow first, alpha-blended over the base already on the RT.
        if shadow_blur > 0 {
            self.submit_drop_shadow_rt(
                dst_x,
                dst_y,
                width,
                height,
                corner_radius,
                shadow_blur,
                0,
                shadow_offset_y,
                RgbaColor::new(0, 0, 0, shadow_alpha.min(255) as u8),
            )?;
        }
        // Sync the content from the VMO atlas into the GL atlas texture — only
        // when it changed. The atlas texture persists across presents, so a
        // cursor-move present re-composites from it WITHOUT this transfer (the
        // per-frame transfer was the mouse-move slowdown).
        let src_row_rel = src_row_abs.saturating_sub(ATLAS_ROW);
        // Source region = the content band when a sub-size is given, else the
        // whole layer. Only the source region is transferred to the GL texture.
        let (src_w, src_h) = if content_w > 0 { (content_w, content_h) } else { (width, height) };
        // Never SAMPLE past the GL atlas texture either (the band-upload
        // clamp below only bounds TRANSFERS): an out-of-range sample reads
        // another window's rows as garbage. Shrink the draw with the clamp.
        let max_h = ATLAS_ROWS.saturating_sub(src_row_rel);
        let height = height.min(max_h);
        let src_h = src_h.min(max_h);
        if height == 0 || src_h == 0 {
            return Ok(());
        }
        if upload {
            if scroll_band_h > 0 {
                // WebRender scroll: upload the WHOLE resident band ONCE so the
                // per-id `src_row` override can shift WITHIN uploaded rows. The
                // band top/height are FIXED (independent of the current scroll
                // position); the SAMPLE below still uses `src_row_rel` + `height`.
                let band_top_rel = scroll_band_top_abs.saturating_sub(ATLAS_ROW);
                // Never transfer past the GL atlas texture (rows 0..ATLAS_ROWS).
                let mut band_h = scroll_band_h;
                if band_top_rel.saturating_add(band_h) > ATLAS_ROWS {
                    let clamped = ATLAS_ROWS.saturating_sub(band_top_rel);
                    if !self.scroll_band_clamp_logged {
                        self.scroll_band_clamp_logged = true;
                        let _ = nexus_abi::debug_println(&alloc::format!(
                            "gpud: scroll band clamped top_rel={band_top_rel} h={band_h} -> {clamped} (atlas rows={ATLAS_ROWS})"
                        ));
                    }
                    band_h = clamped;
                }
                self.virgl_transfer_to_host(
                    ATLAS_RES,
                    src_x,
                    band_top_rel,
                    src_w,
                    band_h,
                    FB_STRIDE,
                )?;
            } else {
                self.virgl_transfer_to_host(
                    ATLAS_RES,
                    src_x,
                    src_row_rel,
                    src_w,
                    src_h,
                    FB_STRIDE,
                )?;
            }
        }
        // Composite the content straight onto the scanout RT, over the base. A content band
        // smaller than the frame (live resize) is spread over it — the window body grows; it
        // snaps sharp when the client re-renders.
        let content = Content {
            view: ATLAS_SVIEW,
            tex: (ATLAS_W, ATLAS_ROWS),
            rect: (src_x, src_row_rel, src_w, src_h),
        };
        let target = self.rt_back_surface();
        let frame = (dst_x, dst_y, width, height);
        self.layer_pass(self.pipelines.layer_atlas, target, content, frame, opacity, corner_radius)
    }

    /// Upload the stored cursor sprite (BGRA, set by `store_cursor_sprite` when
    /// windowd arms the cursor) into its own GL sampler texture. One-shot. Returns true once a cursor texture is ready to
    /// sample. Issues create/attach/transfer ctrl commands — call OUTSIDE a present
    /// batch (like the wallpaper upload), never interleaved with batched draws.
    pub(crate) fn cursor_tex_init(&mut self) -> Result<bool, GfxError> {
        let w = self.cpu.cursor_w;
        let h = self.cpu.cursor_h;
        if w == 0 || h == 0 || self.cpu.cursor.is_empty() {
            return Ok(false); // windowd hasn't uploaded the sprite yet
        }
        if self.cursor_tex_va != 0 {
            return Ok(true); // already uploaded
        }
        let create = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: H_CURSOR_TEX,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_SAMPLER_VIEW,
            width: w,
            height: h,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&create)?;
        let ctx_attach = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: H_CURSOR_TEX,
            _padding: 0,
        };
        self.ctrl_submit_struct(&ctx_attach)?;
        let byte_len = (w as usize) * (h as usize) * 4;
        let va = self.virgl_attach_backing(H_CURSOR_TEX, byte_len)?;
        // Copy the sprite into the texture backing (tight stride = w*4).
        let dst = va as *mut u8;
        if !dst.is_null() && self.cpu.cursor.len() >= byte_len {
            unsafe {
                core::ptr::copy_nonoverlapping(self.cpu.cursor.as_ptr(), dst, byte_len);
            }
        }
        self.virgl_transfer_to_host(H_CURSOR_TEX, 0, 0, w, h, w * 4)?;
        let mut s = Submit3d::new();
        s.emit_create_sampler_view(H_CURSOR_SVIEW, H_CURSOR_TEX, PIPE_FORMAT_B8G8R8A8_UNORM);
        self.submit3d(&s)?;
        self.cursor_tex_va = va;
        self.cursor_tex_w = w;
        self.cursor_tex_h = h;
        Ok(true)
    }

    /// True once the cursor sprite has been uploaded into its GL texture.
    pub(crate) fn cursor_tex_ready(&self) -> bool {
        self.cursor_tex_va != 0
    }

    /// Re-upload the STORED cursor sprite into the existing GL cursor texture
    /// (TASK-0070 Phase 3 pointer-shape switch: windowd re-sends
    /// `OP_UPLOAD_CURSOR` with a different 32×32 sprite; the texture backing
    /// is memcpy'd + transferred — no new resource). No-op until the first
    /// `cursor_tex_init`; dimension changes are refused (all shapes share the
    /// init dimensions by contract). Call OUTSIDE a present batch.
    pub(crate) fn cursor_tex_refresh(&mut self) -> Result<(), GfxError> {
        if self.cursor_tex_va == 0 {
            return Ok(()); // init (lazy, at next present) picks up the sprite
        }
        let w = self.cpu.cursor_w;
        let h = self.cpu.cursor_h;
        if w != self.cursor_tex_w || h != self.cursor_tex_h {
            return Ok(()); // dims fixed at init — keep the old shape visible
        }
        let byte_len = (w as usize) * (h as usize) * 4;
        let dst = self.cursor_tex_va as *mut u8;
        if !dst.is_null() && self.cpu.cursor.len() >= byte_len {
            unsafe {
                core::ptr::copy_nonoverlapping(self.cpu.cursor.as_ptr(), dst, byte_len);
            }
        }
        self.virgl_transfer_to_host(H_CURSOR_TEX, 0, 0, w, h, w * 4)?;
        Ok(())
    }

    /// Composite the cursor sprite as a layer onto the scanout RT at (`dst_x`,
    /// `dst_y`): the sprite pipeline — its colour is premultiplied (nexus-svg), so its
    /// anti-aliased edges keep no dark fringe. Safe inside the present batch (the
    /// texture was created in `cursor_tex_init` outside it). No-op until the sprite
    /// is uploaded.
    pub(crate) fn composite_cursor_rt(&mut self, dst_x: u32, dst_y: u32) -> Result<(), GfxError> {
        if self.cursor_tex_va == 0 {
            return Ok(());
        }
        let (w, h) = (self.cursor_tex_w, self.cursor_tex_h);
        let content = Content { view: H_CURSOR_SVIEW, tex: (w, h), rect: (0, 0, w, h) };
        let target = self.rt_back_surface();
        let p = self.pipelines.layer_premultiplied;
        self.layer_pass(p, target, content, (dst_x, dst_y, w, h), 255, 0)
    }

    /// Upload the real icon sprite (set via `store_icon_sprite`) into its own GL
    /// sampler texture, once. Mirrors `cursor_tex_init` exactly — a separate
    /// texture/sampler-view so the icon composites as its own sprite layer. Returns
    /// `Ok(false)` until windowd has uploaded the sprite.
    pub(crate) fn icon_tex_init(&mut self) -> Result<bool, GfxError> {
        let w = self.icon_sprite_w;
        let h = self.icon_sprite_h;
        if w == 0 || h == 0 || self.icon_sprite.is_empty() {
            return Ok(false); // windowd hasn't uploaded the icon yet
        }
        if self.icon_tex_va != 0 {
            return Ok(true); // already uploaded
        }
        let create = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: H_ICON_TEX,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_SAMPLER_VIEW,
            width: w,
            height: h,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&create)?;
        let ctx_attach = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: H_ICON_TEX,
            _padding: 0,
        };
        self.ctrl_submit_struct(&ctx_attach)?;
        let byte_len = (w as usize) * (h as usize) * 4;
        let va = self.virgl_attach_backing(H_ICON_TEX, byte_len)?;
        let dst = va as *mut u8;
        if !dst.is_null() && self.icon_sprite.len() >= byte_len {
            unsafe {
                core::ptr::copy_nonoverlapping(self.icon_sprite.as_ptr(), dst, byte_len);
            }
        }
        self.virgl_transfer_to_host(H_ICON_TEX, 0, 0, w, h, w * 4)?;
        let mut s = Submit3d::new();
        s.emit_create_sampler_view(H_ICON_SVIEW, H_ICON_TEX, PIPE_FORMAT_B8G8R8A8_UNORM);
        self.submit3d(&s)?;
        self.icon_tex_va = va;
        self.icon_tex_w = w;
        self.icon_tex_h = h;
        Ok(true)
    }

    /// True once the icon sprite has been uploaded into its GL texture.
    pub(crate) fn icon_tex_ready(&self) -> bool {
        self.icon_tex_va != 0
    }

    /// Composite the icon sprite as a layer at its stored position, GPU-scaled
    /// from the (possibly 2×/supersampled) texture down to its on-screen size: the
    /// client-surface pipeline (the sprite is straight alpha). No-op until the
    /// sprite is uploaded. Square corners, fully opaque.
    pub(crate) fn composite_icon_rt(&mut self) -> Result<(), GfxError> {
        if self.icon_tex_va == 0 {
            return Ok(());
        }
        let (tw, th) = (self.icon_tex_w, self.icon_tex_h);
        let (dx, dy) = (self.icon_dst_x, self.icon_dst_y);
        let dw = if self.icon_dst_w == 0 { tw } else { self.icon_dst_w };
        let dh = if self.icon_dst_h == 0 { th } else { self.icon_dst_h };
        let content = Content { view: H_ICON_SVIEW, tex: (tw, th), rect: (0, 0, tw, th) };
        let target = self.rt_back_surface();
        self.layer_pass(self.pipelines.layer_atlas, target, content, (dx, dy, dw, dh), 255, 0)
    }
}
