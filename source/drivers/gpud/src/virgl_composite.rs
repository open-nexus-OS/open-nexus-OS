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
/// Premultiplied-alpha "over" blend (rgb_src = ONE) — for premultiplied sprites
/// (the cursor). Straight-alpha content (atlas layers, icon) keeps H_BLEND_ALPHA.
const H_BLEND_PREMUL: u32 = 0x62;
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
const ATLAS_ROWS: u32 = 4000; // atlas height (rows 3200..7199)
const DISPLAY_PLANE_ROW: u32 = 1600;

// Cursor sprite as a layer: its own self-backed sampler texture (BGRA), so the
// pointer composites through the generic `submit_layer_pass` like any layer —
// no procedural rect. Resource 0xE3 (free: display 0xE1, wallpaper 0xE2),
// sampler view 0x49 (free: 0x44/0x45 display/wallpaper, 0x48 atlas).
const H_CURSOR_TEX: u32 = 0xE3;
const H_CURSOR_SVIEW: u32 = 0x49;
// Real icon sprite texture + sampler view (next free ids after the cursor's).
const H_ICON_TEX: u32 = 0xE4;
const H_ICON_SVIEW: u32 = 0x4A;

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
        s.emit_create_blend_premult(H_BLEND_PREMUL);
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
        // 1:1 texel→pixel straight-alpha path (atlas layers / scroll): src==dst,
        // standard alpha blend. Premultiplied sprites use `submit_layer_pass_scaled`
        // with `H_BLEND_PREMUL` directly (see `composite_sprite_rt`).
        self.submit_layer_pass_scaled(
            target_surface,
            content_sview,
            tex_w,
            tex_h,
            src_x,
            src_row,
            w,
            h,
            dst_x,
            dst_y,
            w,
            h,
            opacity,
            radius,
            H_BLEND_ALPHA,
        )
    }

    /// Composite a `src_w×src_h` region of a texture (at `src_x`,`src_row`) into a
    /// `dst_w×dst_h` rect at (`dst_x`,`dst_y`), with GPU bilinear scaling when the
    /// sizes differ (downscale for supersampled/HiDPI sprites, upscale otherwise).
    /// The UV step is per-DEST-pixel = src-texels / (tex · dst), so the sampled
    /// region spreads across the dest rect; with `src==dst` this is exactly the
    /// 1:1 blit. Rounded-corner mask + opacity use the dest geometry.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn submit_layer_pass_scaled(
        &mut self,
        target_surface: u32,
        content_sview: u32,
        tex_w: u32,
        tex_h: u32,
        src_x: u32,
        src_row: u32,
        src_w: u32,
        src_h: u32,
        dst_x: u32,
        dst_y: u32,
        dst_w: u32,
        dst_h: u32,
        opacity: u32,
        radius: u32,
        blend: u32,
    ) -> Result<(), GfxError> {
        self.composite_init()?;
        if dst_w == 0 || dst_h == 0 || src_w == 0 || src_h == 0 || tex_w == 0 || tex_h == 0 {
            return Err(GfxError::InvalidArgument);
        }
        let r = radius.min(dst_w / 2).min(dst_h / 2) as f32;
        let cx = dst_x as f32 + dst_w as f32 / 2.0;
        let cy = dst_y as f32 + dst_h as f32 / 2.0;
        let bx = dst_w as f32 / 2.0 - r;
        let by = dst_h as f32 / 2.0 - r;
        // Per-dest-pixel UV advance: the src texel span over the dest pixel span.
        let step_u = src_w as f32 / (tex_w as f32 * dst_w as f32);
        let step_v = src_h as f32 / (tex_h as f32 * dst_h as f32);
        // UV at the dst origin = the src origin (in normalized texels).
        let uv_off_x = src_x as f32 / tex_w as f32 - dst_x as f32 * step_u;
        let uv_off_y = src_row as f32 / tex_h as f32 - dst_y as f32 * step_v;
        let opacity01 = (opacity.min(255) as f32) / 255.0;

        let mut s = Submit3d::new();
        s.emit_bind_object(VIRGL_OBJECT_BLEND, blend);
        s.emit_bind_object(VIRGL_OBJECT_DSA, H_DSA);
        s.emit_bind_object(VIRGL_OBJECT_RASTERIZER, H_RAST);
        s.emit_bind_object(VIRGL_OBJECT_VERTEX_ELEMENTS, H_VE);
        s.emit_set_framebuffer_state(0, &[target_surface]);
        s.emit_set_viewport_box(dst_x as f32, dst_y as f32, dst_w as f32, dst_h as f32);
        s.emit_set_sampler_views(PIPE_SHADER_FRAGMENT, 0, &[content_sview]);
        s.emit_bind_sampler_states(PIPE_SHADER_FRAGMENT, 0, &[H_SAMPLER]);
        s.emit_set_constant_buffer(
            PIPE_SHADER_FRAGMENT,
            &[
                step_u, step_v, uv_off_x, uv_off_y, //
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
            let (fb, fb_len, fb_w, _row) = self.scanout_fb().ok_or(GfxError::DeviceNotFound)?;
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
    /// and NO `0xF8` backdrop transfer (the RT is the final surface). Glass layers
    /// get their backdrop from `blur_rt_backdrop` (destination-so-far RT snapshot
    /// + GPU blur), run by `composite_pending_rt_layers` before this composite.
    /// Coordinates are screen-space (0..SCREEN_H), matching the RT.
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
        // Sync the content from the VMO atlas into the GL atlas texture — only
        // when it changed. The atlas texture persists across presents, so a
        // cursor-move present re-composites from it WITHOUT this transfer (the
        // per-frame transfer was the mouse-move slowdown).
        let src_row_rel = src_row_abs.saturating_sub(ATLAS_ROW);
        // Source region = the content band when a sub-size is given, else the
        // whole layer. Only the source region is transferred to the GL texture.
        let (src_w, src_h) = if content_w > 0 { (content_w, content_h) } else { (width, height) };
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
        // Composite the content straight onto the scanout RT (alpha-over base).
        if src_w != width || src_h != height {
            // Live resize: bilinear-scale the band source up to the frame dest —
            // the window body grows; snaps sharp when the client re-renders.
            self.submit_layer_pass_scaled(
                self.rt_back_surface(),
                ATLAS_SVIEW,
                1280,
                ATLAS_ROWS,
                src_x,
                src_row_rel,
                src_w,
                src_h,
                dst_x,
                dst_y,
                width,
                height,
                opacity,
                corner_radius,
                H_BLEND_ALPHA,
            )
        } else {
            self.submit_layer_pass(
                self.rt_back_surface(),
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
    }

    /// Upload the stored cursor sprite (BGRA, set by `store_cursor_sprite` when
    /// windowd arms the cursor) into its own GL sampler texture, and pre-warm the
    /// layer shader. One-shot. Returns true once a cursor texture is ready to
    /// sample. Issues create/attach/transfer ctrl commands — call OUTSIDE a present
    /// batch (like the wallpaper upload), never interleaved with batched draws.
    pub(crate) fn cursor_tex_init(&mut self) -> Result<bool, GfxError> {
        let w = self.cursor_sprite_w;
        let h = self.cursor_sprite_h;
        if w == 0 || h == 0 || self.cursor_sprite.is_empty() {
            return Ok(false); // windowd hasn't uploaded the sprite yet
        }
        if self.cursor_tex_va != 0 {
            return Ok(true); // already uploaded
        }
        // Pre-warm the layer shader/blend/sampler outside any batch so its
        // one-time CREATE_OBJECTs are validated here, not inside a present batch.
        self.composite_init()?;
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
        if !dst.is_null() && self.cursor_sprite.len() >= byte_len {
            unsafe {
                core::ptr::copy_nonoverlapping(self.cursor_sprite.as_ptr(), dst, byte_len);
            }
        }
        self.virgl_transfer_to_host(H_CURSOR_TEX, 0, 0, w, h, w * 4)?;
        let mut s = Submit3d::new();
        s.emit_create_sampler_view(H_CURSOR_SVIEW, H_CURSOR_TEX, PIPE_FORMAT_B8G8R8A8_UNORM);
        self.submit_composite_stream(&s)?;
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
        let w = self.cursor_sprite_w;
        let h = self.cursor_sprite_h;
        if w != self.cursor_tex_w || h != self.cursor_tex_h {
            return Ok(()); // dims fixed at init — keep the old shape visible
        }
        let byte_len = (w as usize) * (h as usize) * 4;
        let dst = self.cursor_tex_va as *mut u8;
        if !dst.is_null() && self.cursor_sprite.len() >= byte_len {
            unsafe {
                core::ptr::copy_nonoverlapping(self.cursor_sprite.as_ptr(), dst, byte_len);
            }
        }
        self.virgl_transfer_to_host(H_CURSOR_TEX, 0, 0, w, h, w * 4)?;
        Ok(())
    }

    /// Composite the cursor sprite as a layer onto the scanout RT at (`dst_x`,
    /// `dst_y`), alpha-blended (the sprite's own alpha shapes the arrow). Reuses
    /// the generic `submit_layer_pass`; safe inside the present batch (the shader
    /// and texture were created in `cursor_tex_init` outside it). No-op until the
    /// sprite is uploaded.
    pub(crate) fn composite_cursor_rt(&mut self, dst_x: u32, dst_y: u32) -> Result<(), GfxError> {
        if self.cursor_tex_va == 0 {
            return Ok(());
        }
        let (w, h) = (self.cursor_tex_w, self.cursor_tex_h);
        // The cursor sprite is PREMULTIPLIED (nexus-svg) → premult-over blend, so
        // its anti-aliased edges don't get a dark fringe (straight-alpha would
        // multiply by alpha twice). Icon/atlas layers stay on H_BLEND_ALPHA.
        self.composite_sprite_rt(H_CURSOR_SVIEW, w, h, dst_x, dst_y, 255, 0, H_BLEND_PREMUL)
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
        self.composite_init()?;
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
        self.submit_composite_stream(&s)?;
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
    /// from the (possibly 2×/supersampled) texture down to its on-screen size.
    /// No-op until the sprite is uploaded. Square corners, fully opaque.
    pub(crate) fn composite_icon_rt(&mut self) -> Result<(), GfxError> {
        if self.icon_tex_va == 0 {
            return Ok(());
        }
        let (tw, th) = (self.icon_tex_w, self.icon_tex_h);
        let (dx, dy) = (self.icon_dst_x, self.icon_dst_y);
        let dw = if self.icon_dst_w == 0 { tw } else { self.icon_dst_w };
        let dh = if self.icon_dst_h == 0 { th } else { self.icon_dst_h };
        self.submit_layer_pass_scaled(
            self.rt_back_surface(),
            H_ICON_SVIEW,
            tw,
            th,
            0,
            0,
            tw,
            th,
            dx,
            dy,
            dw,
            dh,
            255,
            0,
            // Icon sprite is un-premultiplied (straight alpha) → standard blend.
            H_BLEND_ALPHA,
        )
    }

    /// Composite an uploaded BGRA sprite (`content_sview`, a `tex_w×tex_h`
    /// texture) as a layer onto the scanout RT at (`dst_x`,`dst_y`), alpha-blended
    /// via the generic `submit_layer_pass`. The reusable sprite-layer entry — the
    /// cursor uses it, and icons/other sprites can too (each its own texture), so
    /// no sprite gets a bespoke draw path.
    pub(crate) fn composite_sprite_rt(
        &mut self,
        content_sview: u32,
        tex_w: u32,
        tex_h: u32,
        dst_x: u32,
        dst_y: u32,
        opacity: u32,
        radius: u32,
        blend: u32,
    ) -> Result<(), GfxError> {
        self.submit_layer_pass_scaled(
            self.rt_back_surface(),
            content_sview,
            tex_w,
            tex_h,
            0,
            0,
            tex_w,
            tex_h,
            dst_x,
            dst_y,
            tex_w,
            tex_h,
            opacity,
            radius,
            blend,
        )
    }

    fn submit_composite_stream(&mut self, s: &Submit3d) -> Result<(), GfxError> {
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, bytes)
    }
}
