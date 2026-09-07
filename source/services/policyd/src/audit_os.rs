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
    append_logd_deterministic, emit_line, push_bytes, write_hex_u64, OP_CHECK, OP_CHECK_CAP,
    OP_CHECK_CAP_DELEGATED, OP_EXEC, OP_ROUTE,
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
        _ => b"unknown",
    }
}

fn audit_decision_name(decision: AuditDecision) -> &'static [u8] {
    match decision {
        AuditDecision::Allow => b"allow",
        AuditDecision::Deny => b"deny",
    }
}

fn audit_reason_name(reason: AuditReason) -> &'static [u8] {
    match reason {
        AuditReason::Policy => b"policy",
        AuditReason::AbiRule(1) => b"abi-rule:statefs",
        AuditReason::AbiRule(2) => b"abi-rule:net.bind",
        AuditReason::AbiRule(3) => b"abi-rule:net.connect",
        AuditReason::AbiRule(_) => b"abi-rule:unknown",
        AuditReason::AbiMode => b"abi-mode",
    }
}
