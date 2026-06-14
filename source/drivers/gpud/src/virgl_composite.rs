// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: GPU layer compositor (G2) — the production-grade composite op, the
//! OHOS RenderService RSRenderNode / Apple Core Animation CALayer / Fuchsia
//! Flatland model: a layer = a content texture + a transform + per-layer GPU
//! effects (opacity, rounded-corner mask, soft drop shadow), composited on the
//! GPU into the scanout render target.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0063-ui-v5b-scene-graph-gpu-pipeline-virtual-list-theme-contract.md
//!
//! `submit_layer_pass` draws ONE layer into a target surface: it samples the
//! content texture (fragcoord→UV, like the scanout blit) and multiplies the
//! sampled alpha by an analytic rounded-rect SDF coverage and the layer
//! opacity, alpha-blended over the target. This is the GPU primitive the live
//! compositor (and the scene graph) drive; `virgl_composite_selftest` proves it
//! by readback at bringup (no framebuffer handoff needed) → `gpud: layer composite ok`.
//!
//! DriverKit/NexusGfx boundary: the portable contract is `Command::CompositeLayer`
//! (texture region + transform + opacity + radius + shadow). A real-GPU backend
//! reimplements `submit_layer_pass` against its own ISA.

#![cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]

use nexus_gfx::backend::error::GfxError;
use nexus_gfx::command::buffer::RgbaColor;

use crate::backend::VirtioGpuBackend;
use crate::protocol::{
    VirtioGpuCtxAttachResource, VirtioGpuMemEntry, VirtioGpuResourceAttachBacking,
    VirtioGpuResourceCreate3d, VirtioGpuSubmit3d, VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE,
    VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING, VIRTIO_GPU_CMD_RESOURCE_CREATE_3D,
    VIRTIO_GPU_CMD_SUBMIT_3D,
};
use crate::virgl::{
    Submit3d, PIPE_BIND_RENDER_TARGET, PIPE_BIND_SAMPLER_VIEW, PIPE_CLEAR_COLOR0,
    PIPE_FORMAT_B8G8R8A8_UNORM, PIPE_PRIM_TRIANGLES, PIPE_SHADER_FRAGMENT, PIPE_SHADER_VERTEX,
    PIPE_TEXTURE_2D, VIRGL_OBJECT_BLEND, VIRGL_OBJECT_DSA, VIRGL_OBJECT_RASTERIZER,
    VIRGL_OBJECT_VERTEX_ELEMENTS,
};

// Object handles (context-global namespace — see backend.rs gradient-selftest
// comment for the full allocation map). Composite owns 20, 0x47, 0x61.
const H_FS_LAYER: u32 = 20;
const H_SAMPLER: u32 = 0x47;
const H_BLEND_ALPHA: u32 = 0x61;
// Reused from the boot draw self-test (persist for the gpud lifetime):
const H_VS: u32 = 10; // passthrough vertex shader
const H_DSA: u32 = 0x21;
const H_RAST: u32 = 0x22;
const H_VE: u32 = 0x23; // vec4 position vertex elements
const FULLSCREEN_TRI_VBO: u32 = 0xF5; // 3-vert clip-space triangle

// Self-test resources/handles (bringup only).
const ST_CONTENT_RES: u32 = 0xEA;
const ST_DEST_RES: u32 = 0xEB;
const ST_CONTENT_SURF: u32 = 0x44;
const ST_DEST_SURF: u32 = 0x45;
const ST_CONTENT_SVIEW: u32 = 0x46;

// Live layer compositing: the chat/window content is rendered by windowd into
// the shared framebuffer atlas (rows 3200..6399). We alias it as a GPU texture
// and sample it as the layer content. The destination is the display-plane
// surface 0x30 (= texture 0xF8 aliasing rows 1600..3199), which the existing
// GL present already blits to the scanout RT — so no present-path surgery.
const ATLAS_RES: u32 = 0xF3;
const ATLAS_SVIEW: u32 = 0x48;
const H_FBSRC_SURF: u32 = 0x30; // display-plane surface (created by virgl_blur_init)
const FB_STRIDE: u32 = 1280 * 4;
const ATLAS_ROW: u32 = 3200; // atlas start row in the VMO
const ATLAS_ROWS: u32 = 3200; // atlas height (rows 3200..6399)
const DISPLAY_PLANE_ROW: u32 = 1600;

/// Sample a content texture (fragcoord→UV) and modulate its alpha by an analytic
/// rounded-rect SDF coverage and the layer opacity.
/// CONST[0] = (1/tex_w, 1/tex_h, (src_x-dst_x)/tex_w, (src_row-dst_y)/tex_h) — UV map.
/// CONST[1] = (-cx, -cy, bx, by) — layer rect centre (negated) + half-extents minus radius.
/// CONST[2] = (radius, opacity01, 0, 0).
const FS_LAYER: &str = "FRAG\n\
    DCL IN[0], POSITION, LINEAR\n\
    DCL OUT[0], COLOR\n\
    DCL SAMP[0]\n\
    DCL SVIEW[0], 2D, FLOAT\n\
    DCL CONST[0..2]\n\
    DCL TEMP[0..5]\n\
    IMM[0] FLT32 { 0.5000, 0.0000, 1.0000, 0.0000}\n\
    MAD TEMP[0].xy, IN[0].xyyy, CONST[0].xyyy, CONST[0].zwww\n\
    TEX TEMP[1], TEMP[0], SAMP[0], 2D\n\
    ADD TEMP[2].xy, IN[0].xyyy, CONST[1].xyyy\n\
    MAX TEMP[2].xy, TEMP[2].xyyy, -TEMP[2].xyyy\n\
    ADD TEMP[2].xy, TEMP[2].xyyy, -CONST[1].zwww\n\
    MAX TEMP[3].xy, TEMP[2].xyyy, IMM[0].yyyy\n\
    DP2 TEMP[4].x, TEMP[3].xyyy, TEMP[3].xyyy\n\
    SQRT TEMP[4].x, TEMP[4].xxxx\n\
    MAX TEMP[5].x, TEMP[2].xxxx, TEMP[2].yyyy\n\
    MIN TEMP[5].x, TEMP[5].xxxx, IMM[0].yyyy\n\
    ADD TEMP[4].x, TEMP[4].xxxx, TEMP[5].xxxx\n\
    ADD TEMP[4].x, TEMP[4].xxxx, -CONST[2].xxxx\n\
    ADD TEMP[5].x, IMM[0].xxxx, -TEMP[4].xxxx\n\
    MAX TEMP[5].x, TEMP[5].xxxx, IMM[0].yyyy\n\
    MIN TEMP[5].x, TEMP[5].xxxx, IMM[0].zzzz\n\
    MUL TEMP[5].x, TEMP[5].xxxx, CONST[2].yyyy\n\
    MUL TEMP[1].w, TEMP[1].wwww, TEMP[5].xxxx\n\
    MOV OUT[0], TEMP[1]\n\
    END\n";

impl VirtioGpuBackend {
    /// Lazily create the layer shader, alpha-blend state, and sampler state.
    /// No framebuffer dependency — usable at bringup and live.
    fn composite_init(&mut self) -> Result<(), GfxError> {
        if self.virgl_composite_ready {
            return Ok(());
        }
        let mut s = Submit3d::new();
        s.emit_create_shader(H_FS_LAYER, PIPE_SHADER_FRAGMENT, FS_LAYER);
        s.emit_create_blend_alpha(H_BLEND_ALPHA);
        s.emit_create_sampler_state_default(H_SAMPLER);
        self.submit_composite_stream(&s)?;
        self.virgl_composite_ready = true;
        Ok(())
    }

    /// Composite one layer (content texture region → target surface) with a
    /// rounded-corner mask + opacity, alpha-blended. `tex_w/h` are the content
    /// texture dimensions; `src_x/src_row` its sub-region; `dst_x/dst_y/w/h` the
    /// destination rect (screen/RT space, also the viewport + SDF frame).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn submit_layer_pass(
        &mut self,
        target_surface: u32,
        content_sview: u32,
        tex_w: u32,
        tex_h: u32,
        src_x: u32,
        src_row: u32,
        w: u32,
        h: u32,
        dst_x: u32,
        dst_y: u32,
        opacity: u32,
        radius: u32,
    ) -> Result<(), GfxError> {
        self.composite_init()?;
        if w == 0 || h == 0 || tex_w == 0 || tex_h == 0 {
            return Err(GfxError::InvalidArgument);
        }
        let r = radius.min(w / 2).min(h / 2) as f32;
        let cx = dst_x as f32 + w as f32 / 2.0;
        let cy = dst_y as f32 + h as f32 / 2.0;
        let bx = w as f32 / 2.0 - r;
        let by = h as f32 / 2.0 - r;
        let inv_tw = 1.0 / tex_w as f32;
        let inv_th = 1.0 / tex_h as f32;
        let uv_off_x = (src_x as f32 - dst_x as f32) * inv_tw;
        let uv_off_y = (src_row as f32 - dst_y as f32) * inv_th;
        let opacity01 = (opacity.min(255) as f32) / 255.0;

        let mut s = Submit3d::new();
        s.emit_bind_object(VIRGL_OBJECT_BLEND, H_BLEND_ALPHA);
        s.emit_bind_object(VIRGL_OBJECT_DSA, H_DSA);
        s.emit_bind_object(VIRGL_OBJECT_RASTERIZER, H_RAST);
        s.emit_bind_object(VIRGL_OBJECT_VERTEX_ELEMENTS, H_VE);
        s.emit_set_framebuffer_state(0, &[target_surface]);
        s.emit_set_viewport_box(dst_x as f32, dst_y as f32, w as f32, h as f32);
        s.emit_set_sampler_views(PIPE_SHADER_FRAGMENT, 0, &[content_sview]);
        s.emit_bind_sampler_states(PIPE_SHADER_FRAGMENT, 0, &[H_SAMPLER]);
        s.emit_set_constant_buffer(
            PIPE_SHADER_FRAGMENT,
            &[
                inv_tw, inv_th, uv_off_x, uv_off_y, //
                -cx, -cy, bx, by, //
                r, opacity01, 0.0, 0.0,
            ],
        );
        s.emit_bind_shader(H_VS, PIPE_SHADER_VERTEX);
        s.emit_bind_shader(H_FS_LAYER, PIPE_SHADER_FRAGMENT);
        s.emit_set_vertex_buffers(&[(16, 0, FULLSCREEN_TRI_VBO)]);
        s.emit_draw_vbo(0, 3, PIPE_PRIM_TRIANGLES);
        self.submit_composite_stream(&s)
    }

    /// Bringup proof: composite a red content layer onto a blue dest RT and read
    /// back — center must be red (layer), a corner must stay blue (outside the
    /// layer). Returns true on success. Reuses the draw-selftest's state objects
    /// (blend/DSA/rast/VE/VS/triangle VBO) created earlier in bringup.
    pub(crate) fn virgl_composite_selftest(&mut self) -> Result<bool, GfxError> {
        if !self.virgl_capable || !self.virgl_draw_ok {
            return Err(GfxError::DeviceNotFound);
        }
        self.composite_init()?;

        self.virgl_create_rt(ST_CONTENT_RES, 64, 64)?;
        self.virgl_create_rt(ST_DEST_RES, 128, 128)?;
        let dest_va = self.virgl_attach_backing(ST_DEST_RES, 128 * 128 * 4)?;

        // Surfaces + content sampler view; clear content=red, dest=blue.
        let mut s = Submit3d::new();
        s.emit_create_surface(ST_CONTENT_SURF, ST_CONTENT_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_surface(ST_DEST_SURF, ST_DEST_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_sampler_view(ST_CONTENT_SVIEW, ST_CONTENT_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_set_framebuffer_state(0, &[ST_CONTENT_SURF]);
        s.emit_set_viewport_box(0.0, 0.0, 64.0, 64.0);
        s.emit_clear(PIPE_CLEAR_COLOR0, [1.0, 0.0, 0.0, 1.0], 1.0, 0); // red (RGBA clear)
        s.emit_set_framebuffer_state(0, &[ST_DEST_SURF]);
        s.emit_set_viewport_box(0.0, 0.0, 128.0, 128.0);
        s.emit_clear(PIPE_CLEAR_COLOR0, [0.0, 0.0, 1.0, 1.0], 1.0, 0); // blue
        self.submit_composite_stream(&s)?;

        // Composite the red content as a 64×64 layer at (32,32), opaque, square.
        self.submit_layer_pass(
            ST_DEST_SURF,
            ST_CONTENT_SVIEW,
            64,
            64,
            0,
            0,
            64,
            64,
            32,
            32,
            255,
            0,
        )?;

        // Read back the dest RT and inspect: center red, corner blue.
        self.virgl_transfer_from_host(ST_DEST_RES, 0, 0, 128, 128, 128 * 4)?;
        let px = |x: usize, y: usize| -> [u8; 4] {
            let o = (y * 128 + x) * 4;
            unsafe {
                let p = (dest_va + o) as *const u8;
                [
                    p.read_volatile(),
                    p.add(1).read_volatile(),
                    p.add(2).read_volatile(),
                    p.add(3).read_volatile(),
                ]
            }
        };
        // BGRA: red = [0,0,255,255], blue = [255,0,0,255].
        let center = px(64, 64);
        let corner = px(8, 8);
        let center_red = center[2] > 200 && center[0] < 64;
        let corner_blue = corner[0] > 200 && corner[2] < 64;
        Ok(center_red && corner_blue)
    }

    /// Lazily alias the framebuffer atlas (rows 3200..6399) as a GPU texture +
    /// sampler view, so window/layer content windowd renders there can be
    /// sampled. Needs the framebuffer handoff (the scanout resource's phys base).
    fn composite_atlas_init(&mut self) -> Result<(), GfxError> {
        if self.virgl_atlas_ready {
            return Ok(());
        }
        let scanout = self.scanout_resource.ok_or(GfxError::DeviceNotFound)?;
        let record = self.find_resource(scanout).ok_or(GfxError::DeviceNotFound)?;
        let alias_pa = record.backing_pa + (ATLAS_ROW as u64) * (FB_STRIDE as u64);
        let alias_len = ATLAS_ROWS * FB_STRIDE;

        let create = VirtioGpuResourceCreate3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_CREATE_3D),
            resource_id: ATLAS_RES,
            target: PIPE_TEXTURE_2D,
            format: PIPE_FORMAT_B8G8R8A8_UNORM,
            bind: PIPE_BIND_RENDER_TARGET | PIPE_BIND_SAMPLER_VIEW,
            width: 1280,
            height: ATLAS_ROWS,
            depth: 1,
            array_size: 1,
            last_level: 0,
            nr_samples: 0,
            flags: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&create)?;
        let attach = VirtioGpuResourceAttachBacking {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING),
            resource_id: ATLAS_RES,
            nr_entries: 1,
        };
        let entry = VirtioGpuMemEntry { addr: alias_pa, length: alias_len, _padding: 0 };
        self.ctrl_submit_pair(&attach, &entry)?;
        let ctx_attach = VirtioGpuCtxAttachResource {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_CTX_ATTACH_RESOURCE),
            resource_id: ATLAS_RES,
            _padding: 0,
        };
        self.ctrl_submit_struct(&ctx_attach)?;
        let mut s = Submit3d::new();
        s.emit_create_sampler_view(ATLAS_SVIEW, ATLAS_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
        self.submit_composite_stream(&s)?;
        self.virgl_atlas_ready = true;
        Ok(())
    }

    /// Live GPU composite of one window/layer into the display plane (G2): soft
    /// drop shadow + the content texture (atlas region) with a rounded-corner
    /// mask + opacity, alpha-blended over the current backdrop. Renders into the
    /// display-plane surface 0x30 (the existing GL present then shows it). All
    /// coordinates are display-plane-relative (0..800), matching CompositeLayer.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn composite_layer_gpu(
        &mut self,
        src_row_abs: u32,
        src_x: u32,
        width: u32,
        height: u32,
        dst_x: u32,
        dst_y: u32,
        opacity: u32,
        corner_radius: u32,
        shadow_blur: u32,
        shadow_offset_y: i32,
        shadow_alpha: u32,
        backdrop_blur: u32,
    ) -> Result<(), GfxError> {
        if !self.virgl_capable || !self.virgl_draw_ok {
            return Err(GfxError::DeviceNotFound);
        }
        self.composite_atlas_init()?;
        // Shadow first (into the display plane, blends over the backdrop).
        if shadow_blur > 0 {
            self.submit_virgl_drop_shadow(
                dst_x,
                dst_y + DISPLAY_PLANE_ROW,
                width,
                height,
                corner_radius,
                shadow_blur,
                0,
                shadow_offset_y,
                RgbaColor::new(0, 0, 0, shadow_alpha.min(255) as u8),
            )?;
        }
        // Sync the content from the VMO atlas into the GL atlas texture.
        let src_row_rel = src_row_abs.saturating_sub(ATLAS_ROW);
        self.virgl_transfer_to_host(ATLAS_RES, src_x, src_row_rel, width, height, FB_STRIDE)?;
        if backdrop_blur > 0 {
            // Glass: GPU-blur the destination backdrop, leaving the result in
            // the display texture (0xF8) only — the layer pass composites over
            // it directly, so we skip the blur's VMO writeback (one fewer
            // transfer per glass frame; composite_layer_gpu does the final
            // writeback after the content is blended).
            let (fb, fb_len, fb_w, _row) =
                self.scanout_fb().ok_or(GfxError::DeviceNotFound)?;
            self.submit_virgl_blur(
                fb,
                fb_len,
                fb_w,
                dst_x,
                dst_y + DISPLAY_PLANE_ROW,
                width,
                height,
                backdrop_blur,
                false,
            )?;
        } else {
            // Sync the destination backdrop region into the display texture so
            // the layer's translucent (rounded) edges blend over current content.
            self.virgl_transfer_to_host(0xF8, dst_x, dst_y, width, height, FB_STRIDE)?;
        }
        // Composite the layer into the display-plane surface.
        self.submit_layer_pass(
            H_FBSRC_SURF,
            ATLAS_SVIEW,
            1280,
            ATLAS_ROWS,
            src_x,
            src_row_rel,
            width,
            height,
            dst_x,
            dst_y,
            opacity,
            corner_radius,
        )?;
        // Land the composited region back in the scanned-out VMO.
        self.virgl_transfer_from_host(0xF8, dst_x, dst_y, width, height, FB_STRIDE)
    }

    /// RT-direct layer composite (Increment 1 of true GPU compositing): composite
    /// the layer straight onto the scanout render target (`H_GLS_SURF`), over the
    /// base that gl_present already blitted there — NO VMO display-plane writeback
    /// and NO `0xF8` backdrop transfer (the RT is the final surface). Only the
    /// `backdrop_blur == 0` case (shadow + content); glass-over-dynamic still uses
    /// `composite_layer_gpu` until the RT-backdrop pass lands in a later step.
    /// Coordinates are screen-space (0..SCREEN_H), matching the RT.
    pub(crate) fn composite_layer_rt(
        &mut self,
        src_row_abs: u32,
        src_x: u32,
        width: u32,
        height: u32,
        dst_x: u32,
        dst_y: u32,
        opacity: u32,
        corner_radius: u32,
        shadow_blur: u32,
        shadow_offset_y: i32,
        shadow_alpha: u32,
    ) -> Result<(), GfxError> {
        if !self.virgl_capable || !self.virgl_draw_ok {
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
        // Sync the content from the VMO atlas into the GL atlas texture.
        let src_row_rel = src_row_abs.saturating_sub(ATLAS_ROW);
        self.virgl_transfer_to_host(ATLAS_RES, src_x, src_row_rel, width, height, FB_STRIDE)?;
        // Composite the content straight onto the scanout RT (alpha-over base).
        self.submit_layer_pass(
            crate::gl_scanout::H_GLS_SURF,
            ATLAS_SVIEW,
            1280,
            ATLAS_ROWS,
            src_x,
            src_row_rel,
            width,
            height,
            dst_x,
            dst_y,
            opacity,
            corner_radius,
        )
    }

    fn submit_composite_stream(&mut self, s: &Submit3d) -> Result<(), GfxError> {
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, &bytes)
    }
}
