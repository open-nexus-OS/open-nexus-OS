// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The blur golden on the virtual GPU (ADR-0094, TASK-0351 P7): at bringup, after the
//! layer-composite proof, the library's blur chain is run at every library radius over the
//! oracle's backdrop (`nexus_gfx::frame::oracle` — the same scene, reads, expected pixels and
//! tolerance the board's `gpu blur radii` rung uses) and read back: the horizontal pass into a
//! reduced scratch, the vertical pass with the glass's saturation into a cache, the rounded
//! layer sampling the cache back over the glass, as `glass_blur.rs` draws a frame's glass. One
//! line, `gpud: gpu blur golden ok (r8=… r20=… r30=… r40=… tolerance=…)`, the largest deviation
//! per radius in levels — or its FAIL form naming the first read outside the tolerance. The
//! raster is held to the same oracle on the host (`nexus-gfx/tests/frame_oracle.rs`): three
//! executors, one picture.
//! OWNERS: @ui @gpu
//! STATUS: Functional (a bringup proof; the visible lanes require its line)
//! API_STABILITY: Internal
//! TEST_COVERAGE: `just ci-os-visible` (the marker); the oracle's numbers on the host
//! INVARIANTS: bringup-only resources and handles (the context's one object table); no verdict
//!   without a readback.

#![cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]

use alloc::format;
use alloc::string::String;

use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::gpu::quads::Quad;
use nexus_gfx::frame::oracle::{
    backdrop_rect, backdrop_row, expected, tolerance, CORNER, GLASS, READS, SATURATION,
};
use nexus_gfx::frame::{blur_scale, BLUR_RADII};

use crate::backend::virgl3d::rt_pixel;
use crate::backend::VirtioGpuBackend;
use crate::virgl::{Submit3d, PIPE_CLEAR_COLOR0, PIPE_FORMAT_B8G8R8A8_UNORM};

// Bringup-only resources (the backdrop and the destination carry guest backings) and handles.
const BACKDROP_RES: u32 = 0xD0;
const TMP_RES: u32 = 0xD1;
const CACHE_RES: u32 = 0xD2;
const DEST_RES: u32 = 0xD3;
const BACKDROP_SVIEW: u32 = 0x70;
const TMP_SVIEW: u32 = 0x71;
const CACHE_SVIEW: u32 = 0x72;
const TMP_SURF: u32 = 0x73;
const CACHE_SURF: u32 = 0x74;
const DEST_SURF: u32 = 0x75;
/// The destination's clear (RGBA): what the glass's corner pixel keeps.
const CLEAR: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
const CLEAR_BGR: [u8; 3] = [255, 0, 0];

impl VirtioGpuBackend {
    /// The golden at every library radius: its line printed, `Err` only when the device refused
    /// a command (then no verdict).
    pub(crate) fn virgl_blur_golden(&mut self) -> Result<(), GfxError> {
        if !self.virgl_capable || !self.virgl_draw_ok || !self.pipelines.ready {
            return Err(GfxError::DeviceNotFound);
        }
        let (x0, y0, w, h) = backdrop_rect();
        let bytes = (w * h * 4) as usize;
        for res in [BACKDROP_RES, TMP_RES, CACHE_RES, DEST_RES] {
            self.virgl_create_rt(res, w, h)?;
        }
        let backdrop_va = self.virgl_attach_backing(BACKDROP_RES, bytes)?;
        let dest_va = self.virgl_attach_backing(DEST_RES, bytes)?;
        // The oracle's backdrop into the texture.
        // SAFETY: `virgl_attach_backing` mapped `bytes` of fresh, writable guest memory at
        // `backdrop_va` for this resource; nothing else refers to it.
        let backing = unsafe { core::slice::from_raw_parts_mut(backdrop_va as *mut u8, bytes) };
        for (i, row) in backing.chunks_exact_mut((w * 4) as usize).enumerate() {
            if backdrop_row(y0 + i as u32, row) != row.len() {
                return Err(GfxError::InvalidArgument);
            }
        }
        self.virgl_transfer_to_host(BACKDROP_RES, 0, 0, w, h, w * 4)?;

        let mut s = Submit3d::new();
        s.emit_create_sampler_view(BACKDROP_SVIEW, BACKDROP_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_sampler_view(TMP_SVIEW, TMP_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_sampler_view(CACHE_SVIEW, CACHE_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_surface(TMP_SURF, TMP_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_surface(CACHE_SURF, CACHE_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
        s.emit_create_surface(DEST_SURF, DEST_RES, PIPE_FORMAT_B8G8R8A8_UNORM);
        self.submit3d(&s)?;

        let mut line = String::from("gpud: gpu blur golden ok (");
        for radius in BLUR_RADII {
            match self.golden_at(radius, (x0, y0, w, h), dest_va)? {
                Ok(deviation) => line.push_str(&format!("r{radius}={deviation} ")),
                Err(why) => {
                    let _ = nexus_abi::debug_println(&format!(
                        "gpud: FAIL gpu blur golden (r{radius} {why} tolerance={})",
                        tolerance(radius)
                    ));
                    return Ok(());
                }
            }
        }
        line.push_str(&format!(
            "tolerance={}/{}/{}/{})",
            tolerance(BLUR_RADII[0]),
            tolerance(BLUR_RADII[1]),
            tolerance(BLUR_RADII[2]),
            tolerance(BLUR_RADII[3])
        ));
        let _ = nexus_abi::debug_println(&line);
        Ok(())
    }

    /// The chain at `radius` as `glass_blur.rs` draws it, over the backdrop texture whose origin
    /// is the plane pixel `(x0, y0)`; the reads against the oracle: the largest deviation in
    /// levels, or the first read outside the tolerance.
    fn golden_at(
        &mut self,
        radius: u32,
        (x0, y0, w, h): (u32, u32, u32, u32),
        dest_va: usize,
    ) -> Result<Result<u8, String>, GfxError> {
        let Some(pair) = self.pipelines.blur(radius) else {
            return Ok(Err(String::from("no pipeline pair")));
        };
        let (gx, gy, gw, gh) = (GLASS.0 - x0, GLASS.1 - y0, GLASS.2, GLASS.3);
        let sc = blur_scale(radius);
        let taps = radius / sc;
        let (vw, vh) = (gw.div_ceil(sc) + 2, gh.div_ceil(sc) + 2);
        let sf = sc as f32;
        let (fw, fh) = (w as f32, h as f32);
        let (ox, oy) = (gx as f32 - sf, gy as f32 - sf);
        let hh = vh + 2 * taps;
        let halo = (sc * taps) as f32;
        let mut s = Submit3d::new();
        // Horizontal: a reduced pixel's centre is the corner of the four backdrop texels it
        // averages, `scale · taps` full rows above the result on; a tap `scale` texels across.
        let source = [sf * vw as f32 / fw, sf * hh as f32 / fh, ox / fw, (oy - halo) / fh];
        let q = Quad::blur(0.0, 0.0, vw as f32, hh as f32, source, [sf / fw, 0.0], 1.0);
        self.emit_draw(&mut s, pair.horizontal, TMP_SURF, &q, BACKDROP_SVIEW);
        // Vertical: the horizontal result's texel under the output pixel, `taps` rows down its
        // halo; a tap one row down; the glass's saturation.
        let source = [vw as f32 / fw, vh as f32 / fh, 0.0, taps as f32 / fh];
        let q = Quad::blur(0.0, 0.0, vw as f32, vh as f32, source, [0.0, 1.0 / fh], SATURATION);
        self.emit_draw(&mut s, pair.vertical, CACHE_SURF, &q, TMP_SVIEW);
        // The glass: the cache's rectangle inside its margin, sampled back up over the glass
        // with its corners cut, onto the cleared destination.
        s.emit_set_framebuffer_state(0, &[DEST_SURF]);
        s.emit_set_viewport_box(0.0, 0.0, fw, fh);
        s.emit_clear(PIPE_CLEAR_COLOR0, CLEAR, 1.0, 0);
        let q = Quad::layer(gx as f32, gy as f32, gw as f32, gh as f32, [1.0; 4], CORNER)
            .with_texture([gw as f32 / sf / fw, gh as f32 / sf / fh, 1.0 / fw, 1.0 / fh]);
        self.emit_draw(&mut s, self.pipelines.layer_atlas, DEST_SURF, &q, CACHE_SVIEW);
        self.submit3d(&s)?;
        self.virgl_transfer_from_host(DEST_RES, 0, 0, w, h, w * 4)?;
        let mut deviation = 0u8;
        for (i, (x, y)) in READS.iter().enumerate() {
            let want = if i + 1 == READS.len() { CLEAR_BGR } else { expected((*x, *y), radius) };
            let px = rt_pixel(dest_va, w, x - x0, y - y0);
            let d = px.iter().zip(&want).map(|(p, w)| p.abs_diff(*w)).max().unwrap_or(0);
            if d > tolerance(radius) {
                return Ok(Err(format!(
                    "at={x},{y} read={:02x}{:02x}{:02x} want={:02x}{:02x}{:02x}",
                    px[0], px[1], px[2], want[0], want[1], want[2]
                )));
            }
            deviation = deviation.max(d);
        }
        Ok(Ok(deviation))
    }
}
