// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — the session probe (TASK-0065B): after
//! the framebuffer handoff, ask the session authority (`sessiond`) whether a
//! session is active or the greeter owns the display, and apply the resolved
//! SystemUI shell product. Bounded retries; sessiond unreachable = auto shell
//! (today's behavior) — the probe can degrade the experience, never brick boot.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (OS-only IPC; the wire codecs are host-tested in
//! `nexus_abi::sessiond`, the session state machine in `sessiond`)

use super::*;

/// Probe cadence: sessiond spawns last, so the first attempts may race its
/// bind. 250ms × 24 = ~6s bound before the auto-shell fallback.
#[cfg(all(nexus_env = "os", target_os = "none"))]
const SESSION_PROBE_INTERVAL_NS: u64 = 250_000_000;
#[cfg(all(nexus_env = "os", target_os = "none"))]
const SESSION_PROBE_MAX_ATTEMPTS: u32 = 24;

/// Session-probe bookkeeping on the runtime.
#[derive(Default)]
pub(super) struct SessionProbe {
    /// The probe reached a terminal outcome (session applied / greeter / fallback).
    pub resolved: bool,
    /// Failed attempts so far.
    pub attempts: u32,
    /// Monotonic deadline before the next attempt.
    pub next_try_ns: u64,
}

#[cfg(all(nexus_env = "os", target_os = "none"))]
impl DisplayServerRuntime {
    /// One probe step, called from the main loop. Returns `true` while the
    /// probe still needs pacing wakes (the loop is otherwise fully blocking).
    pub(crate) fn session_probe_tick(&mut self, now_ns: u64) -> bool {
        if self.session_probe.resolved {
            return false;
        }
        // The session decision must never delay the boot reveal: probe only
        // after the present handoff is done and the desktop base is up.
        if self.is_handoff_pending() {
            return true;
        }
        if now_ns < self.session_probe.next_try_ns {
            return true;
        }
        self.session_probe.next_try_ns = now_ns.saturating_add(SESSION_PROBE_INTERVAL_NS);
        self.session_probe.attempts = self.session_probe.attempts.saturating_add(1);
        match crate::session_client::fetch_session_state() {
            Some(snapshot) => {
                self.session_probe.resolved = true;
                self.on_session_snapshot(snapshot);
                false
            }
            None if self.session_probe.attempts >= SESSION_PROBE_MAX_ATTEMPTS => {
                self.session_probe.resolved = true;
                let _ = debug_println("windowd: session unavailable (auto shell)");
                false
            }
            None => true,
        }
    }

    /// Terminal probe outcome: apply what the session authority reported.
    fn on_session_snapshot(&mut self, snapshot: crate::session_client::SessionSnapshot) {
        use nexus_abi::sessiond as wire;
        match snapshot.state {
            wire::STATE_ACTIVE => {
                // Resolved user session selects the SystemUI shell product.
                let product =
                    snapshot.active_product().unwrap_or(systemui::DEFAULT_PRODUCT_ID);
                self.apply_session_shell(product);
            }
            wire::STATE_GREETER => {
                // The login window owns the display until a user logs in.
                self.start_greeter(&snapshot.users);
            }
            _ => {
                let _ = debug_println("windowd: session unavailable (auto shell)");
            }
        }
    }

    /// Apply the session-selected shell product. Identical product = today's
    /// boot config already renders — marker only, zero visual change.
    pub(super) fn apply_session_shell(&mut self, product: &str) {
        if product != self.shell_config.product_id {
            match systemui::resolve_product(product) {
                Ok(cfg) => self.apply_shell_config(systemui::ShellConfig::from_resolved(&cfg)),
                Err(_) => {
                    let _ = debug_println(&alloc::format!(
                        "windowd: session product unknown (product={product}, auto shell)"
                    ));
                    return;
                }
            }
        }
        let _ = debug_println(&alloc::format!(
            "windowd: session shell visible (product={})",
            self.shell_config.product_id
        ));
    }
}
