// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! ⚠ CLEANUP-MAP (docs/dev/ui/windowd-cleanup-map.md): MOVE → Shell-App/Widget (Chrome-Animationen = UI).
//! DO NOT EXTEND — new capability belongs at the target, not here.
//
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
        // Springs (sidebar/hover proof layers) keep the present-loop pacer
        // ticking until motion settles, then windowd goes idle. (Window scroll
        // momentum left with the legacy chat/search windows — app scroll is
        // the DSL app's business, presented as client frames.)
        self.animation_driver.active_count() > 0
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
        for update in updates {
            match update.layer_id {
                SIDEBAR_LAYER_ID => sidebar_dirty = true,
                // HOVER drives the (proof) hover highlight alpha.
                HOVER_LAYER_ID => button_dirty = true,
                _ => panel_dirty = true,
            }
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
                _ => {}
            }
        }
    }
}
