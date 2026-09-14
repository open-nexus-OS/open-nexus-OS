// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — the session decision (TASK-0065B, TASK-0324 P7-c):
//! sessiond PUSHES its state (greeter owns the display / a session is active) on windowd's
//! declared push channel, a member of the compositor loop's waitset; windowd applies the
//! resolved SystemUI shell product. No probe, no cadence, no clock: the decision arrives
//! when the session authority has it.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (OS-only IPC; the wire codecs are host-tested in
//! `nexus_abi::sessiond`, the session state machine in `sessiond`)

use super::*;

#[cfg(all(nexus_env = "os", target_os = "none"))]
impl DisplayServerRuntime {
    /// The session decision has been made (session applied or greeter up). Until then NO
    /// shell surface may composite and no shell affordance may react — the desktop must
    /// never flash before login (TASK-0065B ordering: splash → login → shell).
    pub(super) fn session_resolved(&self) -> bool {
        self.session_resolved
    }

    /// Drains the session push channel (a waitset member of the compositor loop, TASK-0324
    /// P7-c): every frame sessiond pushed is applied in order. Nothing here asks or waits.
    pub(crate) fn drain_session_pushes(&mut self) {
        let mut buf = [0u8; 512];
        loop {
            let mut hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
            let Ok(n) = nexus_abi::ipc_recv_v1(
                nexus_service_topology::slots::windowd::SESSION_WATCH_RECV,
                &mut hdr,
                &mut buf,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) else {
                return;
            };
            let n = core::cmp::min(n as usize, buf.len());
            if let Some(snapshot) = crate::session_client::decode_session_push(&buf[..n]) {
                self.on_session_push(snapshot);
            }
        }
    }

    /// One pushed snapshot: the first resolves the session decision; a later one that reports
    /// an ACTIVE session while the login phase owns the display is the login itself
    /// (the DSL greeter logged in out of process) — apply the session shell.
    fn on_session_push(&mut self, snapshot: crate::session_client::SessionSnapshot) {
        if !self.session_resolved {
            self.session_resolved = true;
            self.on_session_snapshot(snapshot);
            return;
        }
        if self.greeter_login_watch && snapshot.state == nexus_abi::sessiond::STATE_ACTIVE {
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
        }
    }

    /// True while the LOGIN PHASE owns the display: chrome + dock stay
    /// suppressed until the session activates. (The built-in avatar greeter is
    /// deleted — the DSL greeter app-host is the login UI; this flag is the
    /// session gate that used to be `greeter.is_some()`.)
    pub(super) fn greeter_active(&self) -> bool {
        self.greeter_login_watch
    }

    /// Shell chrome composites only when the config enables it, the SESSION
    /// DECISION has been made, no login phase owns the display (TASK-0065B),
    /// and no FULLSCREEN window covers it (TASK-0070 Phase 2): the boot order
    /// is splash → login → shell.
    /// (Policy predicate retained: documents the splash → login → shell display
    /// ownership contract even though the DSL shell owns chrome today.)
    #[allow(dead_code)]
    pub(super) fn chrome_composited(&self) -> bool {
        self.shell_config.desktop_chrome
            && self.session_resolved()
            && !self.greeter_active()
            && self.windows.fullscreen_active().is_none()
    }

    /// Terminal probe outcome: apply what the session authority reported.
    fn on_session_snapshot(&mut self, snapshot: crate::session_client::SessionSnapshot) {
        use nexus_abi::sessiond as wire;
        match snapshot.state {
            wire::STATE_ACTIVE => {
                // Resolved user session selects the SystemUI shell product.
                let product = snapshot.active_product().unwrap_or(systemui::DEFAULT_PRODUCT_ID);
                self.apply_session_shell(product);
            }
            wire::STATE_GREETER => {
                // The login phase owns the display until a user logs in: the
                // DSL greeter app-host IS the login UI (the built-in avatar
                // greeter is DELETED per the cleanup map — user-verified DSL
                // login 2026-07-10). bundle_type=greeter passes abilitymgr's
                // pre-session gate; its surface declares `level: desktop`.
                self.launch_app("greeter");
                // Arm the login watch: chrome stays suppressed (`greeter_active`) until
                // sessiond PUSHES the ACTIVE state after the out-of-process login
                // (`svc.session.login`); then the session shell is applied.
                self.greeter_login_watch = true;
                let _ = debug_println("windowd: greeter on (dsl)");
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
