// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd compositor runtime — the frame pulse (the Choreographer contract, RFC-0110):
//! an ANIMATING client (a scroll's ease or fling, a DSL animation) sends `OP_SURFACE_FRAME_REQ`, a
//! one-shot request, and windowd answers with ONE `OP_SURFACE_FRAME` after its next composited
//! frame — naming, since TASK-0343, the time the client's next frame is shown (the display's
//! timeline from gpud's timed acks, `crate::frame_clock`) and the time between frames on the
//! glass. The client advances its physics to that time, so its motion moves by the time between
//! the frames the glass shows. windowd's own transitions step to the same time. Split out of
//! `app_window.rs` (the module-size ratchet).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the timeline in `crate::frame_clock` (host); the wire in
//!   `nexus_display_proto::frame_time`; the pulse by the QEMU lanes (`SELFTEST: ui visible wheel
//!   ok`) and the board's `windowd: frame pace (` line

use super::*;
use crate::client_surface::Delivery;
use crate::frame_clock::FrameClock;
use nexus_display_proto::frame_time::{decode_present_ack_shown, encode_surface_frame_at, FrameAt};

/// The pulses armed outside the windows' own flags, the display's timeline they name, and the
/// latest time windowd's own transitions were stepped to.
pub(crate) struct Pulses {
    /// One-shot frame pulse armed by the desktop surface (the shell's scroll).
    desktop: bool,
    clock: FrameClock,
    stepped_ns: u64,
}

impl Pulses {
    pub(crate) const fn new() -> Self {
        Self { desktop: false, clock: FrameClock::new(), stepped_ns: 0 }
    }

    /// The judged landings so far — on time, late, early — and the cadence and period gpud
    /// named last (`windowd: frame pace (`).
    pub(crate) fn pace(&self) -> ([u32; 3], u8, u32) {
        let c = &self.clock;
        let (cadence, period) = c.shown().map_or((0, 0), |s| (s.cadence, s.period_ns));
        ([c.on_time, c.late, c.early], cadence, period)
    }
}

impl DisplayServerRuntime {
    /// `OP_SURFACE_FRAME_REQ`: arm the one-shot frame pulse for the owning
    /// window (desktop surface included via its own flag-less immediate path:
    /// the desktop is composited every frame anyway, so its pulse sends on
    /// the next flush like any window's).
    pub(crate) fn handle_surface_frame_req(&mut self, frame: &[u8]) {
        let Some(surface_id) = nexus_display_proto::client_surface::decode_surface_frame_req(frame)
        else {
            return;
        };
        if let Some(idx) = self.app_index_by_surface(surface_id) {
            self.apps[idx].frame_pulse_pending = true;
        } else if self.desktop_surface_id == Some(surface_id) {
            self.pulses.desktop = true;
        }
    }

    /// Whether any VISIBLE client is waiting on a frame pulse — while true the
    /// compositor keeps its frame clock running (`keep_frame_clock_alive`, TASK-0324
    /// P7-c): the pulses ARE the vsync the animating client ticks on (without this, a scroll ease degraded to
    /// the client's coarse recv-timeout fallback — "kann dem Scroll nicht
    /// mit den Augen folgen"). A HIDDEN window's pending request is parked
    /// (see [`Self::flush_frame_pulses`]) and must NOT arm the pacer — a
    /// closed animated window would otherwise keep windowd presenting
    /// forever with nothing to draw.
    pub(crate) fn has_frame_pulse_clients(&self) -> bool {
        self.pulses.desktop
            || self.apps.iter().enumerate().any(|(idx, a)| {
                a.frame_pulse_pending
                    && a.win.visible
                    && self.windows.is_visible(crate::window_scene::WindowId::App(idx as u8))
            })
    }

    /// When the present windowd sends next is shown and the time between frames on the glass
    /// (RFC-0110); `None` before gpud named its timeline.
    pub(crate) fn next_frame_shown(&self, now_ns: u64) -> Option<(u64, u32)> {
        self.pulses.clock.next_shown(now_ns, self.frames_in_flight())
    }

    /// The time windowd's own transitions are stepped to: when the frame they shape is shown —
    /// never before a time already stepped to (`now_ns` before gpud named its timeline).
    pub(crate) fn transition_time(&mut self, now_ns: u64) -> u64 {
        let t = self.next_frame_shown(now_ns).map_or(now_ns, |(at, _)| at);
        self.pulses.stepped_ns = self.pulses.stepped_ns.max(t);
        self.pulses.stepped_ns
    }

    /// The judged landings so far, the cadence and the period (`windowd: frame pace (`).
    pub(crate) fn frame_pace(&self) -> ([u32; 3], u8, u32) {
        self.pulses.pace()
    }

    /// gpud credited the present `seq` with `reply`: a timed ack moves the timeline on.
    pub(super) fn note_present_shown(&mut self, seq: u32, reply: &[u8]) {
        if let Some(shown) = decode_present_ack_shown(reply) {
            self.pulses.clock.note_shown(seq, shown);
        }
    }

    /// windowd sends the present `seq` now: the landing predicted for it, kept to judge.
    pub(super) fn note_present_sent(&mut self, seq: u32, now_ns: u64) {
        if let Some((at, _)) = self.next_frame_shown(now_ns) {
            self.pulses.clock.note_sent(seq, at);
        }
    }

    /// The pulse for `surface_id`: timed when gpud named the timeline, plain before.
    fn pulse_frame(&self, surface_id: u32, now_ns: u64, out: &mut [u8]) -> usize {
        match self.next_frame_shown(now_ns) {
            Some((target_ns, step_ns)) => {
                let f = encode_surface_frame_at(FrameAt { surface_id, target_ns, step_ns });
                out[..f.len()].copy_from_slice(&f);
                f.len()
            }
            None => {
                let f = nexus_display_proto::client_surface::encode_surface_frame(surface_id);
                out[..f.len()].copy_from_slice(&f);
                f.len()
            }
        }
    }

    /// Send the armed frame pulses (once per composited frame, after the
    /// present work): ONE `OP_SURFACE_FRAME` per requesting client, then the
    /// request clears — the Choreographer one-shot contract. State (ADR-0075):
    /// a busy client gets the newest pulse once it drains, never a backlog.
    pub(crate) fn flush_frame_pulses(&mut self) {
        #[cfg(nexus_env = "os")]
        {
            let now = nexus_abi::nsec().unwrap_or(0);
            let mut frame = [0u8; nexus_display_proto::frame_time::SURFACE_FRAME_AT_LEN];
            for idx in 0..self.apps.len() {
                if !self.apps[idx].frame_pulse_pending {
                    continue;
                }
                // Visibility gate (the Choreographer contract: the COMPOSITOR
                // owns animation pacing): a closed/minimized/hidden window gets
                // NO pulses — the request stays PENDING at zero cost, and the
                // first flush after re-expose resumes the chain. Without this a
                // closed window running a continuous animation (widget breathe)
                // kept rendering+presenting invisibly forever — every
                // open/close stacked a permanent ~20Hz zombie load (the "mouse
                // gets slower the longer I use the system" report).
                if !self.apps[idx].win.visible
                    || !self.windows.is_visible(crate::window_scene::WindowId::App(idx as u8))
                {
                    continue;
                }
                self.apps[idx].frame_pulse_pending = false;
                let Some(id) = self.apps[idx].surface_id else { continue };
                let Some(slot) = self.apps[idx].event_channel else { continue };
                let n = self.pulse_frame(id, now, &mut frame);
                let _ = self.client_send(slot, &frame[..n], Delivery::Latest);
            }
            if self.pulses.desktop {
                self.pulses.desktop = false;
                if let (Some(id), Some(slot)) = (self.desktop_surface_id, self.desktop_channel) {
                    let n = self.pulse_frame(id, now, &mut frame);
                    let _ = self.client_send(slot, &frame[..n], Delivery::Latest);
                }
            }
        }
    }
}
