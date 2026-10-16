// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The present/compositing path: external-framebuffer attach, scanout damage
//! present (host + os variants), committed-buffer present + command execution,
//! the retained render-target layer composite, and the scanout VMO accessor.

use super::resources::{map_nexus_error, validate_rect};
#[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
use super::{PendingRtLayer, MAX_PENDING_RT_LAYERS};
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

use super::blur_cache;

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
    /// command stream only brings the retained layer set it composites — CompositeLayer ops,
    /// gathered into `pending_rt_layers` (content + shadow + glass, onto the RT). Nothing of the
    /// stream is drawn into the VMO.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn execute_commands(&mut self, cmds: &[Command]) -> Result<(), GfxError> {
        // Cursor-move presents carry a minimal command buffer with NO layer
        // commands. The frame is re-composed whole every present, so if we
        // cleared the layer set on those frames the UI would flicker. Replace
        // the retained set only when this frame actually brings layers;
        // otherwise keep the previous set so the UI stays stable.
        if self.gl_scanout_active
            && cmds.iter().any(|c| matches!(c, Command::CompositeLayer { .. }))
        {
            self.pending_rt_count = 0;
            // Content changed → re-upload once.
            self.rt_layers_dirty = true;
            // A fresh full layer set carries the authoritative scroll offset in
            // the scrollable layers' `src_row_abs`, so drop any fast-path overrides.
            self.scroll_src_rows = [None; crate::backend::MAX_SCROLL_IDS];
            // Transform overrides SURVIVE full presents (unlike scroll):
            // the encode is always the untransformed base, the override
            // is the whole animated state - clearing it here would snap
            // a mid-transition window to identity until the next tick.
            // windowd retires overrides explicitly (identity sends on
            // settle/close).
            for cmd in cmds {
                if let Command::CompositeLayer {
                    src_row_abs,
                    src_x,
                    width,
                    height,
                    dst_x,
                    dst_y,
                    opacity,
                    corner_radius,
                    shadow_blur,
                    shadow_offset_y,
                    shadow_alpha,
                    backdrop_blur,
                    scroll_id,
                    content_w,
                    content_h,
                    scroll_band_top_abs,
                    scroll_band_h,
                    layer_id,
                    content_epoch,
                } = cmd
                {
                    if self.pending_rt_count < MAX_PENDING_RT_LAYERS {
                        self.pending_rt_layers[self.pending_rt_count] = PendingRtLayer {
                            src_row_abs: *src_row_abs,
                            src_x: *src_x,
                            width: *width,
                            height: *height,
                            content_w: *content_w,
                            content_h: *content_h,
                            dst_x: *dst_x,
                            dst_y: *dst_y,
                            opacity: *opacity,
                            corner_radius: *corner_radius,
                            shadow_blur: *shadow_blur,
                            shadow_offset_y: *shadow_offset_y,
                            shadow_alpha: *shadow_alpha,
                            backdrop_blur: *backdrop_blur,
                            scroll_id: *scroll_id,
                            scroll_band_top_abs: *scroll_band_top_abs,
                            scroll_band_h: *scroll_band_h,
                            layer_id: *layer_id,
                            content_epoch: *content_epoch,
                        };
                        self.pending_rt_count += 1;
                    } else {
                        // NEVER silent (fake-proof discipline): a dropped
                        // composite = a missing window / lost chrome on
                        // screen. The 8-layer cap did exactly that with 4
                        // open windows (chat alone is 4 commands: 3 band
                        // slices + title overlay) — "Chat hat das Chrome
                        // verloren".
                        let _ = nexus_abi::debug_println("gpud: FAIL rt-layer overflow");
                    }
                }
            }
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

    /// Composite the retained layer set straight onto the scanout RT, over the
    /// wallpaper the frame drew first and before the flip. Every present composes
    /// the whole frame (cursor moves re-render it), so the set is RETAINED across
    /// presents and only replaced when a new frame brings layers.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn composite_pending_rt_layers(&mut self) {
        let n = self.pending_rt_count;
        // Re-upload the atlas content to the GL texture only when it changed. A
        // retained cursor-move present composites from the already-uploaded
        // texture — no per-frame transfer, which is what made mouse-move slow.
        //
        // Content-epoch gate (present-cost fix): windowd bumps ONE global atlas
        // epoch on every atlas content write and stamps it into each layer; a
        // layer SET whose (max) epoch is unchanged composites entirely from the
        // already-uploaded GL atlas — window drags/transforms re-emit the scene
        // every frame but never write content, so their per-frame
        // TRANSFER_TO_HOST train (the measured ~15-entry / ~21ms present)
        // collapses. The epoch is global + write-driven, so unstamped (0)
        // layers in the SAME set are covered by any stamped layer's value; a
        // set with NO stamped layer (0) keeps the legacy dirty behavior.
        let set_epoch = (0..n).map(|i| self.pending_rt_layers[i].content_epoch).max().unwrap_or(0);
        let upload = if set_epoch != 0 {
            self.rt_layers_dirty && set_epoch != self.atlas_uploaded_epoch
        } else {
            self.rt_layers_dirty
        };
        // Glass-blur cache walk state: the destination-so-far fold + the
        // cache-row packing cursor (`blur_cache`).
        let mut below =
            blur_cache::seed(self.display_w, self.display_h, self.blur_cache.wallpaper_epoch);
        let mut cache_y: u32 = 0;
        for i in 0..n {
            let mut l = self.pending_rt_layers[i];
            // Transform fast path (Track C2): apply the id's recorded
            // translate / opacity-multiply / center-scale override BEFORE the
            // composite — the blur + draw below both read the adjusted rect.
            if let Some(t) = l
                .layer_id
                .checked_sub(1)
                .and_then(|i| self.layer_transforms.get(i as usize).copied().flatten())
            {
                l.dst_x = (l.dst_x as i32 + i32::from(t.dx)).max(0) as u32;
                l.dst_y = (l.dst_y as i32 + i32::from(t.dy)).max(0) as u32;
                l.opacity = l.opacity * u32::from(t.opacity) / 255;
                if t.scale_pct != 100 && t.scale_pct > 0 {
                    let nw = (l.width * u32::from(t.scale_pct) / 100).max(1);
                    let nh = (l.height * u32::from(t.scale_pct) / 100).max(1);
                    // Source stays the un-scaled band: the shader scales
                    // content → frame when they differ (the resize path).
                    if l.content_w == 0 {
                        l.content_w = l.width;
                    }
                    if l.content_h == 0 {
                        l.content_h = l.height;
                    }
                    l.dst_x = (l.dst_x as i32 + (l.width as i32 - nw as i32) / 2).max(0) as u32;
                    l.dst_y = (l.dst_y as i32 + (l.height as i32 - nh as i32) / 2).max(0) as u32;
                    l.width = nw;
                    l.height = nh;
                }
            }
            // Scroll fast path: a scrollable layer (non-zero scroll_id) is
            // re-sampled at its id's override row when set — no CPU re-render,
            // just a different source offset into the already-uploaded atlas
            // texture.
            let src_row_abs = l
                .scroll_id
                .checked_sub(1)
                .and_then(|i| self.scroll_src_rows.get(i as usize).copied().flatten())
                .unwrap_or(l.src_row_abs);
            // Frosted glass: blur what is beneath this layer's rect
            // (destination-so-far — layers composite back-to-front, so lower
            // windows/chrome are already on the RT); the layer's tint +
            // content then composite over it. CACHED: an unchanged backdrop
            // redraws from the cache texture; a changed one re-blurs into its
            // rows (`glass_blur`, `blur_cache`).
            cache_y = self.glass_blur_step(i, below, cache_y, &l);
            // `content_w`/`content_h` (`0` = content fills the layer, the
            // byte-identical steady-state default): the SOURCE sub-size (the
            // client's content band). When set and different from the layer, the
            // content is SCALED from the band up to `width`×`height` (the frame) —
            // the window BODY grows live during a resize; it snaps sharp when the
            // client re-renders at the new size.
            let ok = self
                .composite_layer_rt(
                    src_row_abs,
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
                    upload,
                )
                .is_ok();
            if ok && !self.virgl_layer_marker_done {
                self.virgl_layer_marker_done = true;
                let _ = nexus_abi::debug_println(crate::markers::GPUD_LAYER_COMPOSITE_LIVE);
            }
            // This layer is now part of the destination every LATER glass
            // layer blurs — fold its effective draw inputs.
            below = blur_cache::fold_drawn_layer(below, &l, src_row_abs);
        }
        self.rt_layers_dirty = false;
        if upload && set_epoch != 0 {
            self.atlas_uploaded_epoch = set_epoch;
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
