// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: GPU vector draws (compositor stage G3): a layer's soft drop shadow
//! and a filled rounded rectangle with a vertical gradient (the pointer before
//! its sprite arrives), drawn onto the scanout render target with the
//! library's `shadow` and `gradient` pipelines (`virgl_pipelines`, TASK-0215
//! 5c-2 P6). Resolution independent by construction — the rounded rectangle's
//! distance is evaluated per fragment, so the same commands stay sharp at any
//! DPI.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0063-ui-v5b-scene-graph-gpu-pipeline-virtual-list-theme-contract.md
//!
//! DriverKit boundary: this file is virtio/virgl command encoding only. The
//! portable contract is `Command::CompositeLayer`'s shadow — the board GPU
//! draws the same programs from its own library.

#![cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]

use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::gpu::quads::Quad;
use nexus_gfx::command::buffer::RgbaColor;

use crate::backend::VirtioGpuBackend;

impl VirtioGpuBackend {
    /// A soft drop shadow for the rounded rectangle (`x`, `y`, `w`, `h`), corner `radius`,
    /// blurred over `blur` pixels and moved by the offset — knocked out under the shape itself
    /// (CSS `box-shadow`: never under the element, so a translucent glass panel does not
    /// composite over its own shadow) — over what the frame holds.
    #[allow(clippy::too_many_arguments)]
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
        if !self.pipelines.ready || blur == 0 || w == 0 || h == 0 {
            return Err(GfxError::InvalidArgument);
        }
        let r = radius.min(w / 2).min(h / 2) as f32;
        let offset = [offset_x as f32, offset_y as f32];
        let (fx, fy, fw, fh) = (x as f32, y as f32, w as f32, h as f32);
        let q = Quad::shadow(fx, fy, fw, fh, r, blur as f32, offset, rgba_f32(color));
        self.draw(self.pipelines.shadow, self.rt_back_surface(), &q, 0)
    }

    /// The rectangle (`x`, `y`, `w`, `h`) filled from `top` at its top row to `bottom` at its
    /// bottom one (straight colours), over what the frame holds.
    pub(crate) fn fill_gradient_rt(
        &mut self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        top: RgbaColor,
        bottom: RgbaColor,
    ) -> Result<(), GfxError> {
        if !self.pipelines.ready || w == 0 || h == 0 {
            return Err(GfxError::InvalidArgument);
        }
        let (fx, fy, fw, fh) = (x as f32, y as f32, w as f32, h as f32);
        let q = Quad::gradient(fx, fy, fw, fh, rgba_f32(top), rgba_f32(bottom), 0.0);
        self.draw(self.pipelines.gradient, self.rt_back_surface(), &q, 0)
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
