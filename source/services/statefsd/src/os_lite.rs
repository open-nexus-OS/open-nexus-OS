// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]
#![forbid(unsafe_code)]

//! CONTEXT: statefsd os-lite backend (kernel IPC server; byte-frame protocol v1)
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

use core::fmt;

use nexus_abi::yield_;
use nexus_ipc::{KernelServer, Server as _, Wait};

use statefs::envelope::{EnvelopeKey, PolicyClass, SeqTracker, WriteBudget};
use statefs::protocol::{self as proto, Request};
use statefs::JournalEngine;
use storage::virtio_blk::VirtioBlkDevice;
use storage::BlockDevice;
use storage::MemBlockDevice;

use crate::emit_os::{
    emit_access_denied, emit_blk_marker, emit_budget_warn, emit_envelope_denied,
    emit_envelope_migration, emit_ipc_error, emit_line, emit_statefs_error,
};
use crate::hardening;

/// Result alias surfaced by the lite statefsd backend.
pub type LiteResult<T> = Result<T, ServerError>;

/// Ready notifier invoked once the service becomes available.
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    /// Creates a notifier from the provided closure.
    pub fn new<F>(func: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self(Box::new(func))
    }

    /// Signals readiness to the caller.
    pub fn notify(self) {
        (self.0)();
    }
}

/// Errors surfaced by the lite backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerError {
    /// Functionality not yet available in the os-lite path.
    Unsupported,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "statefsd unsupported"),
        }
    }
}

/// Schema warmer placeholder for API parity.
pub fn touch_schemas() {}

const BLOCK_SIZE: usize = 512;
const BLOCK_COUNT: u64 = 64;
const MAX_LIST_RESPONSE_BYTES: usize = 512;
const IPC_MAX_FRAME_BYTES: usize = 8 * 1024;
pub(crate) const MAX_INLINE_VALUE_BYTES: usize = IPC_MAX_FRAME_BYTES - 64;

const CAP_READ: &str = "statefs.read";
pub(crate) const CAP_WRITE: &str = "statefs.write";
const CAP_KEYSTORE: &str = "statefs.keystore";
pub(crate) const CAP_BOOT: &str = "statefs.boot";

pub(crate) enum Backend {
    Virtio(VirtioBlkDevice),
    Mem(MemBlockDevice),
}

impl BlockDevice for Backend {
    fn block_size(&self) -> usize {
        match self {
            Backend::Virtio(dev) => dev.block_size(),
            Backend::Mem(dev) => dev.block_size(),
        }
    }

    fn block_count(&self) -> u64 {
        match self {
            Backend::Virtio(dev) => dev.block_count(),
            Backend::Mem(dev) => dev.block_count(),
        }
    }

    fn read_block(&self, block_idx: u64, buf: &mut [u8]) -> Result<(), storage::BlockError> {
        match self {
            Backend::Virtio(dev) => dev.read_block(block_idx, buf),
            Backend::Mem(dev) => dev.read_block(block_idx, buf),
        }
    }

    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), storage::BlockError> {
        match self {
            Backend::Virtio(dev) => dev.write_block(block_idx, buf),
            Backend::Mem(dev) => dev.write_block(block_idx, buf),
        }
    }

    fn sync(&mut self) -> Result<(), storage::BlockError> {
        match self {
            Backend::Virtio(dev) => dev.sync(),
            Backend::Mem(dev) => dev.sync(),
        }
    }
}

/// TASK-0025 write-path hardening state carried by the serve loop
/// (shared with the txn glue in `txn_os.rs`, TASK-0026).
pub(crate) struct Hardening {
    /// Per-key max-seen seq (anti-rollback), fed at replay + on accepted puts.
    pub(crate) tracker: SeqTracker,
    /// Deterministic write-latency budget (warn accounting to audit/log).
    budget: WriteBudget,
    /// Lazily derived envelope MAC key (chicken-egg rule: Integrity-class
    /// prefixes are served before keystored has generated the device key).
    pub(crate) key: Option<EnvelopeKey>,
}

impl Hardening {
    fn new() -> Self {
        Self { tracker: SeqTracker::new(), budget: hardening::new_write_budget(), key: None }
    }
}

/// Lazily derive (and cache) the envelope MAC key from the persisted
/// device-key record. Returns None until keystored has generated it —
/// Authenticated-class ops fail closed until then.
pub(crate) fn envelope_mac_key<'a>(
    engine: &JournalEngine<Backend>,
    cached: &'a mut Option<EnvelopeKey>,
) -> Option<&'a EnvelopeKey> {
    if cached.is_none() {
        if let Ok(bytes) = engine.get(hardening::DEVICE_KEY_PATH) {
            // TASK-0025 step 4: keystored wraps the record as an Integrity
            // envelope; pre-migration journals still hold the raw 32-byte
            // seed. `open_stored` handles both (deterministic, no panic).
            if let Ok(stored) = statefs::writer::open_stored(&bytes) {
                let payload = stored.payload();
                if payload.len() == 32 {
                    let mut seed = [0u8; 32];
                    seed.copy_from_slice(payload);
                    if let Ok(key) = hardening::derive_key_from_device_seed(&seed) {
                        *cached = Some(key);
                    }
                }
            }
        }
    }
    cached.as_ref()
}

/// Replay-time anti-rollback feed: walk the enrolled prefixes of a freshly
/// replayed engine and record each envelope seq. Bounded; malformed or
/// migration-era (non-envelope) values are skipped without panic.
fn observe_enrolled(engine: &JournalEngine<Backend>, tracker: &mut SeqTracker) {
    /// Per-prefix replay-walk bound (matches the store's practical key count).
    const PER_PREFIX_LIMIT: usize = 256;
    for prefix in hardening::ENROLLED_PREFIXES {
        let keys = match engine.list(prefix, PER_PREFIX_LIMIT) {
            Ok(keys) => keys,
            Err(_) => continue,
        };
        for key in keys {
            let value = match engine.get(&key) {
                Ok(value) => value,
                Err(_) => continue,
            };
            if hardening::observe_replayed(&key, &value, tracker).is_err() {
                // Tracker capacity exhausted: fail loud, keep serving (the
                // put path still rejects stale seqs for tracked keys).
                emit_line("statefsd: replay observe limit");
                return;
            }
        }
    }
}

/// Main statefsd bring-up service loop (os-lite).
pub fn service_main_loop(notifier: ReadyNotifier) -> LiteResult<()> {
    emit_line("statefsd: entry");
    // init-lite transfers the statefsd service endpoints into deterministic
    // slots (recv: slot 3, send: slot 4); using these directly avoids
    // routing-time races during early bring-up.
    let server = {
        const RECV_SLOT: u32 = 0x03;
        const SEND_SLOT: u32 = 0x04;
        let deadline = match nexus_abi::nsec() {
            Ok(now) => now.saturating_add(10_000_000_000), // 10s
            Err(_) => 0,
        };
        loop {
            let recv_ok =
                nexus_abi::cap_clone(RECV_SLOT).map(|tmp| nexus_abi::cap_close(tmp)).is_ok();
            let send_ok =
                nexus_abi::cap_clone(SEND_SLOT).map(|tmp| nexus_abi::cap_close(tmp)).is_ok();
            if recv_ok && send_ok {
                break KernelServer::new_with_slots(RECV_SLOT, SEND_SLOT)
                    .map_err(|_| ServerError::Unsupported)?;
            }
            if deadline != 0 {
                if let Ok(now) = nexus_abi::nsec() {
                    if now >= deadline {
                        return Err(ServerError::Unsupported);
                    }
                }
            }
            let _ = yield_();
        }
    };

    // Start with an in-memory backend so we can become ready deterministically even if
    // virtio-blk MMIO caps are granted later in bring-up.
    let mut engine =
        match JournalEngine::open(Backend::Mem(MemBlockDevice::new(BLOCK_SIZE, BLOCK_COUNT))) {
            Ok(engine) => engine,
            Err(err) => {
                emit_line("statefsd: journal open failed (mem)");
                emit_statefs_error(err);
                return Err(ServerError::Unsupported);
            }
        };
    // TASK-0026: journal v2 (2PC committed-only replay + bounded compaction)
    // is active on every successful open of this engine — emitted once,
    // only after open() actually succeeded above.
    emit_line("statefsd: journal v2 mounted (2PC)");
    // Track whether we've processed any mutating operations yet. We'll only "upgrade"
    // to the virtio-blk backend while still pristine, to avoid losing in-memory state.
    let mut pristine = true;
    let mut virtio_upgraded = false;
    let mut virtio_retry_count = 0u8;
    const VIRTIO_MAX_RETRIES: u8 = 5;

    // TASK-0025: write-path hardening — policy table is const, the seq
    // tracker is fed from the replayed engine, the MAC key derives lazily
    // (chicken-egg rule). Enforcement is active for every frame below.
    let mut hard = Hardening::new();
    observe_enrolled(&engine, &mut hard.tracker);

    notifier.notify();
    emit_line("statefsd: ready");
    emit_line("statefsd: write hardening on (auth-envelope)");

    nexus_abi::service_verdict_flush("statefsd");
    // SMP robustness circuit breaker (see the Err arm below).
    let mut breaker = nexus_ipc::resilience::CircuitBreaker::new(64, 3);
    loop {
        match server.recv_request_with_meta(Wait::Blocking) {
            Ok((frame, sender_service_id, reply)) => {
                breaker.on_success();
                if pristine && !virtio_upgraded && virtio_retry_count < VIRTIO_MAX_RETRIES {
                    let mut q = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
                    let mmio_ready = nexus_abi::cap_query(48, &mut q).is_ok() && q.kind_tag == 2;
                    if mmio_ready {
                        if let Ok(blk) = VirtioBlkDevice::new(48) {
                            emit_blk_marker(&blk);
                            match JournalEngine::open(Backend::Virtio(blk)) {
                                Ok(new_engine) => {
                                    engine = new_engine;
                                    virtio_upgraded = true;
                                    // New backing store: rebuild the
                                    // anti-rollback state from its replay
                                    // (the mem engine was still pristine).
                                    hard.tracker = SeqTracker::new();
                                    observe_enrolled(&engine, &mut hard.tracker);
                                    emit_line("statefsd: virtio upgrade ok");
                                }
                                Err(err) => {
                                    virtio_retry_count += 1;
                                    emit_line("statefsd: journal open failed (virtio)");
                                    emit_statefs_error(err);
                                    // Delay before next retry to let QEMU virtio settle
                                    if virtio_retry_count < VIRTIO_MAX_RETRIES {
                                        emit_line("statefsd: virtio retry scheduled");
                                        for _ in 0..100 {
                                            let _ = yield_();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                let rsp = handle_frame(&mut engine, &mut hard, sender_service_id, frame.as_slice());
                // Once we accept a mutating op, we no longer allow backend upgrade.
                if let Some(op) = frame.get(3).copied() {
                    if matches!(
                        op,
                        proto::OP_PUT | proto::OP_DEL | proto::OP_SYNC | proto::OP_REOPEN
                    ) || proto::txn::is_txn_op(op)
                    {
                        pristine = false;
                    }
                }
                if let Some(reply) = reply {
                    if reply.reply_and_close(&rsp).is_err() {
                        emit_line("statefsd: reply send fail");
                    }
                } else {
                    if server.send(&rsp, Wait::Blocking).is_err() {
                        emit_line("statefsd: send fail");
                    }
                }
                // TASK-0026: between-requests compaction opportunity (the
                // tick itself defers while transactions are open; the done
                // marker is emitted only after a reopen-verified cycle).
                crate::txn_os::compaction_tick(&mut engine);
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::PermissionDenied)) => {
                // Treat as transient; the control plane can still be settling during bring-up.
                let _ = yield_();
            }
            Err(err) => {
                // SMP robustness: client-side errors are transient; only a
                // persistent error streak (our own endpoint defect) ends the
                // loop. See nexus_ipc::resilience for the rationale.
                let (should_log, verdict) = breaker.on_error();
                if should_log {
                    emit_line("statefsd: transient ipc error (continuing)");
                    emit_ipc_error(err);
                }
                match verdict {
                    nexus_ipc::resilience::BreakerVerdict::Continue => {
                        let _ = yield_();
                    }
                    nexus_ipc::resilience::BreakerVerdict::EndpointDefect => {
                        emit_line("statefsd: endpoint defect (consecutive error limit)");
                        return Err(ServerError::Unsupported);
                    }
                }
            }
        }
    }
}

fn handle_frame(
    engine: &mut JournalEngine<Backend>,
    hard: &mut Hardening,
    sender_service_id: u64,
    frame: &[u8],
) -> Vec<u8> {
    let op_hint = frame.get(3).copied().unwrap_or(proto::OP_GET);
    // TASK-0026: transaction ops (7–10) are decoded and served by the txn
    // glue (same policy authority, envelope hardening at TXN_PUT time).
    if proto::txn::is_txn_op(op_hint) {
        return crate::txn_os::handle_txn_frame(engine, hard, sender_service_id, frame);
    }
    let (request, nonce) = match proto::decode_request_with_nonce(frame) {
        Ok(v) => v,
        Err(status) => return proto::encode_status_response_with_nonce(op_hint, status, None),
    };

    match request {
        Request::Put { key, value } => {
            if value.len() > MAX_INLINE_VALUE_BYTES {
                return proto::encode_status_response_with_nonce(
                    proto::OP_PUT,
                    proto::STATUS_VALUE_TOO_LARGE,
                    nonce,
                );
            }
            if !policy_allows(sender_service_id, proto::OP_PUT, key) {
                emit_access_denied(key, sender_service_id);
                return proto::encode_status_response_with_nonce(
                    proto::OP_PUT,
                    proto::STATUS_ACCESS_DENIED,
                    nonce,
                );
            }
            // TASK-0025: envelope policy check (fail-closed for enrolled
            // prefixes) BEFORE the journal append; forged/stale values must
            // never reach the medium.
            let mac_key = if hardening::policy_class_for(key) == PolicyClass::Authenticated {
                envelope_mac_key(engine, &mut hard.key)
            } else {
                None
            };
            match hardening::verify_put(key, value, mac_key, &mut hard.tracker) {
                Ok(hardening::PutCheck::Migration) => emit_envelope_migration(key),
                Ok(_) => {}
                Err(err) => {
                    let status = proto::status_from_error(err);
                    emit_envelope_denied(key, status);
                    return proto::encode_status_response_with_nonce(proto::OP_PUT, status, nonce);
                }
            }
            let started_ns = nexus_abi::nsec().ok();
            let result = engine.put(key, value);
            if let (Some(start), Ok(now)) = (started_ns, nexus_abi::nsec()) {
                let elapsed = now.saturating_sub(start);
                if hard.budget.record(elapsed) {
                    emit_budget_warn(hard.budget.warn_count(), elapsed);
                }
            }
            match result {
                Ok(()) => {
                    if key == hardening::DEVICE_KEY_PATH {
                        // The derivation ikm changed (keystored keygen):
                        // drop the cached MAC key so it re-derives lazily.
                        hard.key = None;
                    }
                    proto::encode_status_response_with_nonce(proto::OP_PUT, proto::STATUS_OK, nonce)
                }
                Err(err) => proto::encode_status_response_with_nonce(
                    proto::OP_PUT,
                    proto::status_from_error(err),
                    nonce,
                ),
            }
        }
        Request::Get { key } => {
            if !policy_allows(sender_service_id, proto::OP_GET, key) {
                emit_access_denied(key, sender_service_id);
                return proto::encode_get_response_with_nonce(
                    proto::STATUS_ACCESS_DENIED,
                    &[],
                    nonce,
                );
            }
            match engine.get(key) {
                Ok(value) => {
                    // TASK-0025: a stored value whose envelope fails its
                    // policy class (or whose seq is below the replay-fed
                    // high-water mark) is never served.
                    let mac_key = if hardening::policy_class_for(key) == PolicyClass::Authenticated
                    {
                        envelope_mac_key(engine, &mut hard.key)
                    } else {
                        None
                    };
                    if let Err(err) = hardening::verify_get(key, &value, mac_key, &hard.tracker) {
                        let status = proto::status_from_error(err);
                        emit_envelope_denied(key, status);
                        return proto::encode_get_response_with_nonce(status, &[], nonce);
                    }
                    if value.len() > MAX_INLINE_VALUE_BYTES {
                        proto::encode_get_response_with_nonce(
                            proto::STATUS_VALUE_TOO_LARGE,
                            &[],
                            nonce,
                        )
                    } else {
                        proto::encode_get_response_with_nonce(proto::STATUS_OK, &value, nonce)
                    }
                }
                Err(err) => {
                    proto::encode_get_response_with_nonce(proto::status_from_error(err), &[], nonce)
                }
            }
        }
        Request::Delete { key } => {
            if !policy_allows(sender_service_id, proto::OP_DEL, key) {
                emit_access_denied(key, sender_service_id);
                return proto::encode_status_response_with_nonce(
                    proto::OP_DEL,
                    proto::STATUS_ACCESS_DENIED,
                    nonce,
                );
            }
            match engine.delete(key) {
                Ok(()) => {
                    proto::encode_status_response_with_nonce(proto::OP_DEL, proto::STATUS_OK, nonce)
                }
                Err(err) => proto::encode_status_response_with_nonce(
                    proto::OP_DEL,
                    proto::status_from_error(err),
                    nonce,
                ),
            }
        }
        Request::List { prefix, limit } => {
            if !policy_allows(sender_service_id, proto::OP_LIST, prefix) {
                emit_access_denied(prefix, sender_service_id);
                return proto::encode_list_response_with_nonce(
                    proto::STATUS_ACCESS_DENIED,
                    &[],
                    MAX_LIST_RESPONSE_BYTES,
                    nonce,
                );
            }
            match engine.list(prefix, limit as usize) {
                Ok(keys) => proto::encode_list_response_with_nonce(
                    proto::STATUS_OK,
                    &keys,
                    MAX_LIST_RESPONSE_BYTES,
                    nonce,
                ),
                Err(err) => proto::encode_list_response_with_nonce(
                    proto::status_from_error(err),
                    &[],
                    MAX_LIST_RESPONSE_BYTES,
                    nonce,
                ),
            }
        }
        Request::Sync => {
            // Sync is a durability boundary for all writers. Allow if the caller has either:
            // - boot authority (`statefs.boot`) or
            // - generic state writer (`statefs.write`)
            let allowed = policyd_allows(sender_service_id, CAP_BOOT.as_bytes())
                || policyd_allows(sender_service_id, CAP_WRITE.as_bytes());
            if !allowed {
                emit_access_denied("/state", sender_service_id);
                return proto::encode_status_response_with_nonce(
                    proto::OP_SYNC,
                    proto::STATUS_ACCESS_DENIED,
                    nonce,
                );
            }
            match engine.sync() {
                Ok(()) => proto::encode_status_response_with_nonce(
                    proto::OP_SYNC,
                    proto::STATUS_OK,
                    nonce,
                ),
                Err(err) => proto::encode_status_response_with_nonce(
                    proto::OP_SYNC,
                    proto::status_from_error(err),
                    nonce,
                ),
            }
        }
        Request::Reopen => {
            let allowed = policyd_allows(sender_service_id, CAP_BOOT.as_bytes())
                || policyd_allows(sender_service_id, CAP_WRITE.as_bytes());
            if !allowed {
                emit_access_denied("/state", sender_service_id);
                return proto::encode_status_response_with_nonce(
                    proto::OP_REOPEN,
                    proto::STATUS_ACCESS_DENIED,
                    nonce,
                );
            }
            match engine.reopen() {
                Ok(()) => proto::encode_status_response_with_nonce(
                    proto::OP_REOPEN,
                    proto::STATUS_OK,
                    nonce,
                ),
                Err(err) => proto::encode_status_response_with_nonce(
                    proto::OP_REOPEN,
                    proto::status_from_error(err),
                    nonce,
                ),
            }
        }
    }
}

fn policy_allows(sender_service_id: u64, op: u8, path: &str) -> bool {
    let cap = required_cap(op, path);
    let selftest_sid = nexus_abi::service_id_from_name(b"selftest-client");
    let metricsd_sid = nexus_abi::service_id_from_name(b"metricsd");
    policyd_allows(
        crate::canonical_policy_subject_for_statefs(
            sender_service_id,
            op,
            path,
            selftest_sid,
            metricsd_sid,
        ),
        cap.as_bytes(),
    )
}

fn required_cap(op: u8, path: &str) -> &'static str {
    if path.starts_with("/state/keystore/") {
        CAP_KEYSTORE
    } else if path.starts_with("/state/boot/") {
        CAP_BOOT
    } else if matches!(op, proto::OP_PUT | proto::OP_DEL | proto::OP_SYNC | proto::OP_REOPEN) {
        CAP_WRITE
    } else {
        CAP_READ
    }
}

pub(crate) fn policyd_allows(subject_id: u64, cap: &[u8]) -> bool {
    // RFC-0066: the shared CAP_MOVE policy check (nexus_ipc::policyd::check_cap_on)
    // over statefsd's init-wired policyd slots (send=7, @reply recv=5/send=6) —
    // behaviour-preserving; the ~90-line hand-rolled copy was removed.
    matches!(
        nexus_ipc::policyd::check_cap_on(0x07, 0x06, 0x05, subject_id, cap),
        nexus_ipc::policyd::CapDecision::Allow
    )
}

// Emit/audit helpers live in `crate::emit_os` (moved verbatim for the
// structure ratchet; behavior unchanged).
