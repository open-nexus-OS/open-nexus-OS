// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host contract tests for the statefsd transaction wire ops +
//!   compaction trigger (TASK-0026 step 4): commit-visible/abort-discard
//!   through the cfg-free `statefsd::txn` core, deterministic statuses for
//!   every cap, the per-key capability table for TXN_PUT, envelope deny
//!   inside a transaction, and the reopen-verified compaction tick.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (service-internal contract)
//! TEST_COVERAGE: This file IS the coverage for `statefsd::txn` (mirrors
//!   tests/hardening_contract.rs for `statefsd::hardening`).
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

use statefs::envelope::SeqTracker;
use statefs::journal_v2::{MAX_OPEN_TXNS, MAX_TXN_CHUNK};
use statefs::protocol::txn::{self as txn_proto, TxnRequest};
use statefs::protocol::{self as proto};
use statefs::{CompactionConfig, JournalEngine, StatefsError};
use statefsd::hardening::derive_key_from_device_seed;
use statefsd::txn::{self, CompactionTick, TxnPutOutcome};
use storage::MemBlockDevice;

const KEY_A: &str = "/state/app/selftest/txn/a";
const KEY_B: &str = "/state/app/selftest/txn/b";
const AUTH_KEY: &str = "/state/selftest/secure/token";

fn engine() -> JournalEngine<MemBlockDevice> {
    JournalEngine::open(MemBlockDevice::new(512, 200)).expect("open engine")
}

fn tracker() -> SeqTracker {
    SeqTracker::new()
}

#[test]
fn test_txn_commit_visible_and_survives_reopen() {
    let mut engine = engine();
    let mut tracker = tracker();
    let id = txn::txn_begin(&mut engine).expect("begin");
    assert_eq!(
        txn::txn_put(&mut engine, id, KEY_A, b"alpha", None, &mut tracker),
        TxnPutOutcome::Applied
    );
    assert_eq!(
        txn::txn_put(&mut engine, id, KEY_B, b"beta", None, &mut tracker),
        TxnPutOutcome::Applied
    );
    // Nothing visible before commit.
    assert_eq!(engine.get(KEY_A), Err(StatefsError::NotFound));
    assert_eq!(txn::txn_commit(&mut engine, id), proto::STATUS_OK);
    assert_eq!(engine.get(KEY_A).expect("a"), b"alpha");
    assert_eq!(engine.get(KEY_B).expect("b"), b"beta");
    // Both survive a journal re-replay (the wire Reopen path).
    engine.reopen().expect("reopen");
    assert_eq!(engine.get(KEY_A).expect("a"), b"alpha");
    assert_eq!(engine.get(KEY_B).expect("b"), b"beta");
}

#[test]
fn test_txn_abort_discards_all_buffered_keys() {
    let mut engine = engine();
    let mut tracker = tracker();
    let id = txn::txn_begin(&mut engine).expect("begin");
    assert_eq!(
        txn::txn_put(&mut engine, id, KEY_A, b"alpha", None, &mut tracker),
        TxnPutOutcome::Applied
    );
    assert_eq!(
        txn::txn_put(&mut engine, id, KEY_B, b"beta", None, &mut tracker),
        TxnPutOutcome::Applied
    );
    assert_eq!(txn::txn_abort(&mut engine, id), proto::STATUS_OK);
    assert_eq!(engine.get(KEY_A), Err(StatefsError::NotFound));
    assert_eq!(engine.get(KEY_B), Err(StatefsError::NotFound));
    // Discarded after replay too (prepared-without-commit stays invisible).
    engine.reopen().expect("reopen");
    assert_eq!(engine.get(KEY_A), Err(StatefsError::NotFound));
    assert_eq!(engine.get(KEY_B), Err(StatefsError::NotFound));
    // A later commit of the dead id is NOT_FOUND, never a partial apply.
    assert_eq!(txn::txn_commit(&mut engine, id), proto::STATUS_NOT_FOUND);
}

#[test]
fn test_reject_unknown_txn_id_is_not_found_status() {
    let mut engine = engine();
    let mut tracker = tracker();
    assert_eq!(
        txn::txn_put(&mut engine, 999, KEY_A, b"x", None, &mut tracker),
        TxnPutOutcome::RejectedEngine(proto::STATUS_NOT_FOUND)
    );
    assert_eq!(txn::txn_commit(&mut engine, 999), proto::STATUS_NOT_FOUND);
    assert_eq!(txn::txn_abort(&mut engine, 999), proto::STATUS_NOT_FOUND);
}

#[test]
fn test_reject_txn_begin_over_cap_is_txn_limit_status() {
    let mut engine = engine();
    for _ in 0..MAX_OPEN_TXNS {
        txn::txn_begin(&mut engine).expect("begin under cap");
    }
    assert_eq!(txn::txn_begin(&mut engine), Err(txn_proto::STATUS_TXN_LIMIT));
}

#[test]
fn test_reject_oversize_chunk_is_value_too_large_status() {
    let mut engine = engine();
    let mut tracker = tracker();
    let id = txn::txn_begin(&mut engine).expect("begin");
    let oversized = vec![0u8; MAX_TXN_CHUNK + 1];
    assert_eq!(
        txn::txn_put(&mut engine, id, KEY_A, &oversized, None, &mut tracker),
        TxnPutOutcome::RejectedEngine(proto::STATUS_VALUE_TOO_LARGE)
    );
    // Wire decode enforces the same cap before the engine is reached.
    let mut frame = vec![proto::MAGIC0, proto::MAGIC1, proto::VERSION, txn_proto::OP_TXN_PUT];
    frame.extend_from_slice(&id.to_le_bytes());
    frame.extend_from_slice(&(KEY_A.len() as u16).to_le_bytes());
    frame.extend_from_slice(&(oversized.len() as u32).to_le_bytes());
    frame.extend_from_slice(KEY_A.as_bytes());
    frame.extend_from_slice(&oversized);
    assert_eq!(txn_proto::decode_txn_request(&frame), Err(proto::STATUS_VALUE_TOO_LARGE));
}

#[test]
fn test_reject_envelope_deny_inside_txn_never_reaches_buffer() {
    // Authenticated-mandatory prefix + raw (non-envelope) chunk: refused at
    // TXN_PUT time with the integrity status; a later commit applies nothing.
    let mut engine = engine();
    let mut tracker = tracker();
    let mac_key = derive_key_from_device_seed(&[0x42; 32]).expect("derive");
    let id = txn::txn_begin(&mut engine).expect("begin");
    assert_eq!(
        txn::txn_put(&mut engine, id, AUTH_KEY, b"raw-bytes", Some(&mac_key), &mut tracker),
        TxnPutOutcome::RejectedEnvelope(proto::STATUS_INTEGRITY_VIOLATION)
    );
    assert_eq!(txn::txn_commit(&mut engine, id), proto::STATUS_OK);
    assert_eq!(engine.get(AUTH_KEY), Err(StatefsError::NotFound));
}

#[test]
fn test_reject_txn_put_cap_table_matches_put_path() {
    // Per-key policy inside a txn: privileged prefixes demand privileged
    // caps — a writer holding only statefs.write is denied by policyd.
    assert_eq!(txn::required_txn_put_cap("/state/keystore/device.signing"), "statefs.keystore");
    assert_eq!(txn::required_txn_put_cap("/state/boot/bootctl.v1"), "statefs.boot");
    assert_eq!(txn::required_txn_put_cap(KEY_A), "statefs.write");
    assert_eq!(txn::required_txn_put_cap("/state/selftest/ping"), "statefs.write");
}

#[test]
fn test_compaction_tick_threshold_stats_and_reopen_verify() {
    let mut engine = engine();
    engine.set_compaction_config(CompactionConfig {
        min_journal_bytes: 1024,
        ratio: 2,
        max_entries_per_cycle: 128,
    });
    // Below threshold: idle.
    engine.put(KEY_A, b"seed").expect("put");
    assert_eq!(txn::compaction_tick(&mut engine), CompactionTick::Idle);
    // Overwrite churn: journal grows, live state stays one key.
    for round in 0..50u8 {
        engine.put(KEY_A, &[round; 64]).expect("churn put");
    }
    let stats = match txn::compaction_tick(&mut engine) {
        CompactionTick::Done(stats) => stats,
        other => panic!("expected Done, got {other:?}"),
    };
    assert_eq!(stats.generation, 1);
    assert_eq!(stats.entries, 1);
    // The tick already reopened (rotated journal re-replayed clean): the
    // engine now reports the new generation and intact state.
    assert_eq!(engine.generation(), 1);
    assert_eq!(engine.get(KEY_A).expect("a"), [49u8; 64]);
    // Incremental replay: the reopen scanned only the snapshot (+ nothing).
    assert_eq!(engine.replayed_records(), stats.entries + 1);
    // Immediately after a cycle: idle again (below min_journal_bytes).
    assert_eq!(txn::compaction_tick(&mut engine), CompactionTick::Idle);
}

#[test]
fn test_compaction_defers_while_txn_open_then_runs() {
    let mut engine = engine();
    engine.set_compaction_config(CompactionConfig {
        min_journal_bytes: 1024,
        ratio: 2,
        max_entries_per_cycle: 128,
    });
    for round in 0..50u8 {
        engine.put(KEY_A, &[round; 64]).expect("churn put");
    }
    let id = txn::txn_begin(&mut engine).expect("begin");
    // Open transaction: the tick must defer (never rotate PREPARE records
    // out from under an in-flight txn).
    assert_eq!(txn::compaction_tick(&mut engine), CompactionTick::Idle);
    assert_eq!(txn::txn_abort(&mut engine, id), proto::STATUS_OK);
    assert!(matches!(txn::compaction_tick(&mut engine), CompactionTick::Done(_)));
    assert_eq!(engine.get(KEY_A).expect("a"), [49u8; 64]);
}

#[test]
fn test_txn_wire_roundtrip_v1_and_v2_nonce() {
    // BEGIN (v1) and the nonce-correlated v2 upgrade the selftest client uses.
    let begin = txn_proto::encode_txn_begin_request();
    assert_eq!(txn_proto::decode_txn_request(&begin), Ok((TxnRequest::Begin, None)));
    let mut v2 = Vec::new();
    v2.extend_from_slice(&begin[..4]);
    v2[2] = proto::VERSION_V2;
    v2.extend_from_slice(&7u64.to_le_bytes());
    v2.extend_from_slice(&begin[4..]);
    assert_eq!(txn_proto::decode_txn_request(&v2), Ok((TxnRequest::Begin, Some(7))));

    let put = txn_proto::encode_txn_put_request(3, KEY_A, b"chunk").expect("encode put");
    assert_eq!(
        txn_proto::decode_txn_request(&put),
        Ok((TxnRequest::Put { txn_id: 3, key: KEY_A, chunk: b"chunk" }, None))
    );
    let commit = txn_proto::encode_txn_commit_request(3);
    assert_eq!(
        txn_proto::decode_txn_request(&commit),
        Ok((TxnRequest::Commit { txn_id: 3 }, None))
    );
    let abort = txn_proto::encode_txn_abort_request(3);
    assert_eq!(txn_proto::decode_txn_request(&abort), Ok((TxnRequest::Abort { txn_id: 3 }, None)));

    // BEGIN response roundtrip (v1 + v2).
    let rsp = txn_proto::encode_txn_begin_response(proto::STATUS_OK, 42, None);
    assert_eq!(txn_proto::decode_txn_begin_response(&rsp), Ok((proto::STATUS_OK, 42)));
    let rsp = txn_proto::encode_txn_begin_response(txn_proto::STATUS_TXN_LIMIT, 0, Some(9));
    assert_eq!(txn_proto::decode_txn_begin_response(&rsp), Ok((txn_proto::STATUS_TXN_LIMIT, 0)));
}

#[test]
fn test_reject_malformed_txn_frames() {
    // Trailing bytes on BEGIN.
    let mut begin = txn_proto::encode_txn_begin_request();
    begin.push(0);
    assert_eq!(txn_proto::decode_txn_request(&begin), Err(proto::STATUS_MALFORMED));
    // Truncated COMMIT id.
    let commit = vec![proto::MAGIC0, proto::MAGIC1, proto::VERSION, txn_proto::OP_TXN_COMMIT, 1];
    assert_eq!(txn_proto::decode_txn_request(&commit), Err(proto::STATUS_MALFORMED));
    // Non-txn opcode is refused by the txn decoder.
    let put = proto::encode_put_request(KEY_A, b"v").expect("encode");
    assert_eq!(txn_proto::decode_txn_request(&put), Err(proto::STATUS_UNSUPPORTED));
    // Empty-key TXN_PUT.
    let mut frame = vec![proto::MAGIC0, proto::MAGIC1, proto::VERSION, txn_proto::OP_TXN_PUT];
    frame.extend_from_slice(&1u64.to_le_bytes());
    frame.extend_from_slice(&0u16.to_le_bytes());
    frame.extend_from_slice(&0u32.to_le_bytes());
    assert_eq!(txn_proto::decode_txn_request(&frame), Err(proto::STATUS_MALFORMED));
}
