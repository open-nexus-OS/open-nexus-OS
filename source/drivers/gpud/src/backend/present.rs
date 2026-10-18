// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The present/compositing path: external-framebuffer attach, scanout damage
//! present (host + os variants), committed-buffer present + command execution,
//! the retained render-target layer composite, and the scanout VMO accessor.

use super::resources::{map_nexus_error, validate_rect};
use super::{ResourceRecord, VirtioGpuBackend};
use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::traits::GfxBackend;
use nexus_gfx::backend::types::{Rect, ResourceId};
use nexus_gfx::command::buffer::{Command, CommittedBuffer, RgbaColor};
use nexus_gfx::core::fence::Fence;
use nexus_gfx::core::types::PixelFormat;

#[cfg(all(not(feature = "virgl"), feature = "os-lite", target_os = "none"))]
use super::cpu_frame::Target;
#[cfg(all(feature = "os-lite", target_os = "none"))]
use super::transport::DISPLAY_PLANE_ROW;
#[cfg(all(feature = "os-lite", target_os = "none"))]
use crate::protocol;

impl VirtioGpuBackend {
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn present_scanout_damage(&mut self, rect: Rect) -> Result<(), GfxError> {
        // GL-presented scanout (G1): present = VMO upload + GPU blit + flip.
        #[cfg(feature = "virgl")]
        if self.gl_scanout_active {
            return self.gl_present_damage(rect);
        }
        let scanout = self.scanout_resource.ok_or_else(|| {
            let _ = nexus_abi::debug_println(
                "gpud: backend present_scanout_damage: no scanout_resource",
            );
            GfxError::InvalidArgument
        })?;
        let record = self.find_resource(scanout).ok_or(GfxError::InvalidArgument)?;
        // Display plane is at the fixed row DISPLAY_PLANE_ROW (not height/2 — the
        // resource grew to host the atlas, the display plane did not move).
        let display_rect = Rect {
            x: rect.x,
            y: rect.y + DISPLAY_PLANE_ROW,
            width: rect.width,
            height: rect.height,
        };
        validate_rect(record, display_rect).map_err(|_| {
            let _ = nexus_abi::debug_println("gpud: backend validate_rect FAIL");
            GfxError::InvalidArgument
        })?;
        self.transfer_to_host_os(record, display_rect).map_err(|e| {
            let _ = nexus_abi::debug_println("gpud: backend transfer_to_host_os FAIL");
            e
        })?;
        self.flush_rect_os(record, display_rect).map_err(|e| {
            let _ = nexus_abi::debug_println("gpud: backend flush_rect_os FAIL");
            e
        })?;
        Ok(())
    }

    #[cfg(not(all(feature = "os-lite", target_os = "none")))]
    pub fn present_scanout_damage(&mut self, rect: Rect) -> Result<(), GfxError> {
        let scanout = self.scanout_resource.ok_or(GfxError::InvalidArgument)?;
        self.transfer_to_host(scanout, rect)
    }

    /// The virgl build: the GL scanout composes every frame itself (`gl_scanout`), so the
    /// command stream only brings the retained layer set it composites — its CompositeLayer ops,
    /// kept by the frame planner (`nexus_gfx::frame`: content, shadow and glass onto the RT).
    /// Nothing of the stream is drawn into the VMO.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn execute_commands(&mut self, cmds: &[Command]) -> Result<(), GfxError> {
        // A pointer move brings no layer and keeps the set (every present composes it whole;
        // clearing it would flicker); layers replace it — the scroll overrides with it, the
        // transform overrides kept (windowd retires them itself).
        if self.gl_scanout_active && self.frame.layers.take_frame(cmds).dropped > 0 {
            // NEVER silent: a dropped layer is a missing window or lost chrome on screen.
            let _ = nexus_abi::debug_println("gpud: FAIL rt-layer overflow");
        }
        Ok(())
    }

    /// The 2D path: the CPU executor draws the stream into the scanned-out VMO.
    #[cfg(all(not(feature = "virgl"), feature = "os-lite", target_os = "none"))]
    pub(crate) fn execute_commands(&mut self, cmds: &[Command]) -> Result<(), GfxError> {
        let scanout = self.scanout_resource.ok_or(GfxError::DeviceNotFound)?;
        let record = self.find_resource(scanout).ok_or(GfxError::DeviceNotFound)?;
        if record.backing_va == 0 {
            return Err(GfxError::MmioFault);
        }
        // SAFETY: the scanout resource's backing is this service's own mapping of
        // `backing_len` bytes, kept for the resource's life; the commands run one at a time.
        let target = unsafe {
            Target::new(record.backing_va as *mut u8, record.backing_len, record.width as usize)
        };
        for cmd in cmds {
            self.cpu.execute(&target, cmd)?;
        }
        Ok(())
    }

    /// Composite the retained layer set straight onto the scanout RT, over the wallpaper the
    /// frame drew first and before the flip: the frame planner's steps (`nexus_gfx::frame`) —
    /// per layer, back to front, its glass (the cache's read, the blur into its rows, or the blur
    /// alone), its shadow, its content. The atlas's content is uploaded to the GL texture only
    /// when its generation changed: a pointer move or a window drag composes from the uploaded
    /// texture (the per-frame transfer train was the slow present).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn composite_pending_rt_layers(&mut self) {
        let display = (self.display_w, self.display_h);
        let pipelines = &self.pipelines;
        let planned = self.frame.plan(display, &|radius| pipelines.blur(radius).is_some());
        for i in 0..planned.layers {
            let Some(step) = self.frame.step(i) else { break };
            let l = step.layer;
            if let Some(glass) = step.glass {
                self.glass_draw(&l, glass);
            }
            // A content band smaller than the layer (a live resize) is spread over it; the window
            // snaps sharp when the client renders at the new size.
            let ok = self
                .composite_layer_rt(
                    l.src_row_abs,
                    l.src_x,
                    l.width,
                    l.height,
                    l.content_w,
                    l.content_h,
                    l.dst_x,
                    l.dst_y,
                    l.opacity,
                    l.corner_radius,
                    l.shadow_blur,
                    l.shadow_offset_y,
                    l.shadow_alpha,
                    l.scroll_band_top_abs,
                    l.scroll_band_h,
                    planned.upload,
                )
                .is_ok();
            if ok && !self.virgl_layer_marker_done {
                self.virgl_layer_marker_done = true;
                let _ = nexus_abi::debug_println(crate::markers::GPUD_LAYER_COMPOSITE_LIVE);
            }
        }
    }

    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn scanout_fb(&self) -> Option<(*mut u8, usize, usize, u32)> {
        let scanout = self.scanout_resource?;
        let record = self.find_resource(scanout)?;
        if record.backing_va == 0 {
            return None;
        }
        Some((
            record.backing_va as *mut u8,
            record.backing_len,
            record.width as usize,
            DISPLAY_PLANE_ROW,
        ))
    }
}
