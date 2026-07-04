// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — animation tick (springs → scene graph).
//! OWNERS: @ui
//! STATUS: Experimental
//!
//! Split out of `runtime/mod.rs` (TASK-0063 modularization): the
//! `DisplayServerRuntime` animation driver step. `tick` advances the springs,
//! routes each `SceneUpdate` to its scene-graph node (`apply_scene_updates` via
//! the shell's layer→node map), and marks the changed regions dirty. Child module
//! of `runtime`, so it reads the runtime's private state directly.

use super::*;

impl DisplayServerRuntime {
    pub(crate) fn has_active_animations(&self) -> bool {
        // Springs (sidebar/hover) OR either window's scroll momentum — any keeps the
        // present-loop pacer ticking until motion settles, then windowd goes idle.
        self.animation_driver.active_count() > 0
            || self.chat_list.is_animating()
            || (self.search.visible && self.search_scroll.is_animating())
    }

    /// Record one empty NonBlocking poll wake-up (busy-poll spin) for telemetry.
    /// See `WindowdDisplayTelemetryReport::spin_hz`.
    pub(crate) fn record_poll_spin(&mut self) {
        self.telemetry.record_poll_spin();
    }

    pub(crate) fn tick(&mut self, now_ns: u64) {
        // Reactive: only drive animations when they are active.
        // No polling — the caller gates this via has_active_animations().
        // When no animation is running, tick() is not called at all.
        //
        // Chat scroll momentum first: it integrates the virtual-list velocity
        // over real elapsed time and queues the cheap GPU offset re-present. Runs
        // even when no spring is active (the spring block early-returns below), so
        // a pure scroll flick still advances every frame.
        self.tick_chat_scroll(now_ns);
        // Search window scroll momentum: the SAME engine, eased the same way (E2).
        self.tick_search_scroll(now_ns);
        // Freeze forensics (rate-limited ~500ms while anything scrolls): one line
        // with both engines' position/target + the present-loop health counters.
        // If scroll ever dies again, this pins WHICH stage stopped: the engine
        // (pos stuck), the pacer (no lines at all), or the present path
        // (inflight pinned / pending never draining).
        if (self.chat_list.is_animating()
            || (self.search.visible && self.search_scroll.is_animating()))
            && now_ns.saturating_sub(self.chat_scroll_diag_ns) >= 500_000_000
        {
            self.chat_scroll_diag_ns = now_ns;
            let _ = debug_println(&alloc::format!(
                "windowd: scroll diag chat={}/{} search={}/{} inflight={} pending={}",
                self.chat_list.scroll_offset().as_i32(),
                self.chat_list.scroll_target(),
                self.search_scroll.offset_px(),
                self.search_scroll.target() as i32,
                self.frames_in_flight(),
                self.has_pending_damage(),
            ));
        }

        let mut anim_updates = [SceneUpdate::default(); ANIMATION_UPDATE_CAP];
        let update_count = self.animation_driver.tick_into(now_ns, &mut anim_updates);
        if update_count == 0 {
            return;
        }
        let updates = &anim_updates[..update_count];
        self.apply_scene_updates(updates);

        // Per-layer damage: only mark regions that actually changed.
        // Sidebar animation → only sidebar rect; hover/click/key → only panel.
        // GPU-only blit rects: Plane 1 content is unchanged, the GPU CB reads
        // the animated state each frame — no CPU recomposite, no re-blur.
        let mut panel_dirty = false;
        let mut sidebar_dirty = false;
        let mut button_dirty = false;
        let mut dropdown_dirty = false;
        for update in updates {
            match update.layer_id {
                SIDEBAR_LAYER_ID => sidebar_dirty = true,
                DROPDOWN_LAYER_ID => dropdown_dirty = true,
                // HOVER drives the glass button's alpha (hover highlight), which
                // sits at the top-right — not in the left panel rect.
                HOVER_LAYER_ID => button_dirty = true,
                _ => panel_dirty = true,
            }
        }
        if dropdown_dirty {
            use crate::compositor::desktop_layer::{
                menu_item_x, DROPDOWN_W, TOPBAR_H, TOPBAR_MARGIN_X, TOPBAR_TOP,
            };
            let dx = TOPBAR_MARGIN_X + menu_item_x(self.dropdown_item());
            let dy = TOPBAR_TOP + TOPBAR_H + 4;
            self.queue_gpu_blit_rect(DamageRect {
                x: dx,
                y: dy,
                width: DROPDOWN_W.min(self.mode.width.saturating_sub(dx)),
                height: self.dropdown_h,
            });
        }
        if panel_dirty {
            let panel_damage = DamageRect {
                x: 0,
                y: 0,
                width: COMBINED_PANEL_WIDTH as u32,
                height: PROOF_PANEL_H,
            };
            self.queue_gpu_blit_rect(panel_damage);
        }
        if button_dirty {
            let b = crate::interaction::button_rect(self.mode.width);
            self.queue_gpu_blit_rect(DamageRect {
                x: b.x,
                y: b.y,
                width: b.width,
                height: b.height,
            });
        }
        if sidebar_dirty {
            self.queue_gpu_blit_rect(self.sidebar_damage_rect());
        }

        // Markers: emit once per animation lifecycle, not per tick.
        if !self.animation_proof.batch_marker {
            let _ = debug_println(UIRUNTIME_BATCH_COMMIT_OK);
            self.animation_proof.batch_marker = true;
        }
        if !self.animation_proof.live_marker {
            let _ = debug_println(WINDOWD_LIVE_TRANSITION_OK);
            self.animation_proof.live_marker = true;
        }
        if self.animation_driver.active_count() == 0 && !self.animation_proof.spring_marker {
            let _ = debug_println(UIANIM_SPRING_CONVERGE_OK);
            self.animation_proof.spring_marker = true;
        }
        // Invalidate sidebar blur cache only after the close animation finishes.
        // Keeping it valid during slide-out means every closing frame uses the cache
        // instead of triggering a full re-blur.
        if !self.state.sidebar_open_visible && self.animated_scene.sidebar_opacity < 0.01 {
            self.sidebar_blur_cache_valid = false;
        }
        if self.animation_proof.batch_marker
            && self.animation_proof.live_marker
            && self.animation_proof.spring_marker
            && self.input_markers_emitted.v2b_assets_summary
            && !self.animation_proof.v5_summary_marker
        {
            let _ = debug_println(SELFTEST_UI_V5_TRANSITION_OK);
            self.animation_proof.v5_summary_marker = true;
        }

        if let Some(report) = self.telemetry.report_values_if_due(now_ns) {
            emit_windowd_telemetry(report);
        }
    }

    fn apply_scene_updates(&mut self, updates: &[SceneUpdate]) {
        for update in updates {
            // Route to the scene graph via the shell's explicit layer→node
            // mapping (unknown layers are dropped, never mistargeted).
            if let Some(target) = self.shell.animation_target(update.layer_id) {
                self.shell.graph.apply_animation_update_to(target, *update);
            }
            // Keep legacy state for backward-compatible markers/telemetry.
            match (update.layer_id, update.property) {
                (HOVER_LAYER_ID, AnimProp::Opacity) => {
                    self.animated_scene.hover_opacity = update.value.clamp(0.0, 1.0);
                }
                (SIDEBAR_LAYER_ID, AnimProp::TranslateX) => {
                    self.animated_scene.sidebar_translate_x =
                        update.value.clamp(0.0, SIDEBAR_WIDTH as f32);
                    // Also update the sidebar's scene graph position.
                    self.shell.set_sidebar_slide(self.animated_scene.sidebar_translate_x);
                }
                (SIDEBAR_LAYER_ID, AnimProp::Opacity) => {
                    self.animated_scene.sidebar_opacity = update.value.clamp(0.0, 1.0);
                }
                (DROPDOWN_LAYER_ID, AnimProp::Opacity) => {
                    self.animated_scene.apps_dropdown_progress = update.value.clamp(0.0, 1.0);
                }
                _ => {}
            }
        }
    }
}
