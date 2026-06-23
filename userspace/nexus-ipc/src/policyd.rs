// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! One reusable policyd capability-check client (RFC-0066).
//!
//! Enforcement points (bundlemgrd, abilitymgr, …) authorize a caller through a
//! *single* delegated capability check here, instead of each one hand-rolling the
//! CAP_MOVE request/reply dance (the copy-paste that scattered authorization). The
//! wire encode/decode is pure + host-tested; the OS path does the bounded IPC.
//!
//! Policy is the authority: `Allow`/`Deny` come from policyd. `Unreachable` lets a
//! caller fall back to a boot-safe static rule while policyd is still coming up —
//! so capabilities become real without bricking early boot.

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use alloc::vec::Vec;
#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
use std::vec::Vec;

const MAGIC0: u8 = b'P';
const MAGIC1: u8 = b'O';
const VERSION_V2: u8 = 2;
/// Delegated capability check: "is `subject` allowed `cap`?", asked by an
/// enforcement point on the subject's behalf.
const OP_CHECK_CAP_DELEGATED: u8 = 5;
const RESPONSE_BIT: u8 = 0x80;
const STATUS_ALLOW: u8 = 0;
const STATUS_DENY: u8 = 1;

/// Maximum capability-name length accepted on the wire.
pub const MAX_CAP_LEN: usize = 48;

/// The decision returned by a capability check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapDecision {
    /// policyd allowed the capability.
    Allow,
    /// policyd denied the capability.
    Deny,
    /// policyd could not be reached/answered (caller may apply a boot-safe fallback).
    Unreachable,
}

/// Encodes a delegated capability-check request:
/// `[P, O, ver=2, OP_CHECK_CAP_DELEGATED, nonce:u32le, subject:u64le, cap_len:u8, cap...]`.
/// Returns `None` if `cap` is empty or too long.
pub fn encode_check_cap_delegated(nonce: u32, subject_id: u64, cap: &[u8]) -> Option<Vec<u8>> {
    if cap.is_empty() || cap.len() > MAX_CAP_LEN {
        return None;
    }
    let mut frame = Vec::with_capacity(17 + cap.len());
    frame.push(MAGIC0);
    frame.push(MAGIC1);
    frame.push(VERSION_V2);
    frame.push(OP_CHECK_CAP_DELEGATED);
    frame.extend_from_slice(&nonce.to_le_bytes());
    frame.extend_from_slice(&subject_id.to_le_bytes());
    frame.push(cap.len() as u8);
    frame.extend_from_slice(cap);
    Some(frame)
}

/// Decodes a delegated-check response correlated to `expected_nonce`:
/// `[P, O, ver=2, OP|0x80, nonce:u32le, status:u8]`. Returns `None` on a malformed
/// frame or nonce mismatch.
pub fn decode_decision(frame: &[u8], expected_nonce: u32) -> Option<CapDecision> {
    if frame.len() < 10
        || frame[0] != MAGIC0
        || frame[1] != MAGIC1
        || frame[2] != VERSION_V2
        || frame[3] != (OP_CHECK_CAP_DELEGATED | RESPONSE_BIT)
    {
        return None;
    }
    let nonce = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    if nonce != expected_nonce {
        return None;
    }
    Some(match frame[8] {
        STATUS_ALLOW => CapDecision::Allow,
        STATUS_DENY => CapDecision::Deny,
        _ => return None,
    })
}

/// Performs a bounded delegated capability check against policyd over **explicit
/// slots** — the shared CAP_MOVE request/reply, so each enforcement point can use
/// its own init-wired slots (statefsd uses fixed 7/6/5; others route dynamically)
/// without copy-pasting the ~90-line dance. `send_slot` reaches policyd's request
/// endpoint; `reply_send_slot`/`reply_recv_slot` are the caller's @reply inbox.
/// Returns [`CapDecision::Unreachable`] on any IPC failure. OS-only.
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub fn check_cap_on(
    send_slot: u32,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    subject_id: u64,
    cap: &[u8],
) -> CapDecision {
    static NONCE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);
    let nonce = NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

    let frame = match encode_check_cap_delegated(nonce, subject_id, cap) {
        Some(f) => f,
        None => return CapDecision::Unreachable,
    };

    let reply_send_clone = match nexus_abi::cap_clone(reply_send_slot) {
        Ok(c) => c,
        Err(_) => return CapDecision::Unreachable,
    };
    let hdr = nexus_abi::MsgHeader::new(
        reply_send_clone,
        0,
        0,
        nexus_abi::ipc_hdr::CAP_MOVE,
        frame.len() as u32,
    );

    let start = nexus_abi::nsec().unwrap_or(0);
    let deadline = start.saturating_add(500_000_000);

    let mut sent = false;
    let mut spins: u32 = 0;
    loop {
        match nexus_abi::ipc_send_v1(send_slot, &hdr, &frame, nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => {
                sent = true;
                break;
            }
            Err(nexus_abi::IpcError::QueueFull) => {
                if nexus_abi::nsec().unwrap_or(0) >= deadline || spins >= 200_000 {
                    break;
                }
                spins = spins.saturating_add(1);
                let _ = nexus_abi::yield_();
            }
            Err(_) => break,
        }
    }
    let _ = nexus_abi::cap_close(reply_send_clone);
    if !sent {
        return CapDecision::Unreachable;
    }

    loop {
        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 32];
        match nexus_abi::ipc_recv_v1(
            reply_recv_slot,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                if let Some(decision) = decode_decision(&buf[..n], nonce) {
                    return decision;
                }
                if nexus_abi::nsec().unwrap_or(0) >= deadline {
                    return CapDecision::Unreachable;
                }
                let _ = nexus_abi::yield_();
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                if nexus_abi::nsec().unwrap_or(0) >= deadline {
                    return CapDecision::Unreachable;
                }
                let _ = nexus_abi::yield_();
            }
            Err(_) => return CapDecision::Unreachable,
        }
    }
}

/// Delegated capability check that **routes dynamically** to policyd + `@reply`
/// (for services without fixed policyd slots), then runs [`check_cap_on`]. Returns
/// [`CapDecision::Unreachable`] on any routing/IPC failure. OS-only.
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub fn check_cap_delegated(subject_id: u64, cap: &[u8]) -> CapDecision {
    use crate::budget::{route_with_nonce_budgeted, NonceMismatchBudget, RouteRetryOutcome};
    use core::time::Duration;

    let route = |name: &[u8]| match route_with_nonce_budgeted(
        name,
        1,
        2,
        Duration::from_secs(2),
        NonceMismatchBudget::new(64),
    ) {
        RouteRetryOutcome::Success { send_slot, recv_slot } => Some((send_slot, recv_slot)),
        _ => None,
    };

    let (send_slot, _r) = match route(b"policyd") {
        Some(s) => s,
        None => return CapDecision::Unreachable,
    };
    let (reply_send_slot, reply_recv_slot) = match route(b"@reply") {
        Some(s) => s,
        None => return CapDecision::Unreachable,
    };

    check_cap_on(send_slot, reply_send_slot, reply_recv_slot, subject_id, cap)
}

/// Authorizes `subject` for a **typed** [`Capability`] against policyd, and emits a
/// direct, greppable `!cap-deny:` error marker on denial — so a capability failure
/// surfaces immediately in the log instead of having to be hunted down (it also
/// makes the enforcer + capability impossible to typo). The caller decides how to
/// treat [`CapDecision::Unreachable`] (typically a boot-safe fallback).
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub fn authorize(
    subject_id: u64,
    cap: crate::capabilities::Capability,
    enforcer: &str,
) -> CapDecision {
    let decision = check_cap_delegated(subject_id, cap.as_bytes());
    if decision == CapDecision::Deny {
        emit_cap_error(enforcer, cap, subject_id);
    }
    decision
}

/// Emits `!cap-deny: enforcer=<x> cap=<name> subject=0x<id>` — a single, distinct,
/// greppable error line so policy denials never have to be searched for.
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
fn emit_cap_error(enforcer: &str, cap: crate::capabilities::Capability, subject_id: u64) {
    let put = |b: u8| {
        let _ = nexus_abi::debug_putc(b);
    };
    for &b in b"!cap-deny: enforcer=" {
        put(b);
    }
    for &b in enforcer.as_bytes() {
        put(b);
    }
    for &b in b" cap=" {
        put(b);
    }
    for &b in cap.as_bytes() {
        put(b);
    }
    for &b in b" subject=0x" {
        put(b);
    }
    let mut shift: i32 = 60;
    while shift >= 0 {
        let nib = ((subject_id >> shift) & 0xf) as u8;
        put(if nib < 10 { b'0' + nib } else { b'a' + (nib - 10) });
        shift -= 4;
    }
    put(b'\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_rejects_bad_caps() {
        assert!(encode_check_cap_delegated(1, 7, b"").is_none());
        assert!(encode_check_cap_delegated(1, 7, &[0u8; MAX_CAP_LEN + 1]).is_none());
        assert!(encode_check_cap_delegated(1, 7, b"bundle.query").is_some());
    }

    #[test]
    fn encode_decode_roundtrip() {
        let frame = encode_check_cap_delegated(0xAABBCCDD, 0x1122_3344_5566_7788, b"bundle.query")
            .expect("encode");
        // Header is well-formed.
        assert_eq!(&frame[0..4], &[b'P', b'O', 2, OP_CHECK_CAP_DELEGATED]);
        assert_eq!(u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]), 0xAABBCCDD);

        // A matching ALLOW response decodes to Allow.
        let rsp = [b'P', b'O', 2, OP_CHECK_CAP_DELEGATED | RESPONSE_BIT, 0xDD, 0xCC, 0xBB, 0xAA, STATUS_ALLOW, 0];
        assert_eq!(decode_decision(&rsp, 0xAABBCCDD), Some(CapDecision::Allow));
        // DENY → Deny.
        let mut deny = rsp;
        deny[8] = STATUS_DENY;
        assert_eq!(decode_decision(&deny, 0xAABBCCDD), Some(CapDecision::Deny));
    }

    #[test]
    fn decode_rejects_nonce_mismatch_and_malformed() {
        let rsp = [b'P', b'O', 2, OP_CHECK_CAP_DELEGATED | RESPONSE_BIT, 1, 0, 0, 0, STATUS_ALLOW, 0];
        assert_eq!(decode_decision(&rsp, 999), None);
        assert_eq!(decode_decision(&[0u8; 4], 1), None);
        // Wrong opcode (no response bit).
        let bad = [b'P', b'O', 2, OP_CHECK_CAP_DELEGATED, 1, 0, 0, 0, STATUS_ALLOW, 0];
        assert_eq!(decode_decision(&bad, 1), None);
    }
}
