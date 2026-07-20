// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — damage queueing, frame writes (`write_rows`/`write_damage_rect`), the batched gpud present, and the stall watchdog.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (behavior covered via windowd QEMU smoke + host integration)
//!
//! Split out of `runtime/mod.rs` (TASK-0063 modularization). A child module of
//! `runtime`, so these `impl DisplayServerRuntime` methods read the runtime's
//! private fields directly; previously-private methods are widened to
//! `pub(super)` so the parent and sibling submodules can still call them.

use super::*;

impl DisplayServerRuntime {
    /// C1 (RFC-0067 closure): the proof/target-test panel was deleted, so there
    /// are no target-test rects to damage. Retained as a no-op for the input
    /// state-machine call sites until they're pruned.
    pub(super) fn queue_target_damage(
        &mut self,
        _old_state: VisibleState,
        _new_state: VisibleState,
    ) {
    }

    /// Upload the cursor sprite to gpud. gpud arms the virtio-gpu hardware
    /// cursor overlay (64×64 resource on the cursor queue) and keeps the sprite
    /// as the software BlendCursor fallback. Blocking: the 5-byte reply reports
    /// which path is live (`flags == 1` → hardware overlay).
    /// (OP_SUBMIT_ANIMATION_FRAME sender — animation now rides widget loops;
    /// kept: documents the fragment-uniform wire contract gpud still serves.)
    #[allow(dead_code)]
    pub(super) fn submit_animation_to_gpud(
        &mut self,
        updates: &[SceneUpdate],
    ) -> Result<(), WindowdError> {
        let mut cmd = CommandBuffer::new();
        {
            let mut encoder = cmd
                .try_begin_render_pass(RenderPassDesc {
                    color_attachments: alloc::vec![],
                    width: self.mode.width,
                    height: self.mode.height,
                })
                .map_err(|_| WindowdError::InvalidDamage)?;
            let mut payload = [0u8; 16];
            payload[..4].copy_from_slice(&(updates.len() as u32).to_le_bytes());
            payload[4..8].copy_from_slice(&self.animated_scene.hover_opacity.to_le_bytes());
            payload[8..12].copy_from_slice(&self.animated_scene.sidebar_translate_x.to_le_bytes());
            payload[12..16].copy_from_slice(&self.animated_scene.sidebar_opacity.to_le_bytes());
            encoder.try_set_fragment_bytes(0, &payload).map_err(|_| WindowdError::InvalidDamage)?;
            encoder
                .try_draw_tiles(
                    &[
                        TileRect {
                            x: self.mode.width.saturating_sub(SIDEBAR_WIDTH),
                            y: 0,
                            width: SIDEBAR_WIDTH,
                            height: self.mode.height,
                        },
                        TileRect {
                            x: self.mode.width.saturating_sub(180),
                            y: 24,
                            width: 156,
                            height: 56,
                        },
                    ],
                    RgbaColor::new(200, 220, 255, 192),
                )
                .map_err(|_| WindowdError::InvalidDamage)?;
            encoder.end_encoding();
        }
        let committed = cmd.try_commit().map_err(|_| WindowdError::InvalidDamage)?;
        if committed.command_count() == 0 {
            return Err(WindowdError::InvalidDamage);
        }
        // Serialize the CommittedBuffer into an IPC frame.
        // Frame layout: [opcode=GPU_ANIMATION_SUBMIT_OP] + serialized CommittedBuffer.
        let mut frame_buf = [0u8; 512];
        let written = committed
            .serialize_into(&mut frame_buf[1..])
            .map_err(|_| WindowdError::InvalidDamage)?;
        frame_buf[0] = GPU_ANIMATION_SUBMIT_OP;
        let total = 1usize.saturating_add(written);
        self.send_gpud_status_request(&frame_buf[..total])
    }

    pub(super) fn sidebar_damage_rect(&self) -> DamageRect {
        DamageRect {
            x: self.mode.width.saturating_sub(SIDEBAR_WIDTH),
            y: 0,
            width: SIDEBAR_WIDTH,
            height: self.mode.height,
        }
    }

    pub(super) fn write_current_frame(&mut self) -> Result<(), WindowdError> {
        self.reset_effect_caches();
        // Mark every tile dirty so the first full-screen write renders all rows.
        self.tile_map.mark_rect(DamageRect {
            x: 0,
            y: 0,
            width: self.mode.width,
            height: self.mode.height,
        });
        self.write_rows(0, self.mode.height, select_glass_quality(PROOF_PANEL_H), false)
    }

    pub(super) fn write_rows(
        &mut self,
        start_y: u32,
        end_y: u32,
        _glass_quality: GlassQuality,
        _paint_only: bool,
    ) -> Result<(), WindowdError> {
        let render_start_ns = nsec().unwrap_or(0);
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let row_len = self.mode.stride as usize;
        if self.band_scratch.len() < row_len * ROW_WRITE_CHUNK {
            return Err(WindowdError::BufferLengthMismatch);
        }
        // G3 (RFC-0067 P5-Final): the base pass is wallpaper-only — the proof panel
        // and all glass are GPU layers, so the per-row shadow/content state
        // (proof layout, glass/path/backdrop caches, shadow arena) is gone.
        let source_frame = &self.source_frame;
        let source_x_lut = self.source_x_lut.as_slice();
        let source_y_lut = self.source_y_lut.as_slice();
        let mode = self.mode;
        let end_y = end_y.min(self.mode.height);
        let render_clip = RenderClip::full(self.mode.width);
        let band_scratch = &mut self.band_scratch;
        let mut band_start = start_y.min(end_y);
        while band_start < end_y {
            let band_end = (band_start as usize + ROW_WRITE_CHUNK).min(end_y as usize) as u32;
            // Skip bands that contain only clean tiles.
            if !self.tile_map.has_dirty_in_row_range(band_start, band_end) {
                band_start = band_end;
                continue;
            }
            // band rendering
            let band_rows = (band_end - band_start) as usize;
            let band_bytes = band_rows * row_len;
            for (row_idx, y) in (band_start..band_end).enumerate() {
                let dest_start = row_idx * row_len;
                let dest_end = dest_start + row_len;
                let band_row = &mut band_scratch[dest_start..dest_end];
                copy_scene_row(
                    source_frame,
                    source_x_lut,
                    source_y_lut,
                    mode,
                    y,
                    render_clip,
                    band_row,
                )?;
                // Chat is a retained-surface layer composited by build_scene_cb —
                // no longer baked into Plane 1 here. GPU overlays (button, sidebar,
                // cursor) are likewise added in the CommandBuffer.
            }
            let offset = band_start as usize * row_len;
            // CPU computes background content (wallpaper + proof panel) into Plane 1.
            // GPU draws the animated overlay (button, sidebar, cursor) on top each frame.
            vmo_write(handle, RETAINED_OFFSET_BYTES + offset, &band_scratch[..band_bytes])
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
            band_start = band_end;
        }
        self.state.cursor_overlay_visible = self.state.cursor_svg_visible;
        self.telemetry.record_compose_timed(
            u64::from(self.mode.width).saturating_mul(u64::from(end_y.saturating_sub(start_y))),
            nsec().unwrap_or(render_start_ns).saturating_sub(render_start_ns),
        );
        self.telemetry.record_present();
        self.refresh_observer_state();
        Ok(())
    }

    pub(super) fn write_damage_rect(
        &mut self,
        rect: DamageRect,
        _glass_quality: GlassQuality,
        _paint_only: bool,
    ) -> Result<(), WindowdError> {
        let render_start_ns = nsec().unwrap_or(0);
        let Some(handle) = self.framebuffer else {
            return Ok(());
        };
        let row_len = self.mode.stride as usize;
        if self.band_scratch.len() < row_len * ROW_WRITE_CHUNK {
            return Err(WindowdError::BufferLengthMismatch);
        }
        let start_y = rect.y.min(self.mode.height);
        let end_y = rect.end_y().min(self.mode.height);
        let start_x = rect.x.min(self.mode.width);
        let end_x = rect.end_x().min(self.mode.width);
        if start_y >= end_y || start_x >= end_x {
            return Ok(());
        }
        // G3: wallpaper-only base pass (proof panel + glass are GPU layers).
        let source_frame = &self.source_frame;
        let source_x_lut = self.source_x_lut.as_slice();
        let source_y_lut = self.source_y_lut.as_slice();
        let mode = self.mode;
        let byte_start = start_x as usize * 4;
        let byte_end = end_x as usize * 4;
        let render_clip = RenderClip::new(start_x, end_x, self.mode.width);
        let mut band_start = start_y;
        while band_start < end_y {
            let band_end = (band_start as usize + ROW_WRITE_CHUNK).min(end_y as usize) as u32;
            for (row_idx, y) in (band_start..band_end).enumerate() {
                let dest_start = row_idx * row_len;
                let band_row = &mut self.band_scratch[dest_start..dest_start + row_len];
                copy_scene_row(
                    source_frame,
                    source_x_lut,
                    source_y_lut,
                    mode,
                    y,
                    render_clip,
                    band_row,
                )?;
                // Chat is composited as a layer in build_scene_cb, not baked here.
            }
            for (row_idx, y) in (band_start..band_end).enumerate() {
                let offset = y as usize * row_len + byte_start;
                let src_offset = row_idx * row_len + byte_start;
                if byte_start == 0 && byte_end == row_len {
                    let band_bytes = (band_end - band_start) as usize * row_len;
                    vmo_write(
                        handle,
                        RETAINED_OFFSET_BYTES + band_start as usize * row_len,
                        &self.band_scratch[..band_bytes],
                    )
                    .map_err(|_| WindowdError::BufferLengthMismatch)?;
                    break;
                }
                vmo_write(
                    handle,
                    RETAINED_OFFSET_BYTES + offset,
                    &self.band_scratch[src_offset..src_offset + (byte_end - byte_start)],
                )
                .map_err(|_| WindowdError::BufferLengthMismatch)?;
            }
            band_start = band_end;
        }
        self.state.cursor_overlay_visible = self.state.cursor_svg_visible;
        self.telemetry.record_compose_timed(
            u64::from(end_x.saturating_sub(start_x))
                .saturating_mul(u64::from(end_y.saturating_sub(start_y))),
            nsec().unwrap_or(render_start_ns).saturating_sub(render_start_ns),
        );
        self.telemetry.record_present();
        self.refresh_observer_state();
        Ok(())
    }

    pub(super) fn queue_dirty_rect(&mut self, rect: DamageRect) {
        self.tile_map.mark_rect(rect);
        for existing in &mut self.pending_damage_rects {
            if rect.x <= existing.end_x()
                && rect.end_x() >= existing.x
                && rect.y <= existing.end_y()
                && rect.end_y() >= existing.y
            {
                *existing = existing.merge(rect);
                return;
            }
        }
        if self.pending_damage_rects.len() < 4 {
            self.pending_damage_rects.push(rect);
        }
    }

    /// Queue a GPU-only blit rect for animation frames where only GPU CB params
    /// (translate_x, opacity) changed. Plane 1 is already current — no CPU
    /// recomposite. The rect still needs a display-plane refresh from Plane 1.
    pub(super) fn queue_gpu_blit_rect(&mut self, rect: DamageRect) {
        self.pending_gpu_blit_rect = Some(match self.pending_gpu_blit_rect {
            Some(existing) => existing.merge(rect),
            None => rect,
        });
    }

    /// Flush pending damage to gpud as one batched CommandBuffer.
    ///
    /// Phase 0 (GPU pipeline hardening): the scene graph is the single rendering
    /// authority. `compute_dirty_set()` on the scene graph drives all CB generation.
    /// No CPU compositing — wallpaper is a `BlitSurface` from Plane 0,
    /// panels are `FillSdfRoundedRect`/`BlurBackdrop`, and the cursor is `BlendCursor`.
    pub(crate) fn flush_pending_damage(&mut self) -> Result<(), WindowdError> {
        let paint_only = self.paint_only_damage;

        // 1. Collect content damage (panels/text — needs CPU recomposite of Plane 1).
        let mut content = [DamageRect { x: 0, y: 0, width: 0, height: 0 }; 5];
        let mut content_count = 0usize;
        if let Some(rect) = self.pending_damage_rect.take() {
            content[content_count] = rect;
            content_count += 1;
        }
        while let Some(rect) = self.pending_damage_rects.pop() {
            if content_count < content.len() {
                content[content_count] = rect;
                content_count += 1;
            }
        }
        content_count = premerge_damage_rects(&mut content, content_count);

        // GPU-blit-only rect from animation ticks (Plane 1 already current).
        let gpu_blit_rect = self.pending_gpu_blit_rect.take();
        // Cursor-only move: skip CPU recomposite — just a cheap blit of the
        // cursor region from the retained Plane 1 + BlendCursor (the hot path).
        let cursor_rect = self.pending_cursor_rect.take();

        if content_count == 0 && gpu_blit_rect.is_none() && cursor_rect.is_none() {
            return Ok(());
        }

        // 2. Recomposite ONLY content damage into Plane 1 (CPU, blur cached).
        // (The login greeter is a composited LAYER above this base — Plane 1
        // keeps the normal desktop while it is up, no special casing.)
        let glass_quality = select_glass_quality(PROOF_PANEL_H);
        for rect in content.iter().copied().take(content_count) {
            if let Err(e) = self.write_damage_rect(rect, glass_quality, paint_only) {
                // Fold-immune: a failed Plane-1 recomposite silently freezes
                // the base scene (the wallpaper-swap hunt earned this line).
                let _ = nexus_abi::debug_write(b"windowd: recomposite ERR\n");
                return Err(e);
            }
        }

        // 3. Blit list: content + gpu-blit + cursor rects — all refresh the
        //    display plane from the retained Plane 1.
        let mut blits = [DamageRect { x: 0, y: 0, width: 0, height: 0 }; 7];
        let mut blit_count = 0usize;
        for rect in content.iter().copied().take(content_count) {
            blits[blit_count] = rect;
            blit_count += 1;
        }
        if let Some(rect) = gpu_blit_rect {
            blits[blit_count] = rect;
            blit_count += 1;
        }
        if let Some(rect) = cursor_rect {
            blits[blit_count] = rect;
            blit_count += 1;
        }

        // 4. One scene CB: blit retained→display + GPU glass overlays + cursor.
        let mut frame_buf = [0u8; 8192];
        let written = self.build_scene_cb_into(&blits, blit_count, &mut frame_buf[1..])?;
        self.tile_map.clear();
        frame_buf[0] = GPU_PRESENT_DAMAGE_OP;
        let gpud_ok = self.send_gpud_present(&frame_buf[..1 + written]);
        if !gpud_ok {
            // gpud queue full / backpressured — requeue so the next tick retries.
            for rect in content.iter().copied().take(content_count) {
                self.queue_dirty_rect(rect);
            }
            if let Some(rect) = gpu_blit_rect {
                self.pending_gpu_blit_rect = Some(match self.pending_gpu_blit_rect {
                    Some(existing) => existing.merge(rect),
                    None => rect,
                });
            }
            if let Some(rect) = cursor_rect {
                self.pending_cursor_rect = Some(match self.pending_cursor_rect {
                    Some(existing) => existing.merge(rect),
                    None => rect,
                });
            }
            self.paint_only_damage = false;
            return Ok(());
        }
        if !self.v3b_composition_verified {
            let _ = debug_println("windowd: scene graph on");
            let _ = debug_println("windowd: gpu pipeline on");
            // Window manager is live: the chat ShellWindow (and any sibling
            // windows) are registered and driving the composite with drag +
            // z-order. TASK-0064 (UI v6a) WM marker ladder.
            let _ = debug_println("windowd: wm on");
            let _ = debug_println("SELFTEST: ui v6 wm ok");
        }
        self.emit_input_markers();
        self.v3b_composition_verified = true;
        self.emit_v3b_markers();
        self.paint_only_damage = false;
        Ok(())
    }

    /// VSYNC-aligned flush: submit pending damage at most once per pacer
    /// interval. A lone event after idle flushes immediately (the stamp is
    /// stale); a sustained input burst (pointer pushes at up to 250Hz) becomes
    /// a steady ~120Hz present train — the staged-input newest-wins coalescing
    /// keeps each presented frame fresh. `force` marks a real pacer tick (the
    /// vsync itself) and always flushes.
    pub(crate) fn flush_pending_damage_paced(
        &mut self,
        now_ns: u64,
        force: bool,
    ) -> Result<(), WindowdError> {
        if !self.has_pending_damage() {
            return Ok(());
        }
        if !force && now_ns.saturating_sub(self.last_paced_flush_ns) < super::PACER_INTERVAL_NS {
            // Too soon after the last present: leave the damage pending. The
            // pacer stays armed while damage is pending (needs_pacing), so the
            // next tick — at most one interval away — submits the merged rect.
            return Ok(());
        }
        self.last_paced_flush_ns = now_ns;
        self.flush_pending_damage()
    }

    pub(crate) fn has_pending_damage(&self) -> bool {
        self.pending_gpu_blit_rect.is_some()
            || !self.pending_damage_rects.is_empty()
            || self.pending_damage_rect.is_some()
            || self.pending_cursor_rect.is_some()
    }

    /// P0.3 self-heal: gpud NACKed a present (`gpud: FAIL present deadline` — its
    /// commands ran into the ring's 500ms net and were abandoned). The frame is
    /// done in the flow-control sense (slot freed, seq advanced so the watchdog
    /// tracks genuine no-reply stalls, not this handled case) but its pixels never
    /// reached the screen. Requeue FULL-frame damage — after an abandoned GPU
    /// batch the RT state is unknown, so a partial repaint could leave stale
    /// regions — under a bounded budget: a permanently failing device degrades
    /// to ONE loud FAIL marker instead of an infinite repaint loop.
    pub(super) fn note_present_nacked(&mut self) {
        const MAX_PRESENT_RETRIES: u32 = 8;
        self.last_completed_seq = self.present_seq;
        self.frames_in_flight = self.frames_in_flight.saturating_sub(1);
        self.present_retry_count = self.present_retry_count.saturating_add(1);
        #[cfg(nexus_env = "os")]
        NACK_TOTAL.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        if self.present_retry_count <= MAX_PRESENT_RETRIES {
            let _ = debug_println(&alloc::format!(
                "windowd: present retry n={}",
                self.present_retry_count
            ));
            self.queue_dirty_rect(DamageRect {
                x: 0,
                y: 0,
                width: self.mode.width,
                height: self.mode.height,
            });
            // Full recompose, not a paint-only patch: the failed frame's GPU
            // state is undefined.
            self.paint_only_damage = false;
            #[cfg(nexus_env = "os")]
            NACK_FULL_RECOMPOSE_TOTAL.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        } else if !self.present_retry_exhausted {
            self.present_retry_exhausted = true;
            let _ = debug_println(&alloc::format!(
                "windowd: FAIL present retries exhausted (n={})",
                self.present_retry_count
            ));
        }
    }

    /// P0.3: a clean present ack ends any NACK episode — reset the retry budget
    /// so the next degradation gets a fresh bounded recovery (and its markers).
    pub(super) fn note_present_acked_clean(&mut self) {
        self.present_retry_count = 0;
        self.present_retry_exhausted = false;
    }

    /// Stall watchdog — call once per present-loop iteration with `now_ns`.
    ///
    /// Detects the "scrolled and it stopped responding" failure: the loop is still
    /// running but presents make no progress (gpud backpressure / a wedged ring /
    /// heap exhaustion) while damage keeps piling up. When the acknowledged present
    /// seq hasn't advanced for `STALL_THRESHOLD_NS` with damage pending, it logs ONE
    /// diagnostic line per stall episode (rate-limited → the `format!` is not on the
    /// hot path) capturing the state needed to triage it, then re-arms on recovery.
    /// This is the compositor analogue of Android's ANR / Linux's hung-task detector.
    pub(crate) fn watchdog_check(&mut self, now_ns: u64) {
        const STALL_THRESHOLD_NS: u64 = 500_000_000; // 0.5 s — a blatant stall @120Hz
                                                     // Progress = the completed seq advanced, or there's simply nothing pending.
        let progressed = self.last_completed_seq != self.stall_last_seq;
        if progressed || !self.has_pending_damage() {
            self.stall_last_seq = self.last_completed_seq;
            self.stall_last_progress_ns = now_ns;
            self.stall_reported = false;
            return;
        }
        if self.stall_last_progress_ns == 0 {
            self.stall_last_progress_ns = now_ns;
            return;
        }
        let stuck = now_ns.saturating_sub(self.stall_last_progress_ns);
        if stuck >= STALL_THRESHOLD_NS {
            if !self.stall_reported {
                let _ = debug_println(&alloc::format!(
                    "windowd: STALL present stuck {}ms — pending_rects={} in_flight={} last_seq={} (recovering)",
                    stuck / 1_000_000,
                    self.pending_damage_rects.len(),
                    self.frames_in_flight(),
                    self.last_completed_seq,
                ));
                self.stall_reported = true;
            }
            // RECOVERY: a present that never gets acked (QEMU dropped/deferred the
            // completion) would otherwise pin `frames_in_flight` at max forever →
            // windowd could never present again = permanent freeze. Drop the wedged
            // in-flight frames so the next iteration resubmits — a brief hiccup
            // instead of a hang. A late ack is harmless: `note_present_completed`
            // uses `saturating_sub` + an idempotent seq assignment.
            self.frames_in_flight = 0;
            self.last_completed_seq = self.present_seq;
            self.stall_last_seq = self.present_seq;
            self.stall_last_progress_ns = now_ns; // measure the next stall fresh
        }
    }

    /// Phase 7: maximum in-flight frames before backpressure.
    pub(crate) const fn max_in_flight() -> u32 {
        2
    }

    /// Phase 7: current frames in flight to gpud (exposed for pacing).
    pub(crate) fn frames_in_flight(&self) -> u32 {
        self.frames_in_flight
    }

    /// Monotone count of presents actually sent to gpud (loop-cadence telemetry).
    pub(crate) fn present_seq_value(&self) -> u32 {
        self.present_seq
    }

    /// Cumulative present NACKs (loop-cadence telemetry reads window deltas).
    #[cfg(nexus_env = "os")]
    pub(crate) fn nack_total(&self) -> u32 {
        NACK_TOTAL.load(core::sync::atomic::Ordering::Relaxed)
    }

    /// Cumulative NACK-driven full-frame recompose requeues.
    #[cfg(nexus_env = "os")]
    pub(crate) fn nack_full_recompose_total(&self) -> u32 {
        NACK_FULL_RECOMPOSE_TOTAL.load(core::sync::atomic::Ordering::Relaxed)
    }
}

/// SMP-flicker triage counters (statics, not runtime fields, so the
/// grandfathered `runtime/mod.rs` struct stays untouched): cumulative present
/// NACKs and the full-frame recompose requeues they caused. windowd is
/// single-threaded — Relaxed is only formal.
#[cfg(nexus_env = "os")]
static NACK_TOTAL: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
#[cfg(nexus_env = "os")]
static NACK_FULL_RECOMPOSE_TOTAL: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(0);
