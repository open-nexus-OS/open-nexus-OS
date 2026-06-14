// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: GPU vector pipeline (compositor stage G3 / M1b-c): SDF rounded
//! rects with per-pixel gradients and soft drop shadows, executed as virgl
//! fragment-shader draws over the display texture.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0063-ui-v5b-scene-graph-gpu-pipeline-virtual-list-theme-contract.md
//!
//! Execution model (the blur pattern): TRANSFER_TO_HOST syncs the affected
//! region from the windowd VMO into the display texture (0xF8), the shader
//! pass renders with analytic-SDF coverage and alpha blending, and
//! TRANSFER_FROM_HOST lands the pixels back in the scanned-out VMO. Resolution
//! independent by construction — the SDF is evaluated per fragment, so the
//! same commands stay sharp at any DPI (5K-ready).
//!
//! DriverKit boundary: this file is virtio/virgl command encoding only. The
//! portable contract is the `Command::{FillSdfGradient, DropShadow}` pair —
//! a real-GPU backend reimplements the submit fns against its own ISA.

#![cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]

use nexus_gfx::backend::error::GfxError;
use nexus_gfx::command::buffer::RgbaColor;

use crate::backend::VirtioGpuBackend;
use crate::protocol::{VirtioGpuSubmit3d, VIRTIO_GPU_CMD_SUBMIT_3D};
use crate::virgl::{
    Submit3d, PIPE_PRIM_TRIANGLES, PIPE_SHADER_FRAGMENT, PIPE_SHADER_VERTEX, VIRGL_OBJECT_BLEND,
    VIRGL_OBJECT_DSA, VIRGL_OBJECT_RASTERIZER, VIRGL_OBJECT_VERTEX_ELEMENTS,
};

/// Handle namespace (see backend.rs gradient-selftest comment): 18/19 vector
/// fragment shaders, 0x60 alpha-blend state. VS 10 + state 0x20..0x23 persist
/// from the boot draw self-test; FBSRC surface 0x30 + quad 0xFA from blur init.
const H_FS_SDF_GRAD: u32 = 18;
const H_FS_SHADOW: u32 = 19;
const H_BLEND_ALPHA: u32 = 0x60;
const H_FBSRC_SURF: u32 = 0x30;
const QUAD_RES: u32 = 0xFA;

const SCREEN_W: u32 = 1280;
const SCREEN_H: u32 = 800;
const FB_STRIDE: u32 = SCREEN_W * 4;
/// Display rows start at this absolute fb row; the display texture (0xF8)
/// aliases rows DISPLAY_ROW..DISPLAY_ROW+1600.
const DISPLAY_ROW: u32 = 1600;

/// SDF rounded-rect + vertical gradient + analytic-AA coverage.
/// CONST[0] = (-cx, -cy, bx, by): negated rect center, half-extents minus r.
/// CONST[1] = (radius, 1/rect_h, -rect_y, 0).
/// CONST[2] = top RGBA, CONST[3] = bottom RGBA (straight alpha 0..1).
/// Coverage = clamp(0.5 - d, 0, 1) — exact 1px analytic edge AA.
const FS_SDF_GRAD: &str = "FRAG\n\
    DCL IN[0], POSITION, LINEAR\n\
    DCL OUT[0], COLOR\n\
    DCL CONST[0..3]\n\
    DCL TEMP[0..4]\n\
    IMM[0] FLT32 { 0.5000, 0.0000, 1.0000, 0.0000}\n\
    ADD TEMP[0].xy, IN[0].xyyy, CONST[0].xyyy\n\
    MAX TEMP[0].xy, TEMP[0].xyyy, -TEMP[0].xyyy\n\
    ADD TEMP[1].xy, TEMP[0].xyyy, -CONST[0].zwww\n\
    MAX TEMP[1].xy, TEMP[1].xyyy, IMM[0].yyyy\n\
    DP2 TEMP[2].x, TEMP[1].xyyy, TEMP[1].xyyy\n\
    SQRT TEMP[2].x, TEMP[2].xxxx\n\
    ADD TEMP[2].x, TEMP[2].xxxx, -CONST[1].xxxx\n\
    ADD TEMP[3].x, IMM[0].xxxx, -TEMP[2].xxxx\n\
    MAX TEMP[3].x, TEMP[3].xxxx, IMM[0].yyyy\n\
    MIN TEMP[3].x, TEMP[3].xxxx, IMM[0].zzzz\n\
    ADD TEMP[4].x, IN[0].yyyy, CONST[1].zzzz\n\
    MUL TEMP[4].x, TEMP[4].xxxx, CONST[1].yyyy\n\
    LRP TEMP[0], TEMP[4].xxxx, CONST[3], CONST[2]\n\
    MUL TEMP[0].w, TEMP[0].wwww, TEMP[3].xxxx\n\
    MOV OUT[0], TEMP[0]\n\
    END\n";

/// Soft drop shadow: alpha = color.a · (1 - clamp(d/blur, 0, 1))², a
/// quadratic SDF falloff (visually close to a gaussian penumbra, exact at
/// d≤0 → full shadow under the shape).
/// CONST[0] = (-cx, -cy, bx, by) with the shadow offset already applied.
/// CONST[1] = (radius, 1/blur, 0, 0). CONST[2] = shadow RGBA.
const FS_SHADOW: &str = "FRAG\n\
    DCL IN[0], POSITION, LINEAR\n\
    DCL OUT[0], COLOR\n\
    DCL CONST[0..2]\n\
    DCL TEMP[0..3]\n\
    IMM[0] FLT32 { 0.5000, 0.0000, 1.0000, 0.0000}\n\
    ADD TEMP[0].xy, IN[0].xyyy, CONST[0].xyyy\n\
    MAX TEMP[0].xy, TEMP[0].xyyy, -TEMP[0].xyyy\n\
    ADD TEMP[1].xy, TEMP[0].xyyy, -CONST[0].zwww\n\
    MAX TEMP[1].xy, TEMP[1].xyyy, IMM[0].yyyy\n\
    DP2 TEMP[2].x, TEMP[1].xyyy, TEMP[1].xyyy\n\
    SQRT TEMP[2].x, TEMP[2].xxxx\n\
    ADD TEMP[2].x, TEMP[2].xxxx, -CONST[1].xxxx\n\
    MUL TEMP[2].x, TEMP[2].xxxx, CONST[1].yyyy\n\
    ADD TEMP[3].x, IMM[0].zzzz, -TEMP[2].xxxx\n\
    MAX TEMP[3].x, TEMP[3].xxxx, IMM[0].yyyy\n\
    MIN TEMP[3].x, TEMP[3].xxxx, IMM[0].zzzz\n\
    MUL TEMP[3].x, TEMP[3].xxxx, TEMP[3].xxxx\n\
    MOV TEMP[0], CONST[2]\n\
    MUL TEMP[0].w, CONST[2].wwww, TEMP[3].xxxx\n\
    MOV OUT[0], TEMP[0]\n\
    END\n";

impl VirtioGpuBackend {
    /// Lazily create the vector shaders + alpha-blend state. Requires the
    /// blur pipeline objects (display texture, quad VBO) — created on demand.
    fn virgl_vector_init(&mut self) -> Result<(), GfxError> {
        if self.virgl_vector_ready {
            return Ok(());
        }
        if !self.virgl_blur_ready {
            self.virgl_blur_init()?;
        }
        let mut s = Submit3d::new();
        s.emit_create_shader(H_FS_SDF_GRAD, PIPE_SHADER_FRAGMENT, FS_SDF_GRAD);
        s.emit_create_shader(H_FS_SHADOW, PIPE_SHADER_FRAGMENT, FS_SHADOW);
        s.emit_create_blend_alpha(H_BLEND_ALPHA);
        self.submit_vector_stream(&s)?;
        self.virgl_vector_ready = true;
        Ok(())
    }

    /// GPU SDF rounded-rect fill with a vertical linear gradient.
    /// `y_abs` is the absolute fb row (display offset already applied).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn submit_virgl_sdf_gradient(
        &mut self,
        x: u32,
        y_abs: u32,
        w: u32,
        h: u32,
        radius: u32,
        top: RgbaColor,
        bottom: RgbaColor,
    ) -> Result<(), GfxError> {
        let (x, y_rel, w, h) = clamp_display_region(x, y_abs, w, h)?;
        if !self.virgl_capable || !self.virgl_draw_ok {
            return Err(GfxError::DeviceNotFound);
        }
        self.virgl_vector_init()?;

        // Backdrop must be current in the GL texture (translucent fills blend).
        self.virgl_transfer_to_host(0xF8, x, y_rel, w, h, FB_STRIDE)?;

        let r = (radius.min(w / 2).min(h / 2)) as f32;
        let cx = x as f32 + w as f32 / 2.0;
        let cy = y_rel as f32 + h as f32 / 2.0;
        let bx = w as f32 / 2.0 - r;
        let by = h as f32 / 2.0 - r;
        let tc = rgba_f32(top);
        let bc = rgba_f32(bottom);

        let mut s = Submit3d::new();
        self.emit_vector_state(&mut s, x, y_rel, w, h);
        s.emit_set_constant_buffer(
            PIPE_SHADER_FRAGMENT,
            &[
                -cx, -cy, bx, by, //
                r, 1.0 / (h as f32).max(1.0), -(y_rel as f32), 0.0, //
                tc[0], tc[1], tc[2], tc[3], //
                bc[0], bc[1], bc[2], bc[3],
            ],
        );
        s.emit_bind_shader(H_FS_SDF_GRAD, PIPE_SHADER_FRAGMENT);
        s.emit_draw_vbo(0, 6, PIPE_PRIM_TRIANGLES);
        self.submit_vector_stream(&s)?;

        // Land the result back in the scanned-out VMO.
        self.virgl_transfer_from_host(0xF8, x, y_rel, w, h, FB_STRIDE)
    }

    /// GPU soft drop shadow for a rounded rect. `(x, y_abs, w, h)` is the
    /// casting shape; the drawn region is the shape expanded by `blur` and
    /// shifted by the offset, clamped to the display plane.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn submit_virgl_drop_shadow(
        &mut self,
        x: u32,
        y_abs: u32,
        w: u32,
        h: u32,
        radius: u32,
        blur: u32,
        offset_x: i32,
        offset_y: i32,
        color: RgbaColor,
    ) -> Result<(), GfxError> {
        if !self.virgl_capable || !self.virgl_draw_ok {
            return Err(GfxError::DeviceNotFound);
        }
        if y_abs < DISPLAY_ROW || blur == 0 || w == 0 || h == 0 {
            return Err(GfxError::InvalidArgument);
        }
        self.virgl_vector_init()?;

        let y = y_abs - DISPLAY_ROW;
        // Shadow extent: shape shifted by offset, padded by blur.
        let sx0 = (x as i32 + offset_x - blur as i32).max(0) as u32;
        let sy0 = (y as i32 + offset_y - blur as i32).max(0) as u32;
        let sx1 = ((x + w) as i32 + offset_x + blur as i32).min(SCREEN_W as i32) as u32;
        let sy1 = ((y + h) as i32 + offset_y + blur as i32).min(SCREEN_H as i32) as u32;
        if sx0 >= sx1 || sy0 >= sy1 {
            return Ok(()); // fully clipped
        }
        let (rw, rh) = (sx1 - sx0, sy1 - sy0);

        self.virgl_transfer_to_host(0xF8, sx0, sy0, rw, rh, FB_STRIDE)?;

        let r = (radius.min(w / 2).min(h / 2)) as f32;
        let cx = x as f32 + offset_x as f32 + w as f32 / 2.0;
        let cy = y as f32 + offset_y as f32 + h as f32 / 2.0;
        let bx = w as f32 / 2.0 - r;
        let by = h as f32 / 2.0 - r;
        let c = rgba_f32(color);

        let mut s = Submit3d::new();
        self.emit_vector_state(&mut s, sx0, sy0, rw, rh);
        s.emit_set_constant_buffer(
            PIPE_SHADER_FRAGMENT,
            &[
                -cx, -cy, bx, by, //
                r, 1.0 / (blur as f32), 0.0, 0.0, //
                c[0], c[1], c[2], c[3],
            ],
        );
        s.emit_bind_shader(H_FS_SHADOW, PIPE_SHADER_FRAGMENT);
        s.emit_draw_vbo(0, 6, PIPE_PRIM_TRIANGLES);
        self.submit_vector_stream(&s)?;

        self.virgl_transfer_from_host(0xF8, sx0, sy0, rw, rh, FB_STRIDE)
    }

    /// Common pass state: explicit binds (state is context-global), display
    /// surface as target, viewport = affected box, alpha blending, quad VBO.
    fn emit_vector_state(&mut self, s: &mut Submit3d, x: u32, y: u32, w: u32, h: u32) {
        self.emit_vector_state_to(s, H_FBSRC_SURF, x, y, w, h);
    }

    /// Like [`emit_vector_state`] but renders into an explicit target surface.
    /// Used by the RT-direct path to draw vector passes straight onto the scanout
    /// render target instead of the VMO display-plane surface.
    fn emit_vector_state_to(
        &mut self,
        s: &mut Submit3d,
        target_surface: u32,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    ) {
        s.emit_bind_object(VIRGL_OBJECT_BLEND, H_BLEND_ALPHA);
        s.emit_bind_object(VIRGL_OBJECT_DSA, 0x21);
        s.emit_bind_object(VIRGL_OBJECT_RASTERIZER, 0x22);
        s.emit_bind_object(VIRGL_OBJECT_VERTEX_ELEMENTS, 0x23);
        s.emit_set_framebuffer_state(0, &[target_surface]);
        s.emit_set_viewport_box(x as f32, y as f32, w as f32, h as f32);
        s.emit_bind_shader(10, PIPE_SHADER_VERTEX);
        s.emit_set_vertex_buffers(&[(16, 0, QUAD_RES)]);
    }

    /// Drop shadow rendered directly onto the scanout render target (RT-direct
    /// path). Screen-space coordinates (no DISPLAY_ROW), the shadow SDF is
    /// alpha-blended over the base already present on the RT — so unlike
    /// [`submit_virgl_drop_shadow`] there is no `0xF8` backdrop transfer and no
    /// VMO writeback (the RT is the final surface).
    pub(crate) fn submit_drop_shadow_rt(
        &mut self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        radius: u32,
        blur: u32,
        offset_x: i32,
        offset_y: i32,
        color: RgbaColor,
    ) -> Result<(), GfxError> {
        if !self.virgl_capable || !self.virgl_draw_ok || blur == 0 || w == 0 || h == 0 {
            return Err(GfxError::InvalidArgument);
        }
        self.virgl_vector_init()?;
        let sx0 = (x as i32 + offset_x - blur as i32).max(0) as u32;
        let sy0 = (y as i32 + offset_y - blur as i32).max(0) as u32;
        let sx1 = ((x + w) as i32 + offset_x + blur as i32).min(SCREEN_W as i32) as u32;
        let sy1 = ((y + h) as i32 + offset_y + blur as i32).min(SCREEN_H as i32) as u32;
        if sx0 >= sx1 || sy0 >= sy1 {
            return Ok(()); // fully clipped
        }
        let (rw, rh) = (sx1 - sx0, sy1 - sy0);
        let r = (radius.min(w / 2).min(h / 2)) as f32;
        let cx = x as f32 + offset_x as f32 + w as f32 / 2.0;
        let cy = y as f32 + offset_y as f32 + h as f32 / 2.0;
        let bx = w as f32 / 2.0 - r;
        let by = h as f32 / 2.0 - r;
        let c = rgba_f32(color);

        let mut s = Submit3d::new();
        self.emit_vector_state_to(&mut s, crate::gl_scanout::H_GLS_SURF, sx0, sy0, rw, rh);
        s.emit_set_constant_buffer(
            PIPE_SHADER_FRAGMENT,
            &[
                -cx, -cy, bx, by, //
                r, 1.0 / (blur as f32), 0.0, 0.0, //
                c[0], c[1], c[2], c[3],
            ],
        );
        s.emit_bind_shader(H_FS_SHADOW, PIPE_SHADER_FRAGMENT);
        s.emit_draw_vbo(0, 6, PIPE_PRIM_TRIANGLES);
        self.submit_vector_stream(&s)
    }

    fn submit_vector_stream(&mut self, s: &Submit3d) -> Result<(), GfxError> {
        let bytes = s.as_bytes();
        let hdr = VirtioGpuSubmit3d {
            hdr: self.virgl_hdr(VIRTIO_GPU_CMD_SUBMIT_3D),
            size: bytes.len() as u32,
            _padding: 0,
        };
        self.ctrl_submit_header_tail(&hdr, &bytes)
    }
}

/// Convert to shader RGBA floats, matching the CPU path's de-facto channel
/// convention: `blend_pixel_vmo` writes `as_array()[0]` into the framebuffer
/// B byte, so array[0] is BLUE on screen. The shader's red output lands in
/// the fb R byte → feed it array[2].
fn rgba_f32(c: RgbaColor) -> [f32; 4] {
    let a = c.as_array();
    [a[2] as f32 / 255.0, a[1] as f32 / 255.0, a[0] as f32 / 255.0, a[3] as f32 / 255.0]
}

/// Clamp a display-plane region given an absolute fb row; returns
/// (x, y_rel, w, h) in display-texture coordinates.
fn clamp_display_region(x: u32, y_abs: u32, w: u32, h: u32) -> Result<(u32, u32, u32, u32), GfxError> {
    if y_abs < DISPLAY_ROW || w == 0 || h == 0 {
        return Err(GfxError::InvalidArgument);
    }
    let y = y_abs - DISPLAY_ROW;
    if x >= SCREEN_W || y >= SCREEN_H {
        return Err(GfxError::InvalidArgument);
    }
    Ok((x, y, w.min(SCREEN_W - x), h.min(SCREEN_H - y)))
}
