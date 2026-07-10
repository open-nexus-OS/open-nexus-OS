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

/// Login-watch cadence (Umbau #17): while the DSL greeter owns the display,
/// poll sessiond for the login on a slow, bounded-rate cadence. Login is a
/// rare human-latency event — 500ms costs nothing and needs no push channel.
#[cfg(all(nexus_env = "os", target_os = "none"))]
const GREETER_WATCH_INTERVAL_NS: u64 = 500_000_000;

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
    /// The session decision has been made (session applied, greeter up, or
    /// the auto-shell fallback). Until then NO shell surface may composite
    /// and no shell affordance may react — the desktop must never flash
    /// before login (TASK-0065B ordering: splash → login → shell).
    pub(super) fn session_resolved(&self) -> bool {
        self.session_probe.resolved
    }

    /// One SYNCHRONOUS probe attempt during the first-frame handoff, BEFORE
    /// the first present is built: sessiond is ready long before windowd's
    /// handoff, so in the normal boot the very first revealed frame already
    /// carries the session decision (the greeter layer — login directly after
    /// the boot logo, the desktop never flashes). Bounded (the client's 500ms
    /// recv deadline); a miss falls back to the cadenced probe below.
    pub(crate) fn session_probe_at_handoff(&mut self) {
        if self.session_probe.resolved {
            return;
        }
        self.session_probe.attempts = self.session_probe.attempts.saturating_add(1);
        match crate::session_client::fetch_session_state() {
            Some(snapshot) => {
                self.session_probe.resolved = true;
                self.on_session_snapshot(snapshot);
            }
            None => {
                let _ = debug_println("windowd: session probe retry (post-handoff)");
            }
        }
    }

    /// One probe step, called from the main loop. Returns `true` while the
    /// probe still needs pacing wakes (the loop is otherwise fully blocking).
    pub(crate) fn session_probe_tick(&mut self, now_ns: u64) -> bool {
        if self.session_probe.resolved {
            return false;
        }
        // The handoff path runs its own synchronous attempt; the cadenced
        // retries below only cover the sessiond-slow/unreachable cases.
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
                // Chrome was session-gated until now — one full present
                // brings the auto shell up.
                self.queue_gpu_blit_rect(DamageRect {
                    x: 0,
                    y: 0,
                    width: self.mode.width,
                    height: self.mode.height,
                });
                false
            }
            None => true,
        }
    }

    /// One login-watch step (Umbau #17 swap), called from the main loop next
    /// to the session probe. Armed by `swap_greeter_to_dsl` (the DSL greeter
    /// took the display); polls sessiond until the out-of-process login lands,
    /// then applies the session shell. Returns `true` while it needs pacing
    /// wakes. Rate-bounded; runs only between swap and login.
    pub(crate) fn greeter_watch_tick(&mut self, now_ns: u64) -> bool {
        if !self.greeter_login_watch {
            return false;
        }
        if now_ns < self.greeter_watch_next_ns {
            return true;
        }
        self.greeter_watch_next_ns = now_ns.saturating_add(GREETER_WATCH_INTERVAL_NS);
        let Some(snapshot) = crate::session_client::fetch_session_state() else {
            return true;
        };
        if snapshot.state != nexus_abi::sessiond::STATE_ACTIVE {
            return true;
        }
        self.greeter_login_watch = false;
        let product = snapshot.active_product().unwrap_or(systemui::DEFAULT_PRODUCT_ID);
        let _ = debug_println("windowd: dsl login detected (session active)");
        self.apply_session_shell(product);
        self.queue_gpu_blit_rect(DamageRect {
            x: 0,
            y: 0,
            width: self.mode.width,
            height: self.mode.height,
        });
        false
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
                // The login window owns the display until a user logs in. The
                // built-in avatar greeter comes up FIRST (fail-safe: login
                // works even if the app-host chain breaks) …
                self.start_greeter(&snapshot.users);
                // … then the DSL greeter app-host launches (bundle_type=
                // greeter passes abilitymgr's pre-session gate). Its first
                // desktop-surface present retires the avatar greeter
                // (`swap_greeter_to_dsl`) and arms `greeter_watch_tick`,
                // which applies the session shell once the out-of-process
                // login (`svc.session.login`) lands.
                self.launch_app("greeter");
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
        // TASK-0080C #17 (transitional): launch the shell as a real RFC-0065
        // app-host process NOW — the session is ACTIVE, so abilitymgr's session
        // gate authorizes it (launching at boot was denied pre-login every
        // boot). Its surface declares `level: desktop` and lands in the Desktop
        // z-band (`app_stack_id`), composed as the base layer. Once per session
        // lifetime; additive alongside the in-process mount — the in-process
        // mount is DELETED (2d); the app-host desktop surface IS the shell.
        if !self.shell_app_launched {
            self.shell_app_launched = true;
            self.launch_app("desktop-shell");
        }
    }
}
