// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host proofs for the persistent evidence journal (TASK-0049C):
//! evidence-class selection (exact-match, lookalikes rejected), slot-value
//! layout roundtrip + decode rejects, ring rotation + budget bound by
//! construction, and the additive QUERY source byte (wire + handler).
//! OWNERS: @runtime @reliability
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file
//! ADR: docs/rfcs/RFC-0087-reliability-failure-model-v1.md (§5)

use logd::evidence::{classify, EvidenceClass};
use logd::journal::{Journal, LogLevel, LogRecord, TimestampNsec};
use logd::lite_handler::{handle_frame, handle_frame_with_journals};
use logd::protocol::{decode_request, QuerySource, Request};
use logd::security::SenderRateLimiter;
use logd::spill::{
    decode_slot_value, SpillEngine, HEAD_KEY, MAX_SLOTS, SLOT_KEY_PREFIX, SLOT_VALUE_CAP,
};

// ---- classification -------------------------------------------------------

#[test]
fn test_classify_crash_in_fields() {
    let fields = b"build_id=abc\ncode=-22\nevent=crash.v1\nname=demo.fault\n";
    assert_eq!(classify(b"execd", b"crash pid=60", fields), Some(EvidenceClass::Crash));
}

#[test]
fn test_classify_exhaust_in_message() {
    // statefsd folds the event into the MESSAGE (fields empty).
    let msg =
        b"event=exhaust.v1 resource=virtio-blk action=ram-backed reason=upgrade window missed";
    assert_eq!(classify(b"statefsd", msg, b""), Some(EvidenceClass::Exhaust));
}

#[test]
fn test_classify_audit_scope_suffix() {
    assert_eq!(
        classify(b"statefsd.audit", b"denied path=/state/x", b""),
        Some(EvidenceClass::Audit)
    );
    // Deny audits persist; policyd's exact wire shape.
    assert_eq!(
        classify(
            b"policyd.audit",
            b"audit v1 op=check_cap decision=deny subject=0x11 reason=policy",
            b""
        ),
        Some(EvidenceClass::Audit)
    );
}

#[test]
fn test_reject_allow_audit_stays_ram_only() {
    // The spill loop guard: every spill PUT yields a policyd ALLOW audit
    // back at logd — persisting it would spill again, forever.
    assert_eq!(
        classify(
            b"policyd.audit",
            b"audit v1 op=check_cap decision=allow subject=0x11 reason=policy",
            b""
        ),
        None
    );
    // Lookalike must NOT be treated as allow (still evidence).
    assert_eq!(
        classify(b"policyd.audit", b"decision=allowx op=check_cap", b""),
        Some(EvidenceClass::Audit)
    );
}

#[test]
fn test_reject_classify_lookalikes() {
    // Exactness: prefix/infix lookalikes must never classify.
    assert_eq!(classify(b"svc", b"", b"event=crash.v1x\n"), None);
    assert_eq!(classify(b"svc", b"", b"revent=crash.v1\n"), None);
    assert_eq!(classify(b"svc", b"prefix event=exhaust.v11", b""), None);
    // First event= token wins (no tag stuffing past an unknown class).
    assert_eq!(classify(b"svc", b"", b"event=foo\nevent=crash.v1\n"), None);
    // Bulk stream stays bulk; audit is a suffix rule, not a substring rule.
    assert_eq!(classify(b"windowd", b"present ok", b"seq=1\n"), None);
    assert_eq!(classify(b"audit.tail", b"x", b""), None);
}

// ---- slot value layout -----------------------------------------------------

fn sample_record(journal: &mut Journal, scope: &[u8], msg: &[u8], fields: &[u8]) -> LogRecord {
    journal.append(0x77, TimestampNsec(42), LogLevel::Warn, scope, msg, fields).expect("append");
    journal.iter_since(TimestampNsec(0)).last().expect("record").clone()
}

#[test]
fn test_slot_value_roundtrip() {
    let mut j = Journal::new(4, 4096);
    let rec = sample_record(&mut j, b"execd", b"crash pid=60", b"event=crash.v1\ncode=-22\n");
    let mut engine = SpillEngine::new(7);
    let plan = engine.plan(&rec);
    assert_eq!(plan.slot_key.as_str(), format!("{}{:02}", SLOT_KEY_PREFIX, 7 % MAX_SLOTS));
    assert_eq!(u64::from_le_bytes(plan.head_value), 8);
    let dec = decode_slot_value(&plan.value).expect("decode");
    assert_eq!(dec.seq, 7);
    assert_eq!(dec.level, LogLevel::Warn);
    assert_eq!(dec.service_id, 0x77);
    assert_eq!(dec.timestamp_nsec, 42);
    assert_eq!(dec.scope, b"execd");
    assert_eq!(dec.message, b"crash pid=60");
    assert_eq!(dec.fields, b"event=crash.v1\ncode=-22\n");
}

#[test]
fn test_reject_slot_value_decode() {
    let mut j = Journal::new(4, 4096);
    let rec = sample_record(&mut j, b"s", b"m", b"f");
    let mut engine = SpillEngine::new(0);
    let good = engine.plan(&rec).value;
    // Too short / bad version / bad level / length mismatch / oversize.
    assert!(decode_slot_value(&good[..28]).is_none());
    let mut bad = good.clone();
    bad[0] = 9;
    assert!(decode_slot_value(&bad).is_none());
    let mut bad = good.clone();
    bad[1] = 5;
    assert!(decode_slot_value(&bad).is_none());
    let mut bad = good.clone();
    bad[26] = bad[26].wrapping_add(1);
    assert!(decode_slot_value(&bad).is_none());
    let mut big = good;
    big.resize(SLOT_VALUE_CAP + 1, 0);
    assert!(decode_slot_value(&big).is_none());
}

#[test]
fn test_budget_bound_by_construction() {
    // Store-cap maximal record (scope 32, msg 128, fields 128 after the
    // journal's InlineBytes truncation) must stay under the slot cap.
    let mut j = Journal::new(4, 4096);
    let rec = sample_record(&mut j, &[b'a'; 64], &[b'b'; 256], &[b'c'; 512]);
    let mut engine = SpillEngine::new(0);
    let plan = engine.plan(&rec);
    assert!(plan.value.len() <= SLOT_VALUE_CAP, "value {} > cap", plan.value.len());
}

// ---- ring / sequence -------------------------------------------------------

#[test]
fn test_ring_rotation_and_head() {
    let mut j = Journal::new(4, 4096);
    let rec = sample_record(&mut j, b"s", b"m", b"event=exhaust.v1");
    let mut engine = SpillEngine::new(0);
    let mut last_head = 0u64;
    for seq in 0..(MAX_SLOTS + 3) {
        let plan = engine.plan(&rec);
        assert_eq!(plan.slot_key.as_str(), format!("{}{:02}", SLOT_KEY_PREFIX, seq % MAX_SLOTS));
        let head = u64::from_le_bytes(plan.head_value);
        assert_eq!(head, seq + 1);
        assert!(head > last_head);
        last_head = head;
    }
    // Wrap proof: seq MAX_SLOTS reused slot_00, MAX_SLOTS+1 slot_01.
    assert_eq!(SpillEngine::live_slots(3), 3);
    assert_eq!(SpillEngine::live_slots(MAX_SLOTS + 3), MAX_SLOTS);
    assert_eq!(HEAD_KEY, "/state/logd/evidence/head");
}

#[test]
fn test_rollback_on_failed_commit() {
    let mut j = Journal::new(4, 4096);
    let rec = sample_record(&mut j, b"s", b"m", b"event=crash.v1");
    let mut engine = SpillEngine::new(5);
    let _ = engine.plan(&rec);
    engine.rollback_one();
    assert_eq!(engine.next_seq(), 5);
    let plan = engine.plan(&rec);
    assert_eq!(u64::from_le_bytes(plan.head_value), 6);
}

// ---- wire: query source byte ------------------------------------------------

fn query_frame_v1(extra: &[u8]) -> Vec<u8> {
    let mut f = vec![b'L', b'O', 1, 2];
    f.extend_from_slice(&0u64.to_le_bytes());
    f.extend_from_slice(&16u16.to_le_bytes());
    f.extend_from_slice(extra);
    f
}

#[test]
fn test_query_source_byte_decodes() {
    match decode_request(&query_frame_v1(&[])).expect("base") {
        Request::Query(q) => assert_eq!(q.source, QuerySource::Ram),
        other => panic!("unexpected {other:?}"),
    }
    match decode_request(&query_frame_v1(&[1])).expect("persisted") {
        Request::Query(q) => assert_eq!(q.source, QuerySource::Persisted),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn test_reject_query_source_byte() {
    assert!(decode_request(&query_frame_v1(&[2])).is_err());
    assert!(decode_request(&query_frame_v1(&[0, 0])).is_err());
}

// ---- handler: persisted mirror ----------------------------------------------

#[test]
fn test_query_persisted_scope_roundtrip() {
    let mut ram = Journal::new(8, 4096);
    let mut persisted = Journal::new(8, 4096);
    persisted
        .append(0x11, TimestampNsec(1), LogLevel::Warn, b"execd", b"old boot", b"event=crash.v1\n")
        .expect("append");
    let mut limiter = SenderRateLimiter::new();

    let rsp = handle_frame_with_journals(
        &mut ram,
        &persisted,
        0x22,
        TimestampNsec(2),
        &query_frame_v1(&[1]),
        &mut limiter,
    );
    // [L,O,ver,0x82,status, total, dropped, count:u16] — count == 1, and the
    // record body carries the old-boot message.
    assert_eq!(rsp[4], 0);
    let count = u16::from_le_bytes([rsp[21], rsp[22]]);
    assert_eq!(count, 1);
    assert!(rsp.windows(8).any(|w| w == b"old boot"));

    // Same query against RAM finds nothing.
    let rsp = handle_frame_with_journals(
        &mut ram,
        &persisted,
        0x22,
        TimestampNsec(2),
        &query_frame_v1(&[0]),
        &mut limiter,
    );
    assert_eq!(u16::from_le_bytes([rsp[21], rsp[22]]), 0);
}

#[test]
fn test_legacy_entry_point_answers_persisted_empty() {
    let mut ram = Journal::new(8, 4096);
    let rsp = handle_frame(&mut ram, 0x22, TimestampNsec(2), &query_frame_v1(&[1]));
    assert_eq!(rsp[4], 0);
    assert_eq!(u16::from_le_bytes([rsp[21], rsp[22]]), 0);
}
