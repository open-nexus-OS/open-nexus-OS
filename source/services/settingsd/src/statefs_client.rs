// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! statefsd persistence client (TASK-0072 Phase 8): loads/stores settingsd's
//! prefs blob in statefsd's journaled KV store so values survive a reboot.
//! Mirrors the windowd→sessiond registry-client recipe: route to statefsd via
//! the responder, one bounded CAP_MOVE request/reply on the shared `@reply`
//! inbox, best-effort (a routing/policy/IPC failure degrades to defaults —
//! never a boot failure).
#![cfg(all(nexus_env = "os", feature = "os-lite"))]

use alloc::string::String;
use alloc::vec::Vec;
use core::time::Duration;

use nexus_abi::yield_;
use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};

/// init-lite control-channel slots (route requests go through the responder).
const CTRL_SEND_SLOT: u32 = 1;
const CTRL_RECV_SLOT: u32 = 2;

// statefsd v1 wire (userspace/statefs `protocol`): the on-disk KV contract.
const SF_MAGIC0: u8 = b'S';
const SF_MAGIC1: u8 = b'F';
const SF_VERSION: u8 = 1;
const SF_VERSION_V2: u8 = 2;
const SF_OP_PUT: u8 = 1;
const SF_OP_GET: u8 = 2;
const SF_STATUS_OK: u8 = 0;

/// settingsd's key in statefsd's flat KV store. Stable across boots (a const),
/// so the same overrides load back every time.
const PREFS_KEY: &str = "settingsd/prefs";

/// Load the persisted prefs blob, or `None` when statefsd is unreachable /
/// the key is unset. The returned string is the `key=value\n` override blob
/// [`crate::registry::SettingsRegistry::load_prefs_blob`] consumes.
pub(crate) fn load_prefs() -> Option<String> {
    let mut req = Vec::with_capacity(6 + PREFS_KEY.len());
    req.extend_from_slice(&[SF_MAGIC0, SF_MAGIC1, SF_VERSION, SF_OP_GET]);
    req.extend_from_slice(&(PREFS_KEY.len() as u16).to_le_bytes());
    req.extend_from_slice(PREFS_KEY.as_bytes());
    let rsp = request_reply(&req)?;
    decode_get_value(&rsp).and_then(|v| String::from_utf8(v).ok())
}

/// Persist the prefs blob (atomic single PUT — statefsd journals it). Returns
/// true when statefsd acked OK. The caller keeps the in-memory value regardless
/// (set already validated); a persist failure only means "won't survive reboot".
pub(crate) fn store_prefs(blob: &str) -> bool {
    let val = blob.as_bytes();
    let mut req = Vec::with_capacity(10 + PREFS_KEY.len() + val.len());
    req.extend_from_slice(&[SF_MAGIC0, SF_MAGIC1, SF_VERSION, SF_OP_PUT]);
    req.extend_from_slice(&(PREFS_KEY.len() as u16).to_le_bytes());
    req.extend_from_slice(&(val.len() as u32).to_le_bytes());
    req.extend_from_slice(PREFS_KEY.as_bytes());
    req.extend_from_slice(val);
    match request_reply(&req) {
        Some(rsp) => decode_put_ok(&rsp),
        None => false,
    }
}

/// Parse a statefsd GET response value (v1 9-byte or v2 17-byte header),
/// `None` on any non-OK status or malformed frame.
fn decode_get_value(frame: &[u8]) -> Option<Vec<u8>> {
    if frame.len() < 5 || frame[0] != SF_MAGIC0 || frame[1] != SF_MAGIC1 {
        return None;
    }
    if frame[3] != (SF_OP_GET | 0x80) || frame[4] != SF_STATUS_OK {
        return None;
    }
    let (hdr, len_at) = match frame[2] {
        SF_VERSION => (9usize, 5usize),
        SF_VERSION_V2 => (17usize, 13usize),
        _ => return None,
    };
    if frame.len() < hdr {
        return None;
    }
    let vlen = u32::from_le_bytes([
        frame[len_at],
        frame[len_at + 1],
        frame[len_at + 2],
        frame[len_at + 3],
    ]) as usize;
    (frame.len() == hdr + vlen).then(|| frame[hdr..hdr + vlen].to_vec())
}

/// True when a statefsd PUT response reports OK (v1 or v2 status frame).
fn decode_put_ok(frame: &[u8]) -> bool {
    frame.len() >= 5
        && frame[0] == SF_MAGIC0
        && frame[1] == SF_MAGIC1
        && frame[3] == (SF_OP_PUT | 0x80)
        && frame[4] == SF_STATUS_OK
}

/// Resolve a service (or `@reply`) to its `(send, recv)` slots via the responder.
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

/// One bounded CAP_MOVE request/reply exchange with statefsd (the registry-
/// client recipe: clone the reply-send cap, NONBLOCK send with a yield budget,
/// bounded receive on the shared `@reply` inbox, filter on statefs magic).
fn request_reply(req: &[u8]) -> Option<Vec<u8>> {
    let (send_slot, _recv) = route_blocking(b"statefsd")?;
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
        let mut buf = [0u8; 1024];
        match nexus_abi::ipc_recv_v1(
            reply_recv_slot,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                // Accept only OUR protocol frames; skip unrelated inbox traffic.
                if n >= 4 && buf[0] == SF_MAGIC0 && buf[1] == SF_MAGIC1 {
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
