// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Registry client (RFC-0065 dynamic Apps menu): windowd queries `bundlemgrd`'s
//! `OP_LIST_APPS` to populate the topbar Apps dropdown from the **installed bundle
//! registry** instead of a hardcoded list.
//!
//! Uses the production CAP_MOVE request/reply over the init-provisioned
//! `windowd→bundlemgrd` route (the same bahn abilitymgr uses): route to bundlemgrd
//! + our `@reply` inbox, move a reply cap so bundlemgrd answers us, receive the
//! response, and parse it with the host-tested [`crate::app_menu::AppMenu`].
//! Bounded + non-fatal: any routing/IPC failure or malformed reply returns `None`
//! and the caller falls back to [`crate::app_menu::AppMenu::seed`], so the menu
//! never regresses if the registry is briefly unreachable.

#![cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]

use crate::app_menu::AppMenu;
use core::time::Duration;
use nexus_abi::yield_;
use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};

/// init-lite control-channel slots (route requests go through the responder).
const CTRL_SEND_SLOT: u32 = 1;
const CTRL_RECV_SLOT: u32 = 2;

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

/// Best-effort fetch of the Apps menu from the bundle registry. Returns `None`
/// (caller seeds) on any routing/IPC failure or a malformed/empty response.
pub(crate) fn fetch_app_menu() -> Option<AppMenu> {
    let (send_slot, _recv) = route_blocking(b"bundlemgrd")?;
    let (reply_send_slot, reply_recv_slot) = route_blocking(b"@reply")?;

    let mut req = [0u8; 4];
    nexus_abi::bundlemgrd::encode_list_apps(&mut req);

    // Move a clone of our reply-send cap into the request so bundlemgrd replies to
    // our @reply inbox (CAP_MOVE).
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

    // Send (bounded, non-blocking).
    let mut sent = false;
    let mut spins: u32 = 0;
    loop {
        match nexus_abi::ipc_send_v1(send_slot, &hdr, &req, nexus_abi::IPC_SYS_NONBLOCK, 0) {
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

    // Receive the reply on our @reply inbox (bounded). The response carries the full
    // entry list; `AppMenu::from_list_apps_response` parses + bounds it.
    loop {
        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 512];
        match nexus_abi::ipc_recv_v1(
            reply_recv_slot,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                if let Some(menu) = AppMenu::from_list_apps_response(&buf[..n]) {
                    return Some(menu);
                }
                // Unrelated frame on the shared inbox: keep waiting until deadline.
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
