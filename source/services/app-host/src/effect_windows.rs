// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0086 shell-side window state — the sid↔app-id join over the
//! compositor's window feed (`OP_SURFACE_WINDOWS`) and the
//! `svc.shell.activate` verb (restore/raise a running window via windowd, or
//! launch when nothing runs). Split out of `effect_host.rs` (structure
//! ratchet); the decision which of the two to take lives HERE, not in the
//! DSL — the store only says "activate".
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: markers on the QEMU path (`apphost: dsl svc shell.activate
//! ok`, `apphost: shell activate -> launch`).
//! RFC: docs/rfcs/RFC-0086-shell-taskbar-window-feed.md

use crate::effect_host::{AppEffectHost, ERR_SVC_UNAVAILABLE};
use crate::probe::raw_marker;
use nexus_dsl_runtime::Value;

impl AppEffectHost {
    /// The kernel service id execd stamps on an app-host child
    /// (RFC-0086: `app:<bundle_id>` via `exec_v2`). The userspace FNV mirror
    /// is the join key between the compositor's sid-only feed and the
    /// registry's bundle ids — no wire carries both.
    pub(crate) fn app_service_id(app_id: &str) -> u64 {
        let mut name = alloc::string::String::from("app:");
        name.push_str(app_id);
        nexus_abi::service_id_from_name(name.as_bytes())
    }

    /// `(running, minimized, focused)` for a bundle id, from the cached feed.
    /// Several windows of one app collapse with OR — the taskbar tile is
    /// per-APP, so "any window minimized" reads as minimized and "any window
    /// focused" as focused.
    pub(crate) fn window_state_of(&self, app_id: &str) -> (bool, bool, bool) {
        use nexus_display_proto::surface_windows as feed;
        let sid = Self::app_service_id(app_id);
        let (mut running, mut minimized, mut focused) = (false, false, false);
        for w in &self.windows[..self.windows_len] {
            if w.owner_sid != sid {
                continue;
            }
            running = true;
            minimized |= w.flags & feed::WINDOW_FLAG_MINIMIZED != 0;
            focused |= w.flags & feed::WINDOW_FLAG_FOCUSED != 0;
        }
        (running, minimized, focused)
    }

    /// `svc.shell.activate(app_id)`: bring the app's window forward. RUNNING
    /// (its sid is in the feed) ⇒ ask windowd to restore/raise it
    /// (`OP_SURFACE_TASKBAR`); otherwise fall back to a normal launch. The
    /// decision lives HERE, not in the DSL: the store just says "activate".
    pub(crate) fn shell_activate(&mut self, app_id: &str) -> Result<Value, u32> {
        let sid = Self::app_service_id(app_id);
        let running = self.windows[..self.windows_len].iter().any(|w| w.owner_sid == sid);
        if !running {
            raw_marker("apphost: shell activate -> launch");
            return self.ability_launch(app_id);
        }
        #[cfg(nexus_env = "os")]
        {
            use nexus_display_proto::surface_windows as feed;
            // The windowd surface request slot (main.rs WINDOWD_SEND_SLOT) —
            // the same channel `CONTROL_WIN_*` rides; windowd's sid gate is
            // the enforcement point.
            const WINDOWD_SEND_SLOT: u32 = 5;
            let frame = feed::encode_surface_taskbar(feed::TASKBAR_ACTIVATE, sid);
            let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
            if nexus_abi::ipc_send_v1(
                WINDOWD_SEND_SLOT,
                &hdr,
                &frame,
                nexus_abi::IPC_SYS_NONBLOCK,
                0,
            )
            .is_err()
            {
                raw_marker("apphost: FAIL shell activate send");
                return Err(ERR_SVC_UNAVAILABLE);
            }
        }
        raw_marker("apphost: dsl svc shell.activate ok");
        Ok(Value::Bool(true))
    }
}
