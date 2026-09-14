// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd→sessiond session client (TASK-0065B, TASK-0324 P7-c): windowd subscribes
//! ONCE (`OP_WATCH`, a moved SEND cap of its declared push channel) and sessiond PUSHES every
//! state — greeter-vs-active + the user registry — on it. No query, no probe cadence, no
//! login poll: windowd only renders and relays, session state lives in sessiond, never here.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (OS-only IPC; frame codecs are host-tested in
//! `nexus_abi::sessiond`, the greeter hit-tests in `interaction`)
//!

#![cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]

use alloc::string::String;
use alloc::vec::Vec;
use nexus_abi::sessiond as wire;

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

/// Subscribes windowd's session push channel at sessiond (`OP_WATCH`, TASK-0324 P7-c): a
/// clone of the channel's SEND half travels with the request; sessiond acknowledges on it and
/// pushes its state there at once and on every transition. Sent ONCE, nothing waits: sessiond
/// (stage `SessionStart`) drains its queue when it starts — after windowd has reported
/// `DisplayReady` — and the pushes wake the compositor loop's waitset.
pub(crate) fn subscribe_session_watch() -> bool {
    use nexus_service_topology::slots::windowd as topo;
    let Ok(clone) = nexus_abi::cap_clone(topo::SESSION_WATCH_SEND) else {
        return false;
    };
    let mut req = [0u8; 4];
    wire::encode_watch_req(&mut req);
    let hdr =
        nexus_abi::MsgHeader::new(clone, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, req.len() as u32);
    match nexus_abi::ipc_send_v1(topo::SESSIOND.send, &hdr, &req, 0, 0) {
        Ok(_) => true,
        Err(_) => {
            let _ = nexus_abi::cap_close(clone);
            false
        }
    }
}

/// One frame from the session push channel as a snapshot; the `OP_WATCH` acknowledgement and
/// anything that is not a state push decode to `None`.
pub(crate) fn decode_session_push(frame: &[u8]) -> Option<SessionSnapshot> {
    if wire::is_watch_ack(frame) {
        return None;
    }
    parse_state_rsp(frame)
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
