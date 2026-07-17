// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd→sessiond session client — queries the session authority
//! (TASK-0065B): `OP_GET_STATE` to learn greeter-vs-active + the user
//! registry, `OP_LOGIN` to relay the greeter's avatar click. windowd only
//! renders and relays — session state lives in sessiond, never here.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (OS-only IPC; frame codecs are host-tested in
//! `nexus_abi::sessiond`, the greeter hit-tests in `interaction`)
//!
//! Same production CAP_MOVE request/reply bahn as [`crate::registry_client`]:
//! route to sessiond + our `@reply` inbox, move a reply cap, bounded receive.
//! Best-effort and non-fatal: any failure returns `None` and the caller's
//! probe either retries or falls back to the auto shell — boot never bricks
//! on a missing session authority.

#![cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]

use alloc::string::String;
use alloc::vec::Vec;
use core::time::Duration;
use nexus_abi::sessiond as wire;
use nexus_abi::yield_;
use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};

/// init-lite control-channel slots (route requests go through the responder).
const CTRL_SEND_SLOT: u32 = 1;
const CTRL_RECV_SLOT: u32 = 2;

/// One registered user, as reported by sessiond's GET_STATE.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SessionUser {
    /// Stable user id (LOGIN takes this).
    pub id: String,
    /// Name shown on the greeter.
    pub display_name: String,
    /// SystemUI product id selecting this user's shell.
    pub product: String,
}

/// Snapshot of the session authority's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SessionSnapshot {
    /// Wire state (`wire::STATE_GREETER` / `STATE_ACTIVE` / `STATE_LOCKED`).
    pub state: u8,
    /// Index into `users` of the active user, when a session exists.
    pub active_idx: Option<usize>,
    /// The registered users.
    pub users: Vec<SessionUser>,
}

impl SessionSnapshot {
    /// The active user's SystemUI product id, when a session is active.
    pub fn active_product(&self) -> Option<&str> {
        let idx = self.active_idx?;
        self.users.get(idx).map(|u| u.product.as_str())
    }
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

/// Best-effort GET_STATE query. `None` on any routing/IPC failure or a
/// malformed reply (the caller retries on its probe cadence).
pub(crate) fn fetch_session_state() -> Option<SessionSnapshot> {
    let mut req = [0u8; 4];
    wire::encode_get_state(&mut req);
    let rsp = request_reply(&req)?;
    parse_state_rsp(&rsp)
}

/// One bounded CAP_MOVE request/reply exchange with sessiond (the
/// registry-client recipe: clone reply-send cap, NONBLOCK send with yield
/// budget, bounded receive on the shared `@reply` inbox).
fn request_reply(req: &[u8]) -> Option<Vec<u8>> {
    let (send_slot, _recv) = route_blocking(b"sessiond")?;
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

/// Parses a GET_STATE response into a snapshot.
fn parse_state_rsp(frame: &[u8]) -> Option<SessionSnapshot> {
    let (status, state, active_idx, count) = wire::decode_get_state_header(frame)?;
    if status != wire::STATUS_OK {
        return None;
    }
    let mut users = Vec::new();
    let mut at = wire::GET_STATE_BODY_OFFSET;
    for _ in 0..count {
        let id = read_field(frame, &mut at)?;
        let name = read_field(frame, &mut at)?;
        let product = read_field(frame, &mut at)?;
        users.push(SessionUser { id, display_name: name, product });
    }
    let active_idx = if active_idx == wire::NO_ACTIVE_USER {
        None
    } else {
        let idx = active_idx as usize;
        if idx >= users.len() {
            return None;
        }
        Some(idx)
    };
    Some(SessionSnapshot { state, active_idx, users })
}

/// Reads one `[len:u8, bytes...]` UTF-8 field at `*at`, advancing it.
fn read_field(frame: &[u8], at: &mut usize) -> Option<String> {
    let len = *frame.get(*at)? as usize;
    let start = *at + 1;
    let end = start.checked_add(len)?;
    if end > frame.len() {
        return None;
    }
    *at = end;
    core::str::from_utf8(&frame[start..end]).ok().map(String::from)
}
