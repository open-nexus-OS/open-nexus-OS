// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: statefsd transaction + compaction core (TASK-0026 step 4) —
//!   cfg-free wire-status mapping for the txn ops, the per-key capability
//!   table for `TXN_PUT`, envelope verify-at-TXN_PUT composition, and the
//!   no-fake-green compaction tick (reopen-verify before the marker).
//!   Shared by the os-lite serve loop and the host contract tests, exactly
//!   like `hardening.rs`.
//! OWNERS: @runtime
//! STATUS: Functional (host-tested; wired into os_lite via txn_os.rs)
//! API_STABILITY: Unstable (service-internal)
//! TEST_COVERAGE: tests/txn_contract.rs (happy path, abort discard, caps,
//!   envelope deny inside txn, cap table, compaction tick)
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

use statefs::envelope::{EnvelopeKey, SeqTracker};
use statefs::journal_v2::MAX_OPEN_TXNS;
use statefs::protocol::{self as proto, txn as txn_proto};
use statefs::{CompactionStats, JournalEngine};
use storage::BlockDevice;

use crate::hardening;

/// Capability required for a `TXN_PUT` to `key` — the same per-key table the
/// plain put path enforces (`os_lite::required_cap` for mutating ops).
#[must_use]
pub fn required_txn_put_cap(key: &str) -> &'static str {
    if key.starts_with("/state/keystore/") {
        "statefs.keystore"
    } else if key.starts_with("/state/boot/") {
        "statefs.boot"
    } else {
        "statefs.write"
    }
}

/// Outcome of a `TXN_PUT` after the policy gate passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxnPutOutcome {
    /// Chunk journaled and buffered.
    Applied,
    /// Chunk journaled and buffered; migration-era raw bytes under the
    /// Integrity floor (caller audits, mirroring the put path).
    AppliedMigration,
    /// Envelope hardening refused the chunk (wire status; caller audits).
    RejectedEnvelope(u8),
    /// Engine refused the chunk (unknown txn, caps, key errors).
    RejectedEngine(u8),
}

/// Open a transaction. The open-txn cap is answered with the appended
/// `STATUS_TXN_LIMIT` (deterministic, distinct from a device I/O error);
/// any residual engine failure maps through the shared status table.
pub fn txn_begin<B: BlockDevice>(engine: &mut JournalEngine<B>) -> Result<u64, u8> {
    if engine.open_txns() >= MAX_OPEN_TXNS {
        return Err(txn_proto::STATUS_TXN_LIMIT);
    }
    engine.txn_begin().map_err(proto::status_from_error)
}

/// Append one chunk to an open transaction, composing the TASK-0025
/// envelope hardening at `TXN_PUT` time (fail early — a forged/stale value
/// never reaches the journal, even transactionally). Documented
/// consequences (docs/storage/statefs.md §Journal v2 → "Service wire ops"):
/// enrolled values must arrive as a single complete chunk, and a
/// verified-then-aborted txn burns the seq (the anti-rollback high-water
/// mark never rolls back — fail-closed).
pub fn txn_put<B: BlockDevice>(
    engine: &mut JournalEngine<B>,
    txn_id: u64,
    key: &str,
    chunk: &[u8],
    mac_key: Option<&EnvelopeKey>,
    tracker: &mut SeqTracker,
) -> TxnPutOutcome {
    let mut migration = false;
    match hardening::verify_put(key, chunk, mac_key, tracker) {
        Ok(hardening::PutCheck::Migration) => migration = true,
        Ok(_) => {}
        Err(err) => return TxnPutOutcome::RejectedEnvelope(proto::status_from_error(err)),
    }
    match engine.txn_append(txn_id, key, chunk) {
        Ok(()) if migration => TxnPutOutcome::AppliedMigration,
        Ok(()) => TxnPutOutcome::Applied,
        Err(err) => TxnPutOutcome::RejectedEngine(proto::status_from_error(err)),
    }
}

/// Commit a transaction; returns the wire status.
pub fn txn_commit<B: BlockDevice>(engine: &mut JournalEngine<B>, txn_id: u64) -> u8 {
    match engine.txn_commit(txn_id) {
        Ok(()) => proto::STATUS_OK,
        Err(err) => proto::status_from_error(err),
    }
}

/// Abort a transaction; returns the wire status.
pub fn txn_abort<B: BlockDevice>(engine: &mut JournalEngine<B>, txn_id: u64) -> u8 {
    match engine.txn_abort(txn_id) {
        Ok(()) => proto::STATUS_OK,
        Err(err) => proto::status_from_error(err),
    }
}

/// Result of one between-requests compaction opportunity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionTick {
    /// Below threshold, deferred (open txns / caps / geometry), or skipped.
    Idle,
    /// A cycle ran AND the rotated journal re-replayed clean.
    Done(CompactionStats),
    /// A cycle ran (or errored) but the post-cycle verify failed — the
    /// caller must NOT emit the done marker (no-fake-green).
    VerifyFailed,
}

/// Threshold-driven compaction between served requests.
///
/// Honesty note (verified against `statefs::compact`): the engine's
/// `compact_now` snapshots and flips the superblock but does NOT re-verify
/// the rotated journal itself. The `statefsd: compaction done` marker
/// therefore requires this tick to run `verify_last_compaction` — a bounded
/// device readback of the superblock + fresh snapshot checked byte-for-byte
/// against what the cycle serialized — before the caller may emit. A full
/// `engine.reopen()` here would prove the same thing but materializes a
/// second engine state every cycle; under the os-lite bump allocator (never
/// frees) that exhausted statefsd's heap after a handful of ambient cycles
/// (QEMU: alloc-fail at gen=7, 384 KiB heap).
pub fn compaction_tick<B: BlockDevice>(engine: &mut JournalEngine<B>) -> CompactionTick {
    if engine.open_txns() != 0 {
        return CompactionTick::Idle;
    }
    let stats = match engine.maybe_compact() {
        Ok(None) => return CompactionTick::Idle,
        Ok(Some(stats)) => stats,
        Err(_) => return CompactionTick::VerifyFailed,
    };
    if engine.verify_last_compaction(&stats) {
        CompactionTick::Done(stats)
    } else {
        CompactionTick::VerifyFailed
    }
}
