// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The compositor's frame pulse in the app (RFC-0110): the scroll layers' physics, the
//! DSL animations and the interaction layers' fades advance to the time the next frame is shown,
//! the damage they owe is merged, the next pulse is asked for while motion is live — and the
//! motion chain's hop is kept (RFC-0108 §hops, TASK-0354 P1: `nexus_ux_budgets::chains::
//! MotionHops`; its line when an episode falls silent). Split out of the event loop
//! (`probe/mod.rs`, structure ratchet).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the hop on the host (`nexus-ux-budgets`); the scroll and animation ticks by
//!   the QEMU lanes (`windowd: frames(moved=)`, the DSL conformance tests); the line on the board

use super::*;

impl DslApp {
    /// One frame pulse: `at` is the shown time it names (the own clock under a plain pulse).
    pub(super) fn on_frame_pulse(
        &mut self,
        at: Option<u64>,
        client: &KernelClient,
        surface_id: u32,
        dirty: &mut bool,
        dirty_rows: &mut Option<(i32, i32)>,
    ) {
        // A pulse that finds motion live is a wake-up and starts the motion hop; the present
        // that answers it ends the hop (`band.rs`).
        let live = self.momentum_active() || self.anim_active() || self.layers_live();
        self.motion_hops.pulse(nsec_now(), live);
        // The hover and press layers fade by opacity: motion frames, never a raster.
        if self.layers_tick(at.unwrap_or_else(nsec_now)) {
            self.motion_dirty = true;
        }
        // The scroll layers' physics: offsets committed as motion, never a raster; an
        // `EndReached` that loaded more is a model change.
        if self.momentum_tick(at) {
            self.merge_tap_damage(dirty, dirty_rows);
        }
        // Animation tick: damage EXACTLY the animated nodes' union row span (old ∪ new
        // transformed AABB) — the 120Hz damage contract; a full repaint per breathe tick
        // starved the input path. Unions with any scroll span; a pending full request wins.
        if let Some(span) = self.anim_tick(at) {
            crate::layout_diff::merge_span(dirty, dirty_rows, span);
        }
        let still = self.momentum_active() || self.anim_active() || self.layers_live();
        if still || self.bands_pending() {
            let req = wire::encode_surface_frame_req(surface_id);
            let _ = client.send(&req, Wait::Blocking);
        }
        // The episode fell silent: its line — now, or with the answer to this pulse's frame.
        if let Some(line) = self.motion_hops.settled(still) {
            say_motion(line);
        }
    }

    /// A present or a motion frame left: the pending pulse's hop ends (`band.rs`).
    pub(super) fn motion_answered(&mut self) {
        if let Some(line) = self.motion_hops.answered(nsec_now()) {
            say_motion(line);
        }
    }
}

/// The episode's line (RFC-0108 §hops): times and counts, no verdict — the pulse → sent hop,
/// the wake-ups, the raster per frame and its rows, the damage per present, the tokens that ran
/// (TASK-0351 P15: where a transition frame's time goes).
fn say_motion(e: nexus_ux_budgets::chains::Episode) {
    let mut names = alloc::string::String::new();
    for t in animation::MotionToken::ALL {
        if e.tokens & (1 << (t as u32)) != 0 {
            if !names.is_empty() {
                names.push(',');
            }
            names.push_str(t.name());
        }
    }
    raw_marker(&alloc::format!(
        "apphost: hops motion (pulse>sent={}/{}/{} ticks={} paint_us={}/{} rows={} px={} tokens={names})",
        e.mean_us, e.max_us, e.frames, e.ticks, e.paint_mean_us, e.paint_max_us, e.rows, e.px
    ));
}
