// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! statefsd persistence client (TASK-0072 Phase 8, RFC-0083 async): loads
//! settingsd's prefs blob at boot (one bounded kernel-parked round-trip) and
//! carries the ASYNC persist path — a NONBLOCK PUT send plus a NONBLOCK reply
//! drain, both driven by `persist::Persister` from the service loop. Routes
//! are resolved once and cached (named routes are persistent slots). Best-
//! effort throughout: a routing/policy/IPC failure degrades to defaults /
//! retry-with-backoff — never a boot failure, never a blocked client.
#![cfg(all(nexus_env = "os", feature = "os-lite"))]

use alloc::string::String;
use alloc::vec::Vec;
use core::time::Duration;

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
/// so the same overrides load back every time. statefsd's journal engine
/// accepts ONLY `/state/`-rooted keys (validate_key) — the original bare
/// `settingsd/prefs` earned STATUS_INVALID_KEY on every PUT, which was the
/// actual `persist=fail` after the route was wired.
const PREFS_KEY: &str = "/state/settingsd/prefs";

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

/// NONBLOCK send of one prefs PUT (RFC-0083 async persist). Returns whether
/// the frame LEFT — the reply arrives later via [`poll_put_reply`]. A refused
/// send (full queue / no route) is the caller's backoff signal, never a spin.
pub(crate) fn try_send_put(blob: &str) -> bool {
    let Some((send_slot, reply_send_slot, _)) = cached_slots() else {
        return false;
    };
    let val = blob.as_bytes();
    let mut req = Vec::with_capacity(10 + PREFS_KEY.len() + val.len());
    req.extend_from_slice(&[SF_MAGIC0, SF_MAGIC1, SF_VERSION, SF_OP_PUT]);
    req.extend_from_slice(&(PREFS_KEY.len() as u16).to_le_bytes());
    req.extend_from_slice(&(val.len() as u32).to_le_bytes());
    req.extend_from_slice(PREFS_KEY.as_bytes());
    req.extend_from_slice(val);
    let Ok(reply_send_clone) = nexus_abi::cap_clone(reply_send_slot) else {
        return false;
    };
    let hdr = nexus_abi::MsgHeader::new(
        reply_send_clone,
        0,
        0,
        nexus_abi::ipc_hdr::CAP_MOVE,
        req.len() as u32,
    );
    match nexus_abi::ipc_send_v1(send_slot, &hdr, &req, nexus_abi::IPC_SYS_NONBLOCK, 0) {
        Ok(_) => true,
        Err(_) => {
            let _ = nexus_abi::cap_close(reply_send_clone);
            false
        }
    }
}

/// NONBLOCK drain of the shared `@reply` inbox for a statefsd PUT reply.
/// `Some(ok)` = a PUT reply arrived; `None` = nothing (or only foreign
/// frames) waiting. Foreign frames are skipped, bounded per call.
pub(crate) fn poll_put_reply() -> Option<bool> {
    let (_, _, reply_recv_slot) = cached_slots()?;
    for _ in 0..4 {
        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 64]; // a PUT status reply is a handful of bytes
        match nexus_abi::ipc_recv_v1(
            reply_recv_slot,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                let frame = &buf[..n];
                if n >= 5
                    && frame[0] == SF_MAGIC0
                    && frame[1] == SF_MAGIC1
                    && frame[3] == (SF_OP_PUT | 0x80)
                {
                    return Some(frame[4] == SF_STATUS_OK);
                }
                // Foreign inbox traffic: skip and keep draining (bounded).
            }
            Err(_) => return None,
        }
    }
    None
}

/// Route slots resolved ONCE (named routes are persistent slots — never
/// closed, safe to cache): `(statefsd send, @reply send, @reply recv)`.
fn cached_slots() -> Option<(u32, u32, u32)> {
    use core::sync::atomic::{AtomicU32, Ordering};
    static STATEFS_SEND: AtomicU32 = AtomicU32::new(0);
    static REPLY_SEND: AtomicU32 = AtomicU32::new(0);
    static REPLY_RECV: AtomicU32 = AtomicU32::new(0);
    if STATEFS_SEND.load(Ordering::Relaxed) == 0 {
        let (send, _) = route_blocking(b"statefsd")?;
        let (rs, rr) = route_blocking(b"@reply")?;
        STATEFS_SEND.store(send, Ordering::Relaxed);
        REPLY_SEND.store(rs, Ordering::Relaxed);
        REPLY_RECV.store(rr, Ordering::Relaxed);
    }
    Some((
        STATEFS_SEND.load(Ordering::Relaxed),
        REPLY_SEND.load(Ordering::Relaxed),
        REPLY_RECV.load(Ordering::Relaxed),
    ))
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

/// One bounded CAP_MOVE request/reply exchange with statefsd — BOOT ONLY
/// (the prefs GET before the serve loop starts). Kernel-parked with a
/// deadline on both halves (RFC-0083: no yield-spins); the shared `@reply`
/// inbox is filtered on the statefs magic.
fn request_reply(req: &[u8]) -> Option<Vec<u8>> {
    let Some((send_slot, reply_send_slot, reply_recv_slot)) = cached_slots() else {
        let _ = nexus_abi::debug_write(b"settingsd: statefs route FAIL\n");
        return None;
    };
    let Ok(reply_send_clone) = nexus_abi::cap_clone(reply_send_slot) else {
        return None;
    };
    let hdr = nexus_abi::MsgHeader::new(
        reply_send_clone,
        0,
        0,
        nexus_abi::ipc_hdr::CAP_MOVE,
        req.len() as u32,
    );

    let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(500_000_000);
    if nexus_abi::ipc_send_v1(send_slot, &hdr, req, 0, deadline).is_err() {
        let _ = nexus_abi::cap_close(reply_send_clone);
        let _ = nexus_abi::debug_write(b"settingsd: statefs send FAIL\n");
        return None;
    }

    loop {
        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 1024];
        match nexus_abi::ipc_recv_v1(
            reply_recv_slot,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_TRUNCATE,
            deadline,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                // Accept only OUR protocol frames; skip unrelated inbox traffic.
                if n >= 4 && buf[0] == SF_MAGIC0 && buf[1] == SF_MAGIC1 {
                    let mut out = Vec::with_capacity(n);
                    out.extend_from_slice(&buf[..n]);
                    return Some(out);
                }
                let _ = nexus_abi::debug_write(b"settingsd: statefs reply frame mismatch\n");
                if nexus_abi::nsec().unwrap_or(0) >= deadline {
                    return None;
                }
            }
            Err(_) => {
                let _ = nexus_abi::debug_write(b"settingsd: statefs reply timeout\n");
                return None;
            }
        }
    }
}
