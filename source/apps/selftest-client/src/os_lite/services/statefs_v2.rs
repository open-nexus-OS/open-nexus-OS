// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: statefs journal-v2 selftest probes (TASK-0026 step 4) — 2PC
//!   crash-atomicity over the txn wire ops (abort discards / commit survives
//!   Reopen replay), bounded compaction under real churn (compaction-done
//!   line cross-checked via logd, live keys verified after Reopen), and the
//!   keep-blk cold-boot sentinel discipline (ok only on a second boot
//!   against a preserved image).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — bringup phase.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

extern crate alloc;

use nexus_ipc::KernelClient;
use statefs::protocol::txn as txn_proto;
use statefs::protocol::{self as statefs_proto};
use statefs::StatefsError;

use super::logd::logd_query_contains_since_paged;
use super::statefs::statefs_send_recv;

// All probe keys live under the allowlisted selftest write prefix
// (`abi_profile."selftest-client".statefs_put_allow_prefix` in
// policies/base.toml).
const TXN_KEY_A: &str = "/state/app/selftest/txn/a";
const TXN_KEY_B: &str = "/state/app/selftest/txn/b";
const TXN_VAL_A: &[u8] = b"txn-alpha-v1";
const TXN_VAL_B: &[u8] = b"txn-beta-v1";

/// Churn keys (static: the os-lite bump allocator never frees `format!`).
const COMPACT_KEYS: [&str; 6] = [
    "/state/app/selftest/compact/k0",
    "/state/app/selftest/compact/k1",
    "/state/app/selftest/compact/k2",
    "/state/app/selftest/compact/k3",
    "/state/app/selftest/compact/k4",
    "/state/app/selftest/compact/k5",
];
/// Churn value size: 6 keys x 16 rounds x (~15+30+256) journal bytes per
/// batch ≈ 29 KiB — deterministically crosses statefsd's DEFAULT compaction
/// threshold (min 8 KiB, ratio 2x) within the first batch; up to 4 bounded
/// batches absorb pathological live-store sizes. No test-only threshold.
const COMPACT_VAL_LEN: usize = 256;
const COMPACT_ROUNDS_PER_BATCH: u8 = 16;
const COMPACT_MAX_BATCHES: u8 = 4;

const COLD_BOOT_KEY: &str = "/state/app/selftest/coldboot.sentinel";
const COLD_BOOT_VAL: &[u8] = b"cold-boot-v1";

/// Cold-boot sentinel outcome (the caller maps this onto markers).
pub(crate) enum ColdBoot {
    /// Sentinel was absent: seeded + synced for the NEXT boot (info only —
    /// never the ok marker on the boot that wrote it).
    Seeded,
    /// Sentinel present with the expected value: this is a second boot
    /// against a preserved image (NEXUS_KEEP_BLK=1) — the honest cold-boot
    /// persistence proof.
    Present,
}

fn put_ok(client: &KernelClient, key: &str, value: &[u8]) -> core::result::Result<(), ()> {
    let frame = statefs_proto::encode_put_request(key, value).map_err(|_| ())?;
    let rsp = statefs_send_recv(client, &frame)?;
    match statefs_proto::decode_status_response(statefs_proto::OP_PUT, &rsp) {
        Ok(statefs_proto::STATUS_OK) => Ok(()),
        _ => Err(()),
    }
}

fn get_value(client: &KernelClient, key: &str) -> core::result::Result<alloc::vec::Vec<u8>, ()> {
    let frame =
        statefs_proto::encode_key_only_request(statefs_proto::OP_GET, key).map_err(|_| ())?;
    let rsp = statefs_send_recv(client, &frame)?;
    statefs_proto::decode_get_response(&rsp).map_err(|_| ())
}

/// GET that must answer NOT_FOUND (any other reply is a failure).
fn get_not_found(client: &KernelClient, key: &str) -> core::result::Result<(), ()> {
    let frame =
        statefs_proto::encode_key_only_request(statefs_proto::OP_GET, key).map_err(|_| ())?;
    let rsp = statefs_send_recv(client, &frame)?;
    match statefs_proto::decode_get_response(&rsp) {
        Err(StatefsError::NotFound) => Ok(()),
        _ => Err(()),
    }
}

/// DELETE tolerated as OK or NOT_FOUND (cleanup before the txn proof).
fn delete_lenient(client: &KernelClient, key: &str) -> core::result::Result<(), ()> {
    let frame =
        statefs_proto::encode_key_only_request(statefs_proto::OP_DEL, key).map_err(|_| ())?;
    let rsp = statefs_send_recv(client, &frame)?;
    match statefs_proto::decode_status_response(statefs_proto::OP_DEL, &rsp) {
        Ok(statefs_proto::STATUS_OK) | Ok(statefs_proto::STATUS_NOT_FOUND) => Ok(()),
        _ => Err(()),
    }
}

fn status_op(client: &KernelClient, frame: &[u8], op: u8) -> core::result::Result<u8, ()> {
    let rsp = statefs_send_recv(client, frame)?;
    statefs_proto::decode_status_response(op, &rsp).map_err(|_| ())
}

fn txn_begin(client: &KernelClient) -> core::result::Result<u64, ()> {
    let rsp = statefs_send_recv(client, &txn_proto::encode_txn_begin_request())?;
    match txn_proto::decode_txn_begin_response(&rsp) {
        Ok((statefs_proto::STATUS_OK, txn_id)) => Ok(txn_id),
        _ => Err(()),
    }
}

fn txn_put(
    client: &KernelClient,
    txn_id: u64,
    key: &str,
    chunk: &[u8],
) -> core::result::Result<(), ()> {
    let frame = txn_proto::encode_txn_put_request(txn_id, key, chunk).map_err(|_| ())?;
    match status_op(client, &frame, txn_proto::OP_TXN_PUT) {
        Ok(statefs_proto::STATUS_OK) => Ok(()),
        _ => Err(()),
    }
}

fn txn_commit(client: &KernelClient, txn_id: u64) -> core::result::Result<(), ()> {
    match status_op(client, &txn_proto::encode_txn_commit_request(txn_id), txn_proto::OP_TXN_COMMIT)
    {
        Ok(statefs_proto::STATUS_OK) => Ok(()),
        _ => Err(()),
    }
}

fn txn_abort(client: &KernelClient, txn_id: u64) -> core::result::Result<(), ()> {
    match status_op(client, &txn_proto::encode_txn_abort_request(txn_id), txn_proto::OP_TXN_ABORT) {
        Ok(statefs_proto::STATUS_OK) => Ok(()),
        _ => Err(()),
    }
}

fn sync_and_reopen(client: &KernelClient) -> core::result::Result<(), ()> {
    match status_op(client, &statefs_proto::encode_sync_request(), statefs_proto::OP_SYNC) {
        Ok(statefs_proto::STATUS_OK) => {}
        _ => return Err(()),
    }
    match status_op(client, &statefs_proto::encode_reopen_request(), statefs_proto::OP_REOPEN) {
        Ok(statefs_proto::STATUS_OK) => Ok(()),
        _ => Err(()),
    }
}

/// 2PC crash-atomicity over the wire: an aborted transaction leaves neither
/// key visible (also proven invisible BEFORE commit), a committed
/// transaction leaves both visible after Reopen — the protocol Reopen
/// re-replays the journal, which is the honest in-VM replay proof.
pub(crate) fn statefs_txn_crash_atomic(client: &KernelClient) -> core::result::Result<(), ()> {
    // Clean slate (keep-blk second boots may carry committed values).
    delete_lenient(client, TXN_KEY_A)?;
    delete_lenient(client, TXN_KEY_B)?;

    // Abort path: prepared + buffered, never visible.
    let txn = txn_begin(client)?;
    txn_put(client, txn, TXN_KEY_A, TXN_VAL_A)?;
    txn_put(client, txn, TXN_KEY_B, TXN_VAL_B)?;
    get_not_found(client, TXN_KEY_A)?; // nothing visible before commit
    txn_abort(client, txn)?;
    get_not_found(client, TXN_KEY_A)?;
    get_not_found(client, TXN_KEY_B)?;

    // Commit path: both-or-neither, and both survive a journal re-replay.
    let txn = txn_begin(client)?;
    if txn == 0 {
        return Err(());
    }
    txn_put(client, txn, TXN_KEY_A, TXN_VAL_A)?;
    txn_put(client, txn, TXN_KEY_B, TXN_VAL_B)?;
    txn_commit(client, txn)?;
    if get_value(client, TXN_KEY_A)?.as_slice() != TXN_VAL_A {
        return Err(());
    }
    sync_and_reopen(client)?;
    if get_value(client, TXN_KEY_A)?.as_slice() != TXN_VAL_A {
        return Err(());
    }
    if get_value(client, TXN_KEY_B)?.as_slice() != TXN_VAL_B {
        return Err(());
    }
    Ok(())
}

fn compact_value(batch: u8, round: u8, key_idx: u8) -> [u8; COMPACT_VAL_LEN] {
    let mut value = [0xC5u8; COMPACT_VAL_LEN];
    value[0] = batch;
    value[1] = round;
    value[2] = key_idx;
    value
}

/// Compaction under churn: overwrite a fixed key set until statefsd's
/// between-requests trigger fires with the DEFAULT thresholds, cross-check
/// the audited `statefsd: compaction done` line via logd (the service emits
/// it only after its reopen-verify — no fake trigger anywhere), then Reopen
/// and verify every churn key carries its last-written value.
pub(crate) fn statefs_compact_churn(client: &KernelClient) -> core::result::Result<(), ()> {
    let logd = KernelClient::new_for("logd").map_err(|_| ())?;
    let mut compacted = false;
    let mut last: (u8, u8) = (0, 0);
    for batch in 0..COMPACT_MAX_BATCHES {
        for round in 0..COMPACT_ROUNDS_PER_BATCH {
            for (key_idx, key) in COMPACT_KEYS.iter().enumerate() {
                let value = compact_value(batch, round, key_idx as u8);
                put_ok(client, key, &value)?;
            }
            last = (batch, round);
        }
        if logd_query_contains_since_paged(
            &logd,
            0,
            crate::markers::M_STATEFSD_COMPACTION_DONE_GEN.as_bytes(),
        )
        .unwrap_or(false)
        {
            compacted = true;
            break;
        }
    }
    if !compacted {
        return Err(());
    }
    sync_and_reopen(client)?;
    for (key_idx, key) in COMPACT_KEYS.iter().enumerate() {
        let expected = compact_value(last.0, last.1, key_idx as u8);
        if get_value(client, key)?.as_slice() != expected {
            return Err(());
        }
    }
    Ok(())
}

/// Cold-boot sentinel discipline: absent → seed (+sync) for the next boot;
/// present with the expected value → this boot replayed a PRESERVED image
/// (NEXUS_KEEP_BLK=1) — only then may the ok marker appear. A present-but-
/// wrong value is a hard failure.
pub(crate) fn statefs_cold_boot(client: &KernelClient) -> core::result::Result<ColdBoot, ()> {
    let frame = statefs_proto::encode_key_only_request(statefs_proto::OP_GET, COLD_BOOT_KEY)
        .map_err(|_| ())?;
    let rsp = statefs_send_recv(client, &frame)?;
    match statefs_proto::decode_get_response(&rsp) {
        Ok(value) => {
            if value.as_slice() == COLD_BOOT_VAL {
                Ok(ColdBoot::Present)
            } else {
                Err(())
            }
        }
        Err(StatefsError::NotFound) => {
            put_ok(client, COLD_BOOT_KEY, COLD_BOOT_VAL)?;
            match status_op(client, &statefs_proto::encode_sync_request(), statefs_proto::OP_SYNC) {
                Ok(statefs_proto::STATUS_OK) => Ok(ColdBoot::Seeded),
                _ => Err(()),
            }
        }
        Err(_) => Err(()),
    }
}
