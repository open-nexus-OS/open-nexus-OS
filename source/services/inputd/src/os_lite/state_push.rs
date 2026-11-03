// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: inputd os-lite — the visible-state push to windowd: paced to the display (moves at
//! 120 Hz, edges at once), its one-shot facts delivered once and void when the push failed (a
//! chord replayed to a restarted compositor would act twice, RFC-0112), the outage of the
//! compositor named once. Over the declared slots alone — never a route ask. Moved out of
//! `os_lite.rs` under the structure ratchet.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal

use super::*;

impl LiveRouteRuntime {
    pub(super) fn push_visible_state_to_windowd(&mut self, now_ns: u64, immediate: bool) {
        if !should_push_visible_state(
            self.last_windowd_push_state,
            self.visible_state,
            self.last_windowd_push_ns,
            now_ns,
            immediate,
        ) {
            return;
        }
        // The declared leg (slots 5/6), pinned by init before this task ran — and re-pinned
        // over the dead compositor's capabilities when it restarts (RFC-0112). Never a route
        // ask: init may be busy with that very restart.
        if self.windowd_client.is_none() {
            self.windowd_client =
                KernelClient::new_with_slots(topo::WINDOWD.send, topo::WINDOWD.recv).ok();
        }
        let Some(client) = &self.windowd_client else {
            return;
        };
        let (frame, n) =
            encode_update_visible_state_motion(self.visible_state, self.input.motion());
        match client.send(&frame[..n], Wait::Blocking) {
            Ok(()) => {
                self.no_compositor_said = false;
                // The accumulated wheel delta is DELIVERED: clear it so the
                // next (possibly move-throttled) push cannot replay it.
                (self.pending_wheel_delta, self.visible_state.wheel_delta_y) = (0, 0);
                self.input.clear_motion(); // the pointer's samples went with it (RFC-0113 §2)
                self.note_push_rate(now_ns);
                self.last_windowd_push_state = Some(self.visible_state);
                self.last_windowd_push_ns = now_ns;
                if !self.windowd_push_ok_emitted {
                    let _ = nexus_abi::trace_line("inputd: windowd visible-state pushed");
                    // Input-chain hop I5: normalized state delivered to windowd.
                    let _ = nexus_abi::trace_line("inputd: chain I5 delivered to windowd");
                    self.windowd_push_ok_emitted = true;
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock)
            | Err(nexus_ipc::IpcError::Timeout)
            | Err(nexus_ipc::IpcError::NoSpace) => {
                if self.push_fail_count < 40 {
                    self.push_fail_count += 1;
                    let _ = debug_println(&format!(
                        "inputd: push backpressure n={}",
                        self.push_fail_count
                    ));
                }
                // Backpressure only: keep route/client and retry next tick.
            }
            Err(e) => {
                // Unreachable windowd: the sample is spent for pacing purposes (the next
                // event carries fresh state) — the timer never chases a dead route. Its wheel
                // delta was the dead compositor's: replayed to the next one it would scroll twice
                // (the chords and capture keys ride the key channel, dropped there the same way).
                (self.pending_wheel_delta, self.visible_state.wheel_delta_y) = (0, 0);
                (self.last_windowd_push_state, self.last_windowd_push_ns) =
                    (Some(self.visible_state), now_ns);
                self.input.clear_motion();
                // RFC-0112: no compositor runs (its restart) — named, not a broken route.
                if matches!(
                    e,
                    nexus_ipc::IpcError::Disconnected
                        | nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::NoSuchEndpoint)
                ) {
                    if !core::mem::replace(&mut self.no_compositor_said, true) {
                        let _ = debug_println("inputd: state not pushed (no compositor)");
                    }
                } else if !self.windowd_push_fail_emitted {
                    let _ = debug_println("inputd: windowd visible-state push fail");
                    // Input-chain hop I5 fail: windowd unreachable (route dropped).
                    let _ = debug_println("inputd: chain I5 deliver FAIL (windowd route)");
                    self.windowd_push_fail_emitted = true;
                }
                self.windowd_client = None;
            }
        }
    }
}
