// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd→settingsd settings client (TASK-0072 Phase 10): `OP_GET`
//! reads the persisted `ui.theme.mode` at boot so a saved light/dark choice is
//! restored across reboots; `OP_SET` writes the user's toggle so settingsd
//! validates + persists it (via statefsd) + applies. windowd only renders and
//! relays — the typed registry + persistence live in settingsd, never here.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (OS-only IPC; the frame codecs are host-tested in
//! `nexus_abi::settingsd`, the registry/validation in `settingsd`)
//!
//! Same production CAP_MOVE request/reply bahn as [`crate::session_client`]:
//! route to settingsd + our `@reply` inbox, move a reply cap, bounded receive.
//! Best-effort and non-fatal: any failure returns `None`/`false` and the caller
//! keeps the build-time default (Dark) — boot never bricks on a missing or slow
//! settings authority.

#![cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]

use alloc::vec::Vec;
use core::time::Duration;
use nexus_abi::settingsd as wire;
use nexus_abi::yield_;
use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};

/// init-lite control-channel slots (route requests go through the responder).
const CTRL_SEND_SLOT: u32 = 1;
const CTRL_RECV_SLOT: u32 = 2;

/// Best-effort GET of `ui.theme.mode`. Returns the parsed [`crate::theme::ThemeMode`]
/// on an OK reply, `None` on any routing/IPC failure, a non-OK status, or an
/// unrecognized value (caller keeps its default and may retry on its cadence).
pub(crate) fn get_theme_mode() -> Option<crate::theme::ThemeMode> {
    let mut req = [0u8; 32];
    let len = wire::encode_get_req(wire::KEY_UI_THEME_MODE, &mut req)?;
    let rsp = request_reply(&req[..len])?;
    let (status, value) = wire::decode_response(wire::OP_GET, &rsp)?;
    if status != wire::STATUS_OK {
        return None;
    }
    crate::theme::ThemeMode::from_str(value)
}

/// Best-effort SET of `ui.theme.mode` (the settings-panel toggle). settingsd
/// validates → persists (statefsd) → applies; returns `true` only on an OK
/// reply. A transport failure returns `false` — the live in-window switch has
/// already happened; only the persisted value is missed (retried on next SET).
pub(crate) fn set_theme_mode(mode: crate::theme::ThemeMode) -> bool {
    let mut req = [0u8; 48];
    let Some(len) = wire::encode_set_req(wire::KEY_UI_THEME_MODE, mode.as_str(), &mut req) else {
        return false;
    };
    let Some(rsp) = request_reply(&req[..len]) else {
        return false;
    };
    matches!(wire::decode_response(wire::OP_SET, &rsp), Some((wire::STATUS_OK, _)))
}

/// Best-effort SET of `ui.theme.accent` (palette index → its registered
/// name). Same contract as `set_theme_mode`: the live push already happened,
/// a transport failure only misses reboot persistence.
pub(crate) fn set_theme_accent(index: u8) -> bool {
    let name = match index as usize {
        0 => "default",
        i if i <= nexus_theme_tokens::ACCENT_PALETTE.len() => {
            nexus_theme_tokens::ACCENT_PALETTE[i - 1].0
        }
        _ => return false,
    };
    let mut req = [0u8; 48];
    let Some(len) = wire::encode_set_req(nexus_abi::settingsd::KEY_UI_THEME_ACCENT, name, &mut req)
    else {
        return false;
    };
    let Some(rsp) = request_reply(&req[..len]) else {
        return false;
    };
    matches!(wire::decode_response(wire::OP_SET, &rsp), Some((wire::STATUS_OK, _)))
}

/// Best-effort GET of `ui.theme.accent` → the palette index; `None` on any
/// failure (caller keeps 0 = built-in accent). The boot restore twin of
/// `set_theme_accent`.
pub(crate) fn get_theme_accent() -> Option<u8> {
    let mut req = [0u8; 32];
    let len = wire::encode_get_req(nexus_abi::settingsd::KEY_UI_THEME_ACCENT, &mut req)?;
    let rsp = request_reply(&req[..len])?;
    let (status, value) = wire::decode_response(wire::OP_GET, &rsp)?;
    if status != wire::STATUS_OK {
        return None;
    }
    nexus_theme_tokens::accent_index(value)
}

/// Best-effort GET of `ui.shell.mode` (`"tablet"`/`"desktop"`); `None` on any
/// failure or unknown value (caller keeps the SystemUI boot default).
pub(crate) fn get_shell_mode() -> Option<&'static str> {
    let mut req = [0u8; 32];
    let len = wire::encode_get_req(wire::KEY_UI_SHELL_MODE, &mut req)?;
    let rsp = request_reply(&req[..len])?;
    let (status, value) = wire::decode_response(wire::OP_GET, &rsp)?;
    if status != wire::STATUS_OK {
        return None;
    }
    match value {
        "tablet" => Some("tablet"),
        "desktop" => Some("desktop"),
        _ => None,
    }
}

/// Best-effort SET of `ui.shell.mode` (the Control-Center Desktop/Tablet
/// toggle). The live shell switch already happened in windowd; a transport
/// failure only misses reboot persistence.
pub(crate) fn set_shell_mode(mode: &str) -> bool {
    let mut req = [0u8; 48];
    let Some(len) = wire::encode_set_req(wire::KEY_UI_SHELL_MODE, mode, &mut req) else {
        return false;
    };
    let Some(rsp) = request_reply(&req[..len]) else {
        return false;
    };
    matches!(wire::decode_response(wire::OP_SET, &rsp), Some((wire::STATUS_OK, _)))
}

/// Resolves a service (or `@reply`) to its `(send, recv)` slots via the responder.
fn route_blocking(name: &[u8]) -> Option<(u32, u32)> {
    match budget::route_with_nonce_budgeted(
        name,
        CTRL_SEND_SLOT,
        CTRL_RECV_SLOT,
        Duration::from_secs(2),
        NonceMismatchBudget::new(64),
    ) {
        RouteRetryOutcome::Success { send_slot, recv_slot } => Some((send_slot, recv_slot)),
        _ => None,
    }
}

/// One bounded CAP_MOVE request/reply exchange with settingsd (the session/
/// registry-client recipe: clone reply-send cap, NONBLOCK send with a yield
/// budget, bounded receive on the shared `@reply` inbox, filter to our magic).
fn request_reply(req: &[u8]) -> Option<Vec<u8>> {
    let (send_slot, _recv) = route_blocking(b"settingsd")?;
    let (reply_send_slot, reply_recv_slot) = route_blocking(b"@reply")?;

    let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).ok()?;
    let hdr = nexus_abi::MsgHeader::new(
        reply_send_clone,
        0,
        0,
        nexus_abi::ipc_hdr::CAP_MOVE,
        req.len() as u32,
    );

    let start = nexus_abi::nsec().unwrap_or(0);
    let deadline = start.saturating_add(500_000_000); // 500ms bound

    let mut sent = false;
    let mut spins: u32 = 0;
    loop {
        match nexus_abi::ipc_send_v1(send_slot, &hdr, req, nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => {
                sent = true;
                break;
            }
            Err(nexus_abi::IpcError::QueueFull) => {
                if nexus_abi::nsec().unwrap_or(0) >= deadline || spins >= 200_000 {
                    break;
                }
                spins = spins.saturating_add(1);
                let _ = yield_();
            }
            Err(_) => break,
        }
    }
    let _ = nexus_abi::cap_close(reply_send_clone);
    if !sent {
        return None;
    }

    loop {
        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 128];
        match nexus_abi::ipc_recv_v1(
            reply_recv_slot,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                // Only accept frames of OUR protocol; unrelated frames on the
                // shared inbox are skipped until the deadline.
                if n >= 4 && buf[0] == wire::MAGIC0 && buf[1] == wire::MAGIC1 {
                    let mut out = Vec::with_capacity(n);
                    out.extend_from_slice(&buf[..n]);
                    return Some(out);
                }
                if nexus_abi::nsec().unwrap_or(0) >= deadline {
                    return None;
                }
                let _ = yield_();
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                if nexus_abi::nsec().unwrap_or(0) >= deadline {
                    return None;
                }
                let _ = yield_();
            }
            Err(_) => return None,
        }
    }
}
