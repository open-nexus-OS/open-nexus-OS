// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The layer-composite BRINGUP proof (`gpud: layer composite ok`),
//! split out of `virgl_composite.rs` (module-size ratchet): the live
//! composite paths stay there, the one-shot readback selftest lives here.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: exercised at gpud bringup (QEMU smoke marker).

#![cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]

use nexus_gfx::backend::error::GfxError;

use crate::backend::VirtioGpuBackend;
use crate::virgl::{Submit3d, PIPE_CLEAR_COLOR0, PIPE_FORMAT_B8G8R8A8_UNORM};
use crate::virgl_composite::{
    ST_CONTENT_RES, ST_CONTENT_SURF, ST_CONTENT_SVIEW, ST_DEST_RES, ST_DEST_SURF,
};

impl VirtioGpuBackend {
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
}
