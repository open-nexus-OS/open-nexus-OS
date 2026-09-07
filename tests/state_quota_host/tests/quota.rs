// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0072 amendment / TASK-0133 model proofs over the real
//! `JournalEngine` (in-memory block device): deny over the hard limit with
//! the stable `EDQUOTA` code before anything reaches the journal, usage
//! deterministic across replay (reopen), soft warning once per window with
//! re-arm, delete frees, overwrite counts the delta, unmetered prefixes stay
//! untouched, and the shipped `policies/base.toml` quota compiles through
//! the shared grammar.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! TEST_COVERAGE: TASK-0043 P1 required host proofs

use statefs::protocol::{status_from_error, STATUS_QUOTA_EXCEEDED};
use statefs::quota::{check_put, rule_for, PutVerdict, QuotaRule, WarnLatch};
use statefs::{JournalEngine, StatefsError};
use storage::MemBlockDevice;

const RULE: QuotaRule = QuotaRule {
    subject: 0x1234,
    prefixes: &["/state/app/demo/", "/state/demo/"],
    soft_bytes: 200,
    hard_bytes: 300,
};
const RULES: &[QuotaRule] = &[RULE];

fn engine() -> JournalEngine<MemBlockDevice> {
    JournalEngine::open(MemBlockDevice::new(512, 256)).expect("engine")
}

/// The statefsd seam in miniature: gate, then journal; returns the wire status.
fn seam_put(
    engine: &mut JournalEngine<MemBlockDevice>,
    latch: &mut WarnLatch,
    key: &str,
    value: &[u8],
) -> (u8, bool) {
    let mut warned = false;
    if let Some(idx) = rule_for(RULES, key) {
        let rule = &RULES[idx];
        let used = engine.used_under(rule.prefixes);
        match check_put(rule, used, key.len(), engine.stored_len(key), value.len()) {
            PutVerdict::Deny { .. } => {
                return (status_from_error(StatefsError::QuotaExceeded), false)
            }
            PutVerdict::Warn { used: next } => warned = latch.observe(idx, next, rule.soft_bytes),
            PutVerdict::Ok => {
                let _ = latch.observe(idx, used, rule.soft_bytes);
            }
        }
    }
    engine.put(key, value).expect("journal put");
    (0, warned)
}

#[test]
fn test_reject_write_over_hard_quota() {
    let mut e = engine();
    let mut latch = WarnLatch::new();
    let k = "/state/app/demo/k"; // 17
                                 // 17 + 100 = 117 per entry: two fit (234 > soft 200, < hard 300), the third would be 351.
    assert_eq!(seam_put(&mut e, &mut latch, "/state/app/demo/1", &[1u8; 100]), (0, false));
    assert_eq!(seam_put(&mut e, &mut latch, "/state/app/demo/2", &[2u8; 100]), (0, true));
    let (status, _) = seam_put(&mut e, &mut latch, k, &[3u8; 100]);
    assert_eq!(status, STATUS_QUOTA_EXCEEDED);
    assert_eq!(status, 12, "EDQUOTA wire code is stable");
    // Nothing reached the journal for the refused key.
    assert_eq!(e.get(k).unwrap_err(), StatefsError::NotFound);
    assert_eq!(e.used_under(RULE.prefixes), 234);
    // Exactly at the hard limit is allowed: 234 + (17 + 49) = 300.
    assert_eq!(seam_put(&mut e, &mut latch, k, &[4u8; 49]).0, 0);
    assert_eq!(e.used_under(RULE.prefixes), 300);
    assert_eq!(seam_put(&mut e, &mut latch, "/state/demo/x", &[5u8; 1]).0, STATUS_QUOTA_EXCEEDED);
}

#[test]
fn accounting_is_deterministic_across_replay() {
    let mut e = engine();
    let mut latch = WarnLatch::new();
    for (i, size) in [40usize, 60, 20].iter().enumerate() {
        let key = format!("/state/app/demo/{i}");
        assert_eq!(seam_put(&mut e, &mut latch, &key, &vec![7u8; *size]).0, 0);
    }
    e.delete("/state/app/demo/1").unwrap();
    let before = e.used_under(RULE.prefixes);
    assert_eq!(before, (17 + 40) + (17 + 20));
    // Reopen = replay from the device: the same number, from the journal alone.
    e.reopen().unwrap();
    assert_eq!(e.used_under(RULE.prefixes), before);
    // A fresh engine over the same bytes agrees too.
    let device = e.into_device();
    let fresh = JournalEngine::open(device).unwrap();
    assert_eq!(fresh.used_under(RULE.prefixes), before);
}

#[test]
fn soft_warn_once_per_window_and_rearm() {
    let mut e = engine();
    let mut latch = WarnLatch::new();
    assert_eq!(seam_put(&mut e, &mut latch, "/state/app/demo/a", &[0u8; 100]), (0, false));
    // Crossing soft warns exactly once…
    assert_eq!(seam_put(&mut e, &mut latch, "/state/app/demo/b", &[0u8; 100]), (0, true));
    assert_eq!(seam_put(&mut e, &mut latch, "/state/app/demo/b", &[0u8; 110]), (0, false));
    // …drops below soft re-arm it…
    e.delete("/state/app/demo/b").unwrap();
    let rule_idx = rule_for(RULES, "/state/app/demo/b").unwrap();
    assert!(!latch.observe(rule_idx, e.used_under(RULE.prefixes), RULE.soft_bytes));
    // …and the next crossing warns again.
    assert_eq!(seam_put(&mut e, &mut latch, "/state/app/demo/c", &[0u8; 100]), (0, true));
}

#[test]
fn delete_frees_and_overwrite_counts_delta() {
    let mut e = engine();
    let mut latch = WarnLatch::new();
    assert_eq!(seam_put(&mut e, &mut latch, "/state/app/demo/a", &[0u8; 120]).0, 0);
    assert_eq!(seam_put(&mut e, &mut latch, "/state/app/demo/b", &[0u8; 120]).0, 0);
    assert_eq!(e.used_under(RULE.prefixes), 274);
    // Overwrite b with a SMALLER value: delta is negative, never refused.
    assert_eq!(seam_put(&mut e, &mut latch, "/state/app/demo/b", &[0u8; 10]).0, 0);
    assert_eq!(e.used_under(RULE.prefixes), 164);
    // Overwrite b with a value that would exceed hard: refused, old value intact.
    assert_eq!(
        seam_put(&mut e, &mut latch, "/state/app/demo/b", &[0u8; 200]).0,
        STATUS_QUOTA_EXCEEDED
    );
    assert_eq!(e.get("/state/app/demo/b").unwrap().len(), 10);
    // Delete is never quota-denied and frees the bytes.
    e.delete("/state/app/demo/a").unwrap();
    assert_eq!(e.used_under(RULE.prefixes), 27);
}

#[test]
fn unmetered_prefixes_are_untouched() {
    let mut e = engine();
    let mut latch = WarnLatch::new();
    for i in 0..8 {
        let key = format!("/state/app/other/{i}");
        assert_eq!(seam_put(&mut e, &mut latch, &key, &[0u8; 500]).0, 0);
    }
    assert_eq!(e.used_under(RULE.prefixes), 0);
    assert_eq!(rule_for(RULES, "/state/app/other/0"), None);
}

#[test]
fn shipped_selftest_quota_matches_the_os_proof() {
    // The QEMU proof (`SELFTEST: quota deny ok`) fills the tree in three
    // 207-byte puts: two fit, the third is refused, a delete frees room.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../policies");
    let tree = nexus_policy::PolicyTree::load_root(&root).unwrap();
    let q = tree.policy().quota("selftest-client").expect("shipped quota");
    let entry = 27 + 180;
    assert!(entry <= q.soft_bytes as usize, "first put fits under soft");
    assert!(2 * entry <= q.hard_bytes as usize, "second put fits under hard");
    assert!(2 * entry > q.soft_bytes as usize, "second put crosses soft (warn)");
    assert!(3 * entry > q.hard_bytes as usize, "third put is refused");
    assert_eq!(q.prefixes, vec!["/state/app/selftest/quota/".to_string()]);
}
