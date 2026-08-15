// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]
#![forbid(unsafe_code)]

//! CONTEXT: statefsd os-lite transaction wire glue (TASK-0026 step 4) —
//!   decodes the appended txn ops, applies the policyd gates (same
//!   capability table as the put path) and the envelope hardening at
//!   TXN_PUT time, then delegates to the cfg-free `crate::txn` core.
//!   Also hosts the between-requests compaction tick (marker emission
//!   only after the core's reopen-verify).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (service-internal)
//! TEST_COVERAGE: Core logic host-tested via tests/txn_contract.rs; the
//!   IPC/policy glue is proven by the QEMU marker ladder.
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

extern crate alloc;

use alloc::vec::Vec;

use statefs::envelope::PolicyClass;
use statefs::protocol::{self as proto, txn as txn_proto};
use statefs::JournalEngine;

use crate::emit_os::{
    emit_access_denied, emit_compaction_done, emit_compaction_verify_failed, emit_envelope_denied,
    emit_envelope_migration,
};
use crate::os_lite::{
    envelope_mac_key, policyd_allows, Backend, Hardening, CAP_BOOT, CAP_WRITE,
    MAX_INLINE_VALUE_BYTES,
};
use crate::{hardening, txn};

/// Handle one transaction-op frame (ops 7–10). Mirrors the shape of
/// `os_lite::handle_frame`: decode, policy, verify, engine, encode.
pub(crate) fn handle_txn_frame(
    engine: &mut JournalEngine<Backend>,
    hard: &mut Hardening,
    sender_service_id: u64,
    frame: &[u8],
) -> Vec<u8> {
    let op_hint = frame.get(3).copied().unwrap_or(txn_proto::OP_TXN_BEGIN);
    let (request, nonce) = match txn_proto::decode_txn_request(frame) {
        Ok(v) => v,
        Err(status) => return proto::encode_status_response_with_nonce(op_hint, status, None),
    };

    match request {
        txn_proto::TxnRequest::Begin => {
            if !txn_ctl_allowed(sender_service_id) {
                emit_access_denied("/state", sender_service_id);
                return txn_proto::encode_txn_begin_response(proto::STATUS_ACCESS_DENIED, 0, nonce);
            }
            match txn::txn_begin(engine) {
                Ok(txn_id) => txn_proto::encode_txn_begin_response(proto::STATUS_OK, txn_id, nonce),
                Err(status) => txn_proto::encode_txn_begin_response(status, 0, nonce),
            }
        }
        txn_proto::TxnRequest::Put { txn_id, key, chunk } => {
            if chunk.len() > MAX_INLINE_VALUE_BYTES {
                return proto::encode_status_response_with_nonce(
                    txn_proto::OP_TXN_PUT,
                    proto::STATUS_VALUE_TOO_LARGE,
                    nonce,
                );
            }
            if !txn_put_allowed(sender_service_id, key) {
                emit_access_denied(key, sender_service_id);
                return proto::encode_status_response_with_nonce(
                    txn_proto::OP_TXN_PUT,
                    proto::STATUS_ACCESS_DENIED,
                    nonce,
                );
            }
            // TASK-0025 composition: envelope policy check at TXN_PUT time
            // (fail early) — forged/stale values never reach the journal,
            // transactionally or not.
            let mac_key = if hardening::policy_class_for(key) == PolicyClass::Authenticated {
                envelope_mac_key(engine, &mut hard.key)
            } else {
                None
            };
            let status = match txn::txn_put(engine, txn_id, key, chunk, mac_key, &mut hard.tracker)
            {
                txn::TxnPutOutcome::Applied => proto::STATUS_OK,
                txn::TxnPutOutcome::AppliedMigration => {
                    emit_envelope_migration(key);
                    proto::STATUS_OK
                }
                txn::TxnPutOutcome::RejectedEnvelope(status) => {
                    emit_envelope_denied(key, status);
                    status
                }
                txn::TxnPutOutcome::RejectedEngine(status) => status,
            };
            proto::encode_status_response_with_nonce(txn_proto::OP_TXN_PUT, status, nonce)
        }
        txn_proto::TxnRequest::Commit { txn_id } => {
            if !txn_ctl_allowed(sender_service_id) {
                emit_access_denied("/state", sender_service_id);
                return proto::encode_status_response_with_nonce(
                    txn_proto::OP_TXN_COMMIT,
                    proto::STATUS_ACCESS_DENIED,
                    nonce,
                );
            }
            let status = txn::txn_commit(engine, txn_id);
            proto::encode_status_response_with_nonce(txn_proto::OP_TXN_COMMIT, status, nonce)
        }
        txn_proto::TxnRequest::Abort { txn_id } => {
            if !txn_ctl_allowed(sender_service_id) {
                emit_access_denied("/state", sender_service_id);
                return proto::encode_status_response_with_nonce(
                    txn_proto::OP_TXN_ABORT,
                    proto::STATUS_ACCESS_DENIED,
                    nonce,
                );
            }
            let status = txn::txn_abort(engine, txn_id);
            proto::encode_status_response_with_nonce(txn_proto::OP_TXN_ABORT, status, nonce)
        }
    }
}

/// Between-requests compaction opportunity. Emits the done marker ONLY on
/// `CompactionTick::Done` — i.e. after the core re-opened the engine and the
/// rotated journal re-replayed clean (no-fake-green).
pub(crate) fn compaction_tick(engine: &mut JournalEngine<Backend>) {
    match txn::compaction_tick(engine) {
        txn::CompactionTick::Idle => {}
        txn::CompactionTick::Done(stats) => emit_compaction_done(stats.generation, stats.entries),
        txn::CompactionTick::VerifyFailed => emit_compaction_verify_failed(),
    }
}

/// `TXN_BEGIN`/`TXN_COMMIT`/`TXN_ABORT` carry no key: durability/atomicity
/// control mirrors `Sync`/`Reopen` — boot authority or generic state writer.
fn txn_ctl_allowed(sender_service_id: u64) -> bool {
    policyd_allows(sender_service_id, CAP_BOOT.as_bytes())
        || policyd_allows(sender_service_id, CAP_WRITE.as_bytes())
}

/// Per-key gate for `TXN_PUT` — same capability table as the plain put path
/// (`txn::required_txn_put_cap`), same canonical-subject mapping.
fn txn_put_allowed(sender_service_id: u64, key: &str) -> bool {
    let cap = txn::required_txn_put_cap(key);
    let selftest_sid = nexus_abi::service_id_from_name(b"selftest-client");
    let metricsd_sid = nexus_abi::service_id_from_name(b"metricsd");
    policyd_allows(
        crate::canonical_policy_subject_for_statefs(
            sender_service_id,
            txn_proto::OP_TXN_PUT,
            key,
            selftest_sid,
            metricsd_sid,
        ),
        cap.as_bytes(),
    )
}
