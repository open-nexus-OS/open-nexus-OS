// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: One reusable policyd capability-check client — shared by every
//! enforcement point (bundlemgrd, abilitymgr, …) (RFC-0066).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 3 tests
//!
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
const STATUS_UNSUPPORTED: u8 = 3;

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
    match decode_status_v2(frame, OP_CHECK_CAP_DELEGATED, expected_nonce)? {
        STATUS_ALLOW => Some(CapDecision::Allow),
        STATUS_DENY => Some(CapDecision::Deny),
        _ => None,
    }
}

/// Decodes a generic policyd v2 reply `[P,O,2,op|0x80, nonce:u32le, status:u8, _]`
/// for `op`, binding it to `expected_nonce`; returns the status byte.
pub fn decode_status_v2(frame: &[u8], op: u8, expected_nonce: u32) -> Option<u8> {
    if frame.len() < 10
        || frame[0] != MAGIC0
        || frame[1] != MAGIC1
        || frame[2] != VERSION_V2
        || frame[3] != (op | RESPONSE_BIT)
    {
        return None;
    }
    let nonce = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    if nonce != expected_nonce {
        return None;
    }
    Some(frame[8])
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
    let nonce = next_nonce();
    let frame = match encode_check_cap_delegated(nonce, subject_id, cap) {
        Some(f) => f,
        None => return CapDecision::Unreachable,
    };
    match exchange_status_on(
        send_slot,
        reply_send_slot,
        reply_recv_slot,
        &frame,
        OP_CHECK_CAP_DELEGATED,
        nonce,
    ) {
        Some(STATUS_ALLOW) => CapDecision::Allow,
        Some(STATUS_DENY) => CapDecision::Deny,
        _ => CapDecision::Unreachable,
    }
}

/// RFC-0091 §7: asks policyd to evaluate one governed ABI argument tuple for
/// `subject_id` over explicit slots (`OP_ABI_EVAL`). The seam names the subject
/// it serves (it holds `policy.delegate`). `class` / `addr_class` / `port` /
/// `addr_be` / `payload_len` / `deadline_ms` / `path` are the wire fields of
/// `nexus_abi::policyd::encode_abi_eval_v2`. Returns the policyd status byte
/// (`STATUS_ALLOW`, `STATUS_DENY`, `STATUS_UNSUPPORTED` = subject not governed,
/// `STATUS_MALFORMED`) or `None` when policyd could not be reached — the caller
/// decides (an enforcement seam fails closed).
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
#[allow(clippy::too_many_arguments)]
pub fn abi_eval_on(
    send_slot: u32,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    subject_id: u64,
    class: u8,
    addr_class: u8,
    port: u16,
    addr_be: u32,
    payload_len: u32,
    deadline_ms: u32,
    path: &[u8],
) -> Option<u8> {
    let nonce = next_nonce();
    let mut frame = [0u8; 48 + nexus_abi::policyd::MAX_ABI_EVAL_PATH_BYTES];
    let n = nexus_abi::policyd::encode_abi_eval_v2(
        nonce,
        subject_id,
        class,
        addr_class,
        port,
        addr_be,
        payload_len,
        deadline_ms,
        path,
        &mut frame,
    )?;
    exchange_status_on(
        send_slot,
        reply_send_slot,
        reply_recv_slot,
        &frame[..n],
        nexus_abi::policyd::OP_ABI_EVAL,
        nonce,
    )
}

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
fn next_nonce() -> u32 {
    static NONCE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);
    NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed)
}

/// The bounded CAP_MOVE request/reply dance shared by every policyd v2 op over
/// explicit slots: send `frame` with the caller's `@reply` send cap moved along,
/// then poll the reply inbox for the `op|0x80` frame carrying `nonce` (≤ 500 ms).
/// `None` = not sent / no matching reply in time.
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
fn exchange_status_on(
    send_slot: u32,
    reply_send_slot: u32,
    reply_recv_slot: u32,
    frame: &[u8],
    op: u8,
    nonce: u32,
) -> Option<u8> {
    let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).ok()?;
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
        match nexus_abi::ipc_send_v1(send_slot, &hdr, frame, nexus_abi::IPC_SYS_NONBLOCK, 0) {
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
        return None;
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
                if let Some(status) = decode_status_v2(&buf[..n], op, nonce) {
                    return Some(status);
                }
                if nexus_abi::nsec().unwrap_or(0) >= deadline {
                    return None;
                }
                let _ = nexus_abi::yield_();
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                if nexus_abi::nsec().unwrap_or(0) >= deadline {
                    return None;
                }
                let _ = nexus_abi::yield_();
            }
            Err(_) => return None,
        }
    }
}

/// RFC-0091 §7 seam decision over a policyd `OP_ABI_EVAL` outcome: the
/// request must be kernel-attributed (`sender_service_id != 0` — an
/// unattributed request is never admitted, `test_reject_unattributed_connect`)
/// and policyd must have answered `STATUS_ALLOW`, or `STATUS_UNSUPPORTED`
/// (the subject has no authored profile — not governed yet, capability-only
/// per the governed = authored rule). `None` (unreachable) and every other
/// status refuse: a seam fails closed.
#[must_use]
pub fn seam_admits(sender_service_id: u64, eval_status: Option<u8>) -> bool {
    if sender_service_id == 0 {
        return false;
    }
    matches!(eval_status, Some(STATUS_ALLOW) | Some(STATUS_UNSUPPORTED))
}

/// Policyd request slot + the caller's `@reply` inbox, resolved once through
/// init routing (for services without fixed policyd slots).
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
#[derive(Clone, Copy, Debug)]
pub struct PolicySlots {
    /// policyd's request endpoint.
    pub send: u32,
    /// The caller's `@reply` send cap (moved along with each request).
    pub reply_send: u32,
    /// The caller's `@reply` receive slot.
    pub reply_recv: u32,
}

/// Routes `policyd` + `@reply` (bounded) and returns the slots for
/// [`check_cap_on`] / [`abi_eval_on`]; `None` = routing failed.
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub fn resolve_policy_slots() -> Option<PolicySlots> {
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
    let (send, _) = route(b"policyd")?;
    let (reply_send, reply_recv) = route(b"@reply")?;
    Some(PolicySlots { send, reply_send, reply_recv })
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
    fn test_reject_unattributed_connect() {
        // No kernel identity ⇒ never admitted, whatever policyd said.
        assert!(!seam_admits(0, Some(STATUS_ALLOW)));
        assert!(!seam_admits(0, Some(STATUS_UNSUPPORTED)));
        // Attributed: allow and "not governed" admit; deny/unreachable/other refuse.
        assert!(seam_admits(7, Some(STATUS_ALLOW)));
        assert!(seam_admits(7, Some(STATUS_UNSUPPORTED)));
        assert!(!seam_admits(7, Some(STATUS_DENY)));
        assert!(!seam_admits(7, None));
        assert!(!seam_admits(7, Some(2)));
        assert!(!seam_admits(7, Some(4)));
    }

    #[test]
    fn decode_status_v2_binds_op_and_nonce() {
        let rsp = [MAGIC0, MAGIC1, VERSION_V2, 8 | RESPONSE_BIT, 7, 0, 0, 0, 1, 0];
        assert_eq!(decode_status_v2(&rsp, 8, 7), Some(1));
        assert_eq!(decode_status_v2(&rsp, 8, 8), None);
        assert_eq!(decode_status_v2(&rsp, 7, 7), None);
        assert_eq!(decode_status_v2(&rsp[..9], 8, 7), None);
        assert_eq!(decode_decision(&rsp, 7), None); // wrong op for the cap decoder
    }

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
        let rsp = [
            b'P',
            b'O',
            2,
            OP_CHECK_CAP_DELEGATED | RESPONSE_BIT,
            0xDD,
            0xCC,
            0xBB,
            0xAA,
            STATUS_ALLOW,
            0,
        ];
        assert_eq!(decode_decision(&rsp, 0xAABBCCDD), Some(CapDecision::Allow));
        // DENY → Deny.
        let mut deny = rsp;
        deny[8] = STATUS_DENY;
        assert_eq!(decode_decision(&deny, 0xAABBCCDD), Some(CapDecision::Deny));
    }

    #[test]
    fn decode_rejects_nonce_mismatch_and_malformed() {
        let rsp =
            [b'P', b'O', 2, OP_CHECK_CAP_DELEGATED | RESPONSE_BIT, 1, 0, 0, 0, STATUS_ALLOW, 0];
        assert_eq!(decode_decision(&rsp, 999), None);
        assert_eq!(decode_decision(&[0u8; 4], 1), None);
        // Wrong opcode (no response bit).
        let bad = [b'P', b'O', 2, OP_CHECK_CAP_DELEGATED, 1, 0, 0, 0, STATUS_ALLOW, 0];
        assert_eq!(decode_decision(&bad, 1), None);
    }
}
