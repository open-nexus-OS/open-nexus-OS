// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The layer-composite BRINGUP proof (`gpud: layer composite ok`),
//! split out of `virgl_composite.rs` (module-size ratchet): the live
//! composite paths stay there, the one-shot readback selftest lives here. It
//! draws with the library's client-surface pipeline (`layer-atlas`), so it
//! proves a textured program of our toolchain samples where it should.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: exercised at gpud bringup (QEMU smoke marker).

#![cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]

use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::gpu::quads::Quad;

use crate::backend::virgl3d::rt_pixel;
use crate::backend::VirtioGpuBackend;
use crate::virgl::{Submit3d, PIPE_CLEAR_COLOR0, PIPE_FORMAT_B8G8R8A8_UNORM};

// Bringup-only resources and handles (the context's one object table: free elsewhere).
const CONTENT_RES: u32 = 0xEA;
const DEST_RES: u32 = 0xEB;
const CONTENT_SURF: u32 = 0x55;
const DEST_SURF: u32 = 0x56;
const CONTENT_SVIEW: u32 = 0x57;

impl VirtioGpuBackend {
    /// Bringup proof: composite a red content layer onto a blue dest RT and read
    /// back — center must be red (layer), a corner must stay blue (outside the
    /// layer). Returns true on success.
    pub(crate) fn virgl_composite_selftest(&mut self) -> Result<bool, GfxError> {
        if !self.virgl_capable || !self.virgl_draw_ok || !self.pipelines.ready {
            return Err(GfxError::DeviceNotFound);
        }
        self.virgl_create_rt(CONTENT_RES, 64, 64)?;
        self.virgl_create_rt(DEST_RES, 128, 128)?;
        let dest_va = self.virgl_attach_backing(DEST_RES, 128 * 128 * 4)?;

        // Surfaces + content sampler view; clear content=red, dest=blue.
        let mut s = Submit3d::new();
        s.emit_create_surface(CONTENT_SURF, CONTENT_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_surface(DEST_SURF, DEST_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_sampler_view(CONTENT_SVIEW, CONTENT_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_set_framebuffer_state(0, &[CONTENT_SURF]);
        s.emit_set_viewport_box(0.0, 0.0, 64.0, 64.0);
        s.emit_clear(PIPE_CLEAR_COLOR0, [1.0, 0.0, 0.0, 1.0], 1.0, 0); // red (RGBA clear)
        s.emit_set_framebuffer_state(0, &[DEST_SURF]);
        s.emit_set_viewport_box(0.0, 0.0, 128.0, 128.0);
        s.emit_clear(PIPE_CLEAR_COLOR0, [0.0, 0.0, 1.0, 1.0], 1.0, 0); // blue
                                                                       // The red content as a 64×64 layer at (32,32): all of its texture, opaque, square.
        let q =
            Quad::layer(32.0, 32.0, 64.0, 64.0, [1.0; 4], 0.0).with_texture([1.0, 1.0, 0.0, 0.0]);
        self.emit_draw(&mut s, self.pipelines.layer_atlas, DEST_SURF, &q, CONTENT_SVIEW);
        self.submit3d(&s)?;

        // Read back the dest RT and inspect: center red, corner blue.
        self.virgl_transfer_from_host(DEST_RES, 0, 0, 128, 128, 128 * 4)?;
        // BGRA: red = [0,0,255,255], blue = [255,0,0,255].
        let center = rt_pixel(dest_va, 128, 64, 64);
        let corner = rt_pixel(dest_va, 128, 8, 8);
        let center_red = center[2] > 200 && center[0] < 64;
        let corner_blue = corner[0] > 200 && corner[2] < 64;
        Ok(center_red && corner_blue)
    }
}
