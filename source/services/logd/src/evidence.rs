// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: Evidence-class detection for the persistent evidence journal
//! (TASK-0049C, RFC-0087 §5). logd persists ONLY evidence-class records —
//! crash reports (`event=crash.v1`), exhaustion announcements
//! (`event=exhaust.v1`) and audit records — while the bulk debug stream
//! stays RAM-only. Classification is pure byte inspection over the
//! `key=value\n` convention; emitters differ in WHERE they carry the event
//! tag (execd puts `event=crash.v1` in `fields`, statefsd folds
//! `event=exhaust.v1 ...` into the message), so both are scanned. Audit
//! records are recognized by their scope suffix (`.audit`) — the scope is
//! server-side vocabulary, not a sender claim (identity stays the
//! kernel-attributed `sender_service_id`).
//!
//! OWNERS: @runtime @reliability
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `tests/evidence_spill.rs` (classification matrix incl.
//!   reject cases: prefix/infix lookalikes never classify).
//! ADR: docs/rfcs/RFC-0087-reliability-failure-model-v1.md (§5)

/// Evidence class of an appended record (RFC-0087 §5 vocabulary).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceClass {
    /// `event=crash.v1` — crash report records (RFC-0011 envelope).
    Crash,
    /// `event=exhaust.v1` — exhaustion announcements (RFC-0087 §1).
    Exhaust,
    /// `<scope>.audit` records (e.g. policyd/statefsd audit trails).
    Audit,
}

/// Classifies one append; `None` = bulk stream, stays RAM-only.
pub fn classify(scope: &[u8], message: &[u8], fields: &[u8]) -> Option<EvidenceClass> {
    if let Some(class) = classify_event_tag(fields).or_else(|| classify_event_tag(message)) {
        return Some(class);
    }
    if scope.ends_with(b".audit") && !is_allow_decision(message, fields) {
        return Some(EvidenceClass::Audit);
    }
    None
}

/// `decision=allow` audits stay RAM-only. They are operational noise — and
/// crucially the spill's own echo: every spill PUT triggers a policyd check
/// whose ALLOW audit lands back in logd; persisting it would re-trigger a
/// spill, a self-sustaining loop. Deny audits (and audits without a
/// decision token, e.g. statefsd's) are the forensically valuable class and
/// cannot loop: a deny in the spill chain stops the spill itself.
fn is_allow_decision(message: &[u8], fields: &[u8]) -> bool {
    has_token(message, b"decision=allow") || has_token(fields, b"decision=allow")
}

fn has_token(payload: &[u8], token: &[u8]) -> bool {
    payload.split(|&b| b == b'\n' || b == b' ').any(|line| line == token)
}

/// Scans `key=value` tokens (newline- or space-separated) for an exact
/// `event=<class>` match. Exactness matters: `event=crash.v1x` or
/// `revent=crash.v1` must never classify (evidence storage is a bounded
/// resource; lookalikes stay bulk).
fn classify_event_tag(payload: &[u8]) -> Option<EvidenceClass> {
    for line in payload.split(|&b| b == b'\n' || b == b' ') {
        if let Some(value) = line.strip_prefix(b"event=") {
            return match value {
                b"crash.v1" => Some(EvidenceClass::Crash),
                b"exhaust.v1" => Some(EvidenceClass::Exhaust),
                _ => None,
            };
        }
    }
    None
}
