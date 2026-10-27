// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: policyd's audit record (`audit v1 op=… decision=… subject=0x…
//! reason=…`) on logd scope `policyd.audit` — the ONE deny/allow taxonomy
//! (RFC-0068; RFC-0091 adds `abi-rule:<class>` refusals and the `abi-mode`
//! transition). Bounded per boot (`AUDIT_EMIT_LIMIT`), best-effort, never
//! blocking a decision; a deferred append is printed live so a lost audit
//! never hides. Split out of `os_lite.rs` for the structure ratchet.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU `policyd: audit emit ok` + selftest logd audit queries
//! RFC: docs/rfcs/RFC-0068-structured-event-observability-subject-grouped-journal-renderer.md

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::os_lite::{
    emit_line, OP_CHECK, OP_CHECK_CAP, OP_CHECK_CAP_DELEGATED, OP_EXEC, OP_ROUTE,
};

const AUDIT_SCOPE: &str = "policyd.audit";
const AUDIT_EMIT_LIMIT: usize = 128;
static AUDIT_EMIT_COUNT: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug)]
pub(crate) enum AuditReason {
    Policy,
    /// RFC-0091 argument filter refused (class byte from the eval frame).
    AbiRule(u8),
    /// RFC-0091 §6 mode transition (the audited runtime change).
    AbiMode,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum AuditDecision {
    Allow,
    Deny,
}

pub(crate) fn emit_audit(
    op: u8,
    decision: AuditDecision,
    subject_id: u64,
    target_id: Option<u64>,
    reason: AuditReason,
) {
    if AUDIT_EMIT_COUNT.fetch_add(1, Ordering::Relaxed) >= AUDIT_EMIT_LIMIT {
        return;
    }
    let mut buf = [0u8; 256];
    let mut len = 0usize;
    let _ = push_bytes(&mut buf, &mut len, b"audit v1 op=");
    let _ = push_bytes(&mut buf, &mut len, audit_op_name(op));
    let _ = push_bytes(&mut buf, &mut len, b" decision=");
    let _ = push_bytes(&mut buf, &mut len, audit_decision_name(decision));
    let _ = push_bytes(&mut buf, &mut len, b" subject=0x");
    write_hex_u64(&mut buf, &mut len, subject_id);
    if let Some(target) = target_id {
        let _ = push_bytes(&mut buf, &mut len, b" target=0x");
        write_hex_u64(&mut buf, &mut len, target);
    }
    let _ = push_bytes(&mut buf, &mut len, b" reason=");
    let _ = push_bytes(&mut buf, &mut len, audit_reason_name(reason));
    let ok = append_logd_deterministic(AUDIT_SCOPE.as_bytes(), &buf[..len]);
    if ok {
        // RFC-0068 P4: the audit RECORD is now in logd's subject journal (rendered as a `policyd`
        // verdict at quiet), so this success echo is redundant — fold it away in interactive boots.
        // Proof boots still print it raw (verify-uart); `=policyd` recalls it.
        if !nexus_abi::boot_should_fold_verdicts() {
            emit_line("policyd: audit emit ok");
        }
    } else {
        // A DEFERRED append means the audit was NOT recorded (logd queue/readiness) — a real failure,
        // never hidden: always print live so the lost-audit signal stays visible.
        emit_line("policyd: audit emit deferred");
    }
}

fn audit_op_name(op: u8) -> &'static [u8] {
    match op {
        OP_CHECK => b"check",
        OP_CHECK_CAP => b"check_cap",
        OP_CHECK_CAP_DELEGATED => b"check_cap_delegated",
        OP_ROUTE => b"route",
        OP_EXEC => b"exec",
        nexus_abi::policyd::OP_ABI_PROFILE_GET => b"abi_profile",
        nexus_abi::policyd::OP_SET_ABI_MODE => b"abi_mode",
        nexus_abi::policyd::OP_ABI_EVAL => b"abi_eval",
        nexus_abi::policyd::OP_ABI_LEARN_STATS => b"abi_learn_stats",
        _ => b"unknown",
    }
}

fn audit_decision_name(decision: AuditDecision) -> &'static [u8] {
    match decision {
        AuditDecision::Allow => b"allow",
        AuditDecision::Deny => b"deny",
    }
}

/// The ONE deny taxonomy (`nexus_ipc::audit::DenyReason`): refusals of the
/// network classes are reported with their user-facing reasons.
fn audit_reason_name(reason: AuditReason) -> &'static [u8] {
    use nexus_ipc::audit::{AbiClass, DenyReason};
    match reason {
        AuditReason::Policy => DenyReason::Policy.as_str().as_bytes(),
        AuditReason::AbiRule(class) => match AbiClass::from_wire(class) {
            Some(c) => DenyReason::for_abi_class(c).as_str().as_bytes(),
            None => b"abi-rule:unknown",
        },
        AuditReason::AbiMode => DenyReason::AbiMode.as_str().as_bytes(),
    }
}

/// `egress_denies_total{subject}` / `ingress_denies_total{subject}` (TASK-0043
/// P4): counted where the decision is made, flushed to metricsd at most once
/// per second per counter (bounded, best-effort — never in the decision
/// path's error handling). Owned by the service loop and threaded through
/// `handle_frame` (policyd forbids `unsafe`, so no `static` cell).
pub(crate) struct DenyCounters {
    egress: nexus_metrics::deny_counter::DenyCounter,
    ingress: nexus_metrics::deny_counter::DenyCounter,
}

impl DenyCounters {
    pub(crate) const fn new() -> Self {
        Self {
            egress: nexus_metrics::deny_counter::DenyCounter::new("egress_denies_total"),
            ingress: nexus_metrics::deny_counter::DenyCounter::new("ingress_denies_total"),
        }
    }

    /// Counts an `OP_ABI_EVAL` refusal of a network class for `subject`.
    pub(crate) fn note_abi_deny(&mut self, class: u8, subject: u64) {
        let now = nexus_abi::nsec().unwrap_or(0);
        match class {
            3 => self.egress.note(subject, now),
            2 => self.ingress.note(subject, now),
            _ => {}
        }
    }
}

// The logd append transport the audit record (and the ABI learn record) ride on — moved here
// from `os_lite.rs` with the structure ratchet.
pub(crate) fn push_bytes(buf: &mut [u8], len: &mut usize, bytes: &[u8]) -> bool {
    let available = buf.len().saturating_sub(*len);
    if bytes.len() > available {
        return false;
    }
    buf[*len..*len + bytes.len()].copy_from_slice(bytes);
    *len += bytes.len();
    true
}

pub(crate) fn write_hex_u64(buf: &mut [u8], len: &mut usize, value: u64) {
    if buf.len().saturating_sub(*len) < 16 {
        return;
    }
    for shift in (0..16).rev() {
        let nibble = ((value >> (shift * 4)) & 0xF) as u8;
        let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
        buf[*len] = ch;
        *len += 1;
    }
}

pub(crate) fn append_logd_deterministic(scope: &[u8], msg: &[u8]) -> bool {
    // policyd's logd leg as declared (TASK-0324 P4f-2). No reply inbox: the append moves no
    // reply cap (TASK-0054C P2-c).
    const LOGD_SEND_SLOT: u32 = nexus_service_topology::slots::policyd::LOGD.send;

    const MAGIC0: u8 = b'L';
    const MAGIC1: u8 = b'O';
    const VERSION: u8 = 2;
    const OP_APPEND: u8 = 1;
    const LEVEL_INFO: u8 = 2;
    const STATUS_OK: u8 = 0;
    static NONCE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);

    if scope.len() > 64 || msg.len() > 256 {
        return false;
    }

    let mut frame = [0u8; 512];
    let mut len = 0usize;
    let nonce = NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    if frame.len() < 4 + 8 + 1 + 1 + 2 + 2 + scope.len().saturating_add(msg.len()) {
        return false;
    }
    frame[len..len + 4].copy_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_APPEND]);
    len += 4;
    frame[len..len + 8].copy_from_slice(&nonce.to_le_bytes());
    len += 8;
    frame[len] = LEVEL_INFO;
    len += 1;
    frame[len] = scope.len() as u8;
    len += 1;
    frame[len..len + 2].copy_from_slice(&(msg.len() as u16).to_le_bytes());
    len += 2;
    frame[len..len + 2].copy_from_slice(&0u16.to_le_bytes()); // fields_len
    len += 2;
    frame[len..len + scope.len()].copy_from_slice(scope);
    len += scope.len();
    frame[len..len + msg.len()].copy_from_slice(msg);
    len += msg.len();

    // TASK-0054C P2-c: no reply cap moves with an audit record, so logd owes no ack (no TASK-0049C
    // wait triangle). The wait for QUEUE SPACE stays: backpressure. A deferred append is counted.
    let _ = nonce;
    let _ = STATUS_OK;
    nexus_ipc::exchange::send_request(LOGD_SEND_SLOT, &frame[..len]).is_ok()
}
