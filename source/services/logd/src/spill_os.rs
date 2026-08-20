// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OS-side evidence spill (TASK-0049C, RFC-0087 §5) — the I/O half
//! of `spill.rs`. Lazily attaches to statefsd on the FIRST evidence-class
//! append (logd starts before statefsd; an eager boot-time attach would
//! race it), loading the on-disk ring into a RAM mirror (`persisted`
//! queries answer from the mirror — the query handler never does IPC while
//! it owes a response). Each spill rides ONE journal-v2 transaction
//! (slot PUT + head PUT, both-or-neither). Attach retries are bounded;
//! exhaustion degrades LOUD once (`logd: degrade evidence volatile`) and
//! evidence stays RAM-only — RFC-0087's own discipline applied to logd.
//! Loop safety: the spill PUT triggers a policyd check whose ALLOW audit
//! lands back here; `evidence::classify` keeps allow-audits RAM-only, so
//! the chain terminates (see evidence.rs).
//! OWNERS: @runtime @reliability
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: pure halves host-tested (`tests/evidence_spill.rs`);
//!   this wiring is QEMU-proven (`logd: evidence persist on`, cold-boot
//!   lane `loaded=0x<n>` > 0, `SELFTEST: evidence query ok`,
//!   `SELFTEST: evidence budget ok`).
//! ADR: docs/rfcs/RFC-0087-reliability-failure-model-v1.md (§5)

use core::time::Duration;

use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};
use nexus_ipc::KernelClient;
use statefs::client::StatefsClient;
use statefs::StatefsError;

use crate::evidence::{classify, EvidenceClass};
use crate::journal::{InlineBytes, Journal, LogLevel, LogRecord, RecordId, TimestampNsec};
use crate::spill::{decode_slot_value, SpillEngine, SpilledRecord, HEAD_KEY, SLOT_KEY_PREFIX};

/// init-lite control-channel slots (route requests via the responder).
const CTRL_SEND_SLOT: u32 = 1;
const CTRL_RECV_SLOT: u32 = 2;
/// Bounded lazy-attach retries before the loud RAM-only degrade.
const MAX_ATTACH_ATTEMPTS: u8 = 8;
/// Mirror bounds (mirrors the on-disk ring: 32 records, ~16 KiB).
const MIRROR_CAP_RECORDS: u32 = 32;
const MIRROR_CAP_BYTES: u32 = 16 * 1024;

/// Bounded backlog of evidence records awaiting their between-requests
/// spill tick (drop-oldest; evidence is rare by design).
const PENDING_CAP: usize = 8;
/// Consecutive txn failures before the spill degrades TERMINALLY — endless
/// retries are exactly the unbounded-recovery class RFC-0087 forbids, and
/// each failed spill births a policyd deny audit that would re-trigger it.
const MAX_CONSECUTIVE_FAILS: u8 = 4;

/// Evidence spill state carried by the serve loop.
pub(crate) struct SpillState {
    engine: Option<SpillEngine>,
    client: Option<StatefsClient>,
    mirror: Journal,
    attach_attempts: u8,
    degraded: bool,
    consecutive_fails: u8,
    fail_latch: bool,
    overflow_latch: bool,
    pending: [Option<LogRecord>; PENDING_CAP],
    pending_len: usize,
    load: Option<MirrorLoad>,
    /// The apparatus ARMS on the first crash/exhaust event. Audit records
    /// alone never arm it: a preserved-image boot replays statefsd audits
    /// within seconds of statefsd's ready line, and attaching/spilling in
    /// that window shifted early-boot timing enough to park the boot
    /// (deterministic keep-blk hang at `windowd: ready`). Crash/exhaust
    /// events cannot occur in that window; pre-arm audits ride the pending
    /// buffer and spill once armed.
    armed: bool,
}

/// Incremental boot-load of the on-disk ring: ONE slot GET per quiet tick.
/// Loading all 32 slots eagerly at attach put ~33 statefs round trips (each
/// with a policyd check) into the early-boot window on preserved-image
/// boots — enough timing shift to park the boot (the eager variant hung
/// keep-blk boots deterministically at `windowd: ready`).
struct MirrorLoad {
    head: u64,
    first_seq: u64,
    next_slot: u64,
    staged: [Option<SpilledRecord>; 32],
}

impl SpillState {
    pub(crate) fn new() -> Self {
        Self {
            engine: None,
            client: None,
            mirror: Journal::new(MIRROR_CAP_RECORDS, MIRROR_CAP_BYTES),
            attach_attempts: 0,
            degraded: false,
            consecutive_fails: 0,
            fail_latch: false,
            overflow_latch: false,
            pending: Default::default(),
            pending_len: 0,
            load: None,
            armed: false,
        }
    }

    /// The boot-loaded + live-updated mirror `persisted` queries read.
    pub(crate) fn mirror(&self) -> &Journal {
        &self.mirror
    }

    /// Enqueues one just-appended record IF it is evidence-class. NO IPC
    /// here: this runs inside the request handler while logd still owes the
    /// emitter its APPEND ack — a synchronous spill built a cross-service
    /// convoy (policyd waits on logd's ack while statefsd waits on policyd
    /// for the spill's own cap checks) whose latency turned unrelated
    /// policy checks into fail-closed denials. The serve loop drains the
    /// backlog between requests via [`Self::tick`].
    pub(crate) fn maybe_spill(
        &mut self,
        sender_service_id: u64,
        now: TimestampNsec,
        level: LogLevel,
        scope: &[u8],
        message: &[u8],
        fields: &[u8],
    ) {
        // Classify on the WIRE payload — the store truncation (128 B
        // fields) may cut the event tag off; the decision must not depend
        // on where the tag landed relative to the cap.
        let Some(class) = classify(scope, message, fields) else {
            return;
        };
        if self.degraded {
            return;
        }
        if matches!(class, EvidenceClass::Crash | EvidenceClass::Exhaust) {
            self.armed = true;
        }
        if self.pending_len == PENDING_CAP {
            // Drop-oldest, LOUD once: evidence is rare — a sustained
            // overflow means an emitter is flooding (its own event class).
            self.pending.rotate_left(1);
            self.pending[PENDING_CAP - 1] = None;
            self.pending_len -= 1;
            if !self.overflow_latch {
                self.overflow_latch = true;
                emit("logd: evidence spill backlog overflow (drop-oldest)");
            }
        }
        // Store representation: disk and RAM tell the same (bounded) story.
        self.pending[self.pending_len] = Some(LogRecord {
            record_id: RecordId(0),
            timestamp_nsec: now,
            level,
            service_id: sender_service_id,
            scope: InlineBytes::new(scope),
            message: InlineBytes::new(message),
            fields: InlineBytes::new(fields),
            size_bytes: 0,
        });
        self.pending_len += 1;
    }

    /// Spills at most ONE pending record — called by the serve loop BETWEEN
    /// requests (the statefsd `compaction_tick` pattern), so spill I/O never
    /// adds latency to a request/response exchange.
    pub(crate) fn tick(&mut self) {
        if self.degraded || !self.armed {
            return;
        }
        if self.pending_len == 0 {
            self.load_step();
            return;
        }
        if !self.ensure_attached() {
            return;
        }
        let (Some(engine), Some(client)) = (self.engine.as_mut(), self.client.as_ref()) else {
            return;
        };
        let Some(record) = self.pending[0].take() else {
            self.pending.rotate_left(1);
            self.pending_len = self.pending_len.saturating_sub(1);
            return;
        };
        let plan = engine.plan(&record);
        match spill_txn(client, plan.slot_key.as_str(), &plan.value, &plan.head_value) {
            Ok(()) => {
                self.pending.rotate_left(1);
                self.pending_len -= 1;
                self.consecutive_fails = 0;
                let _ = self.mirror.append(
                    record.service_id,
                    record.timestamp_nsec,
                    record.level,
                    record.scope.as_slice(),
                    record.message.as_slice(),
                    record.fields.as_slice(),
                );
            }
            Err(_) => {
                engine.rollback_one();
                // Keep the record for the next tick (bounded by the
                // consecutive-fail budget below).
                self.pending[0] = Some(record);
                self.consecutive_fails = self.consecutive_fails.saturating_add(1);
                if !self.fail_latch {
                    self.fail_latch = true;
                    emit("logd: evidence spill fail (txn)");
                }
                if self.consecutive_fails >= MAX_CONSECUTIVE_FAILS {
                    // Terminal (RFC-0087): announce once, stay RAM-only —
                    // each failed spill mints a deny audit which is itself
                    // evidence; retrying forever would self-sustain.
                    self.degraded = true;
                    self.pending = Default::default();
                    self.pending_len = 0;
                    emit("logd: degrade evidence volatile (statefs unavailable)");
                }
            }
        }
    }

    /// Lazy attach: route to statefsd + ONE `head` GET. The mirror load is
    /// planned, not performed — [`Self::load_step`] drains it one slot per
    /// quiet tick so a preserved-image boot never sees a burst.
    fn ensure_attached(&mut self) -> bool {
        if self.engine.is_some() {
            return true;
        }
        if self.degraded || self.attach_attempts >= MAX_ATTACH_ATTEMPTS {
            return false;
        }
        self.attach_attempts += 1;
        match try_attach() {
            Some((engine, client, head)) => {
                self.engine = Some(engine);
                self.client = Some(client);
                if head > 0 {
                    let live = SpillEngine::live_slots(head);
                    self.load = Some(MirrorLoad {
                        head,
                        first_seq: head - live,
                        next_slot: 0,
                        staged: Default::default(),
                    });
                } else {
                    emit_persist_on(0);
                }
                true
            }
            None => {
                if self.attach_attempts >= MAX_ATTACH_ATTEMPTS {
                    self.degraded = true;
                    emit("logd: degrade evidence volatile (statefs unavailable)");
                }
                false
            }
        }
    }

    /// One incremental mirror-load step (single slot GET); on completion,
    /// stages sort into the mirror oldest-first and the honest cross-boot
    /// count is announced (the cold-boot lane gates on `loaded > 0`).
    fn load_step(&mut self) {
        let Some(client) = self.client.as_ref() else { return };
        let Some(load) = self.load.as_mut() else { return };
        let live = SpillEngine::live_slots(load.head);
        if load.next_slot < live {
            let slot = load.next_slot;
            load.next_slot += 1;
            let mut key = [0u8; 40];
            let prefix = SLOT_KEY_PREFIX.as_bytes();
            key[..prefix.len()].copy_from_slice(prefix);
            key[prefix.len()] = b'0' + ((slot / 10) % 10) as u8;
            key[prefix.len() + 1] = b'0' + (slot % 10) as u8;
            let Ok(key) = core::str::from_utf8(&key[..prefix.len() + 2]) else {
                return;
            };
            let Ok(value) = client.get(key) else { return };
            let Some(rec) = decode_slot_value(&value) else { return };
            // Head is the authority: a slot outside the live window is a
            // leftover from an earlier ring generation.
            if rec.seq < load.first_seq || rec.seq >= load.head {
                return;
            }
            let idx = (rec.seq - load.first_seq) as usize;
            if idx < load.staged.len() {
                load.staged[idx] = Some(rec);
            }
            return;
        }
        // All slots probed: commit oldest-first into the mirror. The
        // mirror is RENUMBERED onto synthetic timestamps 1..n: the query
        // wire pages by since_nsec, and last boot's real timestamps are
        // HIGHER than this boot's early ones — keeping them made paging
        // skip every record this boot spills (keep-blk `evidence query
        // FAIL`). Order stays the spill sequence; the original timestamp
        // survives ON DISK in the slot layout for forensic readers
        // (TASK-0051's diag surface reads slots, not the mirror).
        let mut loaded: u32 = 0;
        for rec in load.staged.iter_mut() {
            let Some(rec) = rec.take() else { continue };
            if self
                .mirror
                .append(
                    rec.service_id,
                    TimestampNsec(loaded as u64 + 1),
                    rec.level,
                    &rec.scope,
                    &rec.message,
                    &rec.fields,
                )
                .is_ok()
            {
                loaded = loaded.saturating_add(1);
            }
        }
        self.load = None;
        emit_persist_on(loaded);
    }
}

/// One spill = ONE transaction: slot + head become visible atomically; a
/// torn write is dropped whole by journal-v2 replay (TASK-0026 semantics).
fn spill_txn(
    client: &StatefsClient,
    slot_key: &str,
    value: &[u8],
    head_value: &[u8; 8],
) -> Result<(), StatefsError> {
    let txn_id = client.txn_begin()?;
    let staged = client
        .txn_put(txn_id, slot_key, value)
        .and_then(|()| client.txn_put(txn_id, HEAD_KEY, head_value));
    match staged {
        Ok(()) => client.txn_commit(txn_id),
        Err(err) => {
            let _ = client.txn_abort(txn_id);
            Err(err)
        }
    }
}

fn try_attach() -> Option<(SpillEngine, StatefsClient, u64)> {
    let (state_send, _) = route_blocking(b"statefsd")?;
    let (reply_send, reply_recv) = route_blocking(b"@reply")?;
    let client = KernelClient::new_with_slots(state_send, reply_recv).ok()?;
    let reply = KernelClient::new_with_slots(reply_send, reply_recv).ok();
    let statefs = StatefsClient::from_clients(client, reply);

    let head = match statefs.get(HEAD_KEY) {
        Ok(bytes) if bytes.len() == 8 => u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]),
        Ok(_) => 0,
        Err(StatefsError::NotFound) => 0,
        // Any wire/policy trouble: attach failed, retry later (bounded).
        Err(_) => return None,
    };
    Some((SpillEngine::new(head), statefs, head))
}

fn route_blocking(name: &[u8]) -> Option<(u32, u32)> {
    match budget::route_with_nonce_budgeted(
        name,
        CTRL_SEND_SLOT,
        CTRL_RECV_SLOT,
        Duration::from_secs(2),
        NonceMismatchBudget::new(64),
    ) {
        RouteRetryOutcome::Success { send_slot, recv_slot } => Some((send_slot, recv_slot)),
        _ => None,
    }
}

fn emit(message: &str) {
    let _ = nexus_abi::debug_println(message);
}

/// `logd: evidence persist on (loaded=0x<n>)` — `n` counts records that
/// came FROM DISK at attach; `n > 0` on a preserved-image boot is the
/// cross-boot persistence truth the cold-boot lane gates on.
fn emit_persist_on(loaded: u32) {
    let mut line = [0u8; 48];
    let mut len = 0usize;
    let text = b"logd: evidence persist on (loaded=0x";
    line[..text.len()].copy_from_slice(text);
    len += text.len();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    line[len] = HEX[((loaded >> 4) & 0xf) as usize];
    line[len + 1] = HEX[(loaded & 0xf) as usize];
    len += 2;
    line[len] = b')';
    len += 1;
    if let Ok(msg) = core::str::from_utf8(&line[..len]) {
        emit(msg);
    }
}
