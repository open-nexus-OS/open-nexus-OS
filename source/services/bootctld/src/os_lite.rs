// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]
#![forbid(unsafe_code)]

//! CONTEXT: bootctld os-lite backend (TASK-0050 PR-2 — the single WRITER).
//! Owns the boot record end to end: loads it at bring-up (v2/v1/legacy —
//! the codec migrates), announces the target truth
//! (`bootctld: target=<t> next=<t|none>`), and serves the full wire:
//! reads for anyone, mutations sender-gated on the kernel-attributed id
//! (OTA ops: `updated` only; boot-attempt: `updated` or init) with
//! deterministic `STATUS_DENIED` — the deny path has no forgeable probe
//! surface. Every mutation is both-or-neither: snapshot → mutate →
//! persist (relocated read-modify-write discipline, `persist_os`); a
//! failed persist restores the snapshot, so RAM and disk can never tell
//! different stories (the old updated writer mutated first and persisted
//! second, leaving RAM ahead of disk on failure).
//! OWNERS: @reliability @runtime
//! STATUS: Experimental (bring-up)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU OTA ladder (markers unchanged, relocated
//!   authority) + bringup markers; machine/record proofs host-side.
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md

extern crate alloc;

use alloc::boxed::Box;

use core::fmt;
use core::time::Duration;

use nexus_abi::yield_;
use nexus_ipc::{KernelClient, KernelServer, Server as _, Wait};
use statefs::client::StatefsClient;
use statefs::StatefsError;

use crate::machine::{BootCtrl, BootCtrlError, BootTarget, Slot};
use crate::persist_os::persist_record;
use crate::record::{self, BOOT_RECORD_KEY};
use crate::wire;

/// Result alias surfaced by the lite backend.
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
    /// Endpoint binding failed permanently.
    Unsupported,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "bootctld unsupported"),
        }
    }
}

/// Schema warmer placeholder for API parity.
pub fn touch_schemas() {}

/// init-lite deterministic slots (bespoke wiring — see the wiring arm):
/// reply inbox recv/send + the statefsd request SEND clone. Fixed on
/// purpose: the record load must not depend on the responder (init calls
/// the boot-attempt handshake before the responder serves).
const REPLY_RECV_SLOT: u32 = 0x05;
const REPLY_SEND_SLOT: u32 = 0x06;
const STATEFS_SEND_SLOT: u32 = 0x07;
const POLICYD_SEND_SLOT: u32 = 0x08;

/// The loaded record + its statefs wire (present once the lazy attach ran).
struct Authority {
    boot: BootCtrl,
    client: StatefsClient,
    /// The graph THIS session runs on: the one-shot target bootctld handed
    /// to init with the boot-attempt ack (the record clears it, so the
    /// persistent field cannot answer "are we in recovery right now").
    session_graph: BootTarget,
}

/// Main bootctld service loop (os-lite).
pub fn service_main_loop(notifier: ReadyNotifier) -> LiteResult<()> {
    nexus_abi::service_verdict_arm();
    let server = match KernelServer::new_for("bootctld") {
        Ok(server) => server,
        Err(_) => KernelServer::new_with_slots(3, 4).map_err(|_| ServerError::Unsupported)?,
    };
    notifier.notify();
    emit("bootctld: ready");
    nexus_abi::service_verdict_flush("bootctld");

    // Eager record load: bounded retries against the fixed wired slots —
    // statefsd is long up (bootctld spawns last), and init's boot-attempt
    // handshake arrives right after bring-up.

    // Kernel-attributed caller identities for the mutation gates.
    let sid_updated = nexus_abi::service_id_from_name(b"updated");
    let sid_init_lite = nexus_abi::service_id_from_name(b"init-lite");
    let sid_init = nexus_abi::service_id_from_name(b"nexus-init");
    let sid_selftest = nexus_abi::service_id_from_name(b"selftest-client");

    let mut authority: Option<Authority> = None;
    let mut load_attempts: u8 = 0;

    let mut breaker = nexus_ipc::resilience::CircuitBreaker::new(64, 3);
    loop {
        if authority.is_none() && load_attempts < 8 {
            load_attempts += 1;
            if let Some(loaded) = try_attach() {
                announce_target(&loaded.boot);
                authority = Some(loaded);
            } else if load_attempts == 8 {
                emit("bootctld: record unavailable (defaults)");
            }
        }

        let mut inbuf = [0u8; 64];
        match server
            .recv_request_with_meta_into(Wait::Timeout(Duration::from_millis(1000)), &mut inbuf)
        {
            Ok((n, sender, reply)) => {
                breaker.on_success();
                // A mutation may arrive before the idle loop attached (init's
                // boot-attempt lands right after bring-up): attach inline,
                // bounded — the caller is waiting synchronously and the wait
                // chain is acyclic (statefsd/policyd never wait on bootctld).
                if authority.is_none() && load_attempts < 8 {
                    load_attempts += 1;
                    if let Some(loaded) = try_attach() {
                        announce_target(&loaded.boot);
                        authority = Some(loaded);
                    }
                }
                let frame = &inbuf[..n];
                let mut rsp = [0u8; 32];
                let len = handle_frame(
                    authority.as_mut(),
                    Gates { sid_updated, sid_init_lite, sid_init, sid_selftest },
                    sender,
                    frame,
                    &mut rsp,
                );
                if let Some(reply) = reply {
                    if reply.reply_and_close(&rsp[..len]).is_err() {
                        emit("bootctld: reply send fail");
                    }
                } else if server.send(&rsp[..len], Wait::NonBlocking).is_err() {
                    emit("bootctld: rsp send fail (dropping)");
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(_) => {
                let (should_log, verdict) = breaker.on_error();
                if should_log {
                    emit("bootctld: transient ipc error (continuing)");
                }
                match verdict {
                    nexus_ipc::resilience::BreakerVerdict::Continue => {
                        let _ = yield_();
                    }
                    nexus_ipc::resilience::BreakerVerdict::EndpointDefect => {
                        emit("bootctld: endpoint defect (consecutive error limit)");
                        return Err(ServerError::Unsupported);
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Gates {
    sid_updated: u64,
    sid_init_lite: u64,
    sid_init: u64,
    sid_selftest: u64,
}

impl Gates {
    /// OTA slot mutations: only the update fassade.
    fn ota_allowed(&self, sender: u64) -> bool {
        sender == self.sid_updated
    }
    /// Boot-attempt tick: the update fassade or init itself.
    fn attempt_allowed(&self, sender: u64) -> bool {
        sender == self.sid_updated || sender == self.sid_init_lite || sender == self.sid_init
    }
    /// System reset: init (escalation, PR-5) or the proof harness. PR-4
    /// upgrades this to the policyd `boot.reset` capability alongside the
    /// target ops; the sender gate stays as defense in depth.
    fn reset_allowed(&self, sender: u64) -> bool {
        sender == self.sid_init_lite || sender == self.sid_init || sender == self.sid_selftest
    }
}

/// One request → one bounded response; returns the response length.
fn handle_frame(
    authority: Option<&mut Authority>,
    gates: Gates,
    sender: u64,
    frame: &[u8],
    rsp: &mut [u8; 32],
) -> usize {
    let op = frame.get(3).copied().unwrap_or(0);
    if frame.len() < 4
        || frame[0] != wire::MAGIC0
        || frame[1] != wire::MAGIC1
        || frame[2] != wire::VERSION
    {
        return encode_status(rsp, op, wire::STATUS_MALFORMED);
    }
    let Some(auth) = authority else {
        // Record not loaded yet: honest FAILED, never fabricated state.
        return encode_status(rsp, op, wire::STATUS_FAILED);
    };
    match op {
        wire::OP_GET_STATUS => {
            let payload = [
                record::encode_slot(auth.boot.active_slot()),
                auth.boot.pending_slot().map(record::encode_slot).unwrap_or(0),
                auth.boot.tries_left(),
                if auth.boot.health_ok() { 1 } else { 0 },
            ];
            encode_payload(rsp, op, &payload)
        }
        wire::OP_GET_RECORD => {
            let payload = record::encode_record(&auth.boot);
            encode_payload(rsp, op, &payload)
        }
        wire::OP_GET_TARGET => {
            let payload = [
                record::encode_target(auth.boot.boot_target()),
                auth.boot.next_boot().map(record::encode_target).unwrap_or(wire::TARGET_NONE),
            ];
            encode_payload(rsp, op, &payload)
        }
        wire::OP_STAGE => {
            // TASK-0051: slot mutations are blocked while the PERSISTENT
            // target is recovery — checked BEFORE the sender gate so the
            // block is provable from the recovery boot itself (commits
            // belong to the normal boot path; RFC-0087 §4).
            if auth.session_graph == BootTarget::Recovery {
                emit("bootctld: commit blocked (target=recovery)");
                return encode_status(rsp, op, wire::STATUS_COMMIT_BLOCKED);
            }
            if !gates.ota_allowed(sender) {
                return deny(rsp, op, sender);
            }
            let snapshot = auth.boot.clone();
            let _slot = auth.boot.stage();
            commit(auth, snapshot, rsp, op, &[])
        }
        wire::OP_SWITCH => {
            // TASK-0051: slot mutations are blocked while the PERSISTENT
            // target is recovery — checked BEFORE the sender gate so the
            // block is provable from the recovery boot itself (commits
            // belong to the normal boot path; RFC-0087 §4).
            if auth.session_graph == BootTarget::Recovery {
                emit("bootctld: commit blocked (target=recovery)");
                return encode_status(rsp, op, wire::STATUS_COMMIT_BLOCKED);
            }
            if !gates.ota_allowed(sender) {
                return deny(rsp, op, sender);
            }
            let Some(&tries) = frame.get(4) else {
                return encode_status(rsp, op, wire::STATUS_MALFORMED);
            };
            let snapshot = auth.boot.clone();
            match auth.boot.switch(tries) {
                Ok(slot) => {
                    let payload = [record::encode_slot(slot)];
                    let marker = match slot {
                        Slot::A => "bootctld: switch scheduled (to=a)",
                        Slot::B => "bootctld: switch scheduled (to=b)",
                    };
                    commit_marked(auth, snapshot, rsp, op, &payload, Some(marker))
                }
                Err(err) => machine_fail(rsp, op, err),
            }
        }
        wire::OP_HEALTH_OK => {
            // TASK-0051: slot mutations are blocked while the PERSISTENT
            // target is recovery — checked BEFORE the sender gate so the
            // block is provable from the recovery boot itself (commits
            // belong to the normal boot path; RFC-0087 §4).
            if auth.session_graph == BootTarget::Recovery {
                emit("bootctld: commit blocked (target=recovery)");
                return encode_status(rsp, op, wire::STATUS_COMMIT_BLOCKED);
            }
            if !gates.ota_allowed(sender) {
                return deny(rsp, op, sender);
            }
            let snapshot = auth.boot.clone();
            match auth.boot.commit_health() {
                Ok(()) => commit(auth, snapshot, rsp, op, &[]),
                Err(err) => machine_fail(rsp, op, err),
            }
        }
        wire::OP_ROLLBACK => {
            // TASK-0051: slot mutations are blocked while the PERSISTENT
            // target is recovery — checked BEFORE the sender gate so the
            // block is provable from the recovery boot itself (commits
            // belong to the normal boot path; RFC-0087 §4).
            if auth.session_graph == BootTarget::Recovery {
                emit("bootctld: commit blocked (target=recovery)");
                return encode_status(rsp, op, wire::STATUS_COMMIT_BLOCKED);
            }
            if !gates.ota_allowed(sender) {
                return deny(rsp, op, sender);
            }
            let snapshot = auth.boot.clone();
            match auth.boot.rollback() {
                Ok(slot) => {
                    let payload = [record::encode_slot(slot)];
                    commit(auth, snapshot, rsp, op, &payload)
                }
                Err(err) => machine_fail(rsp, op, err),
            }
        }
        wire::OP_BOOT_ATTEMPT => {
            if !gates.attempt_allowed(sender) {
                return deny(rsp, op, sender);
            }
            let snapshot = auth.boot.clone();
            match auth.boot.tick_boot_attempt() {
                Ok(rolled_back) => {
                    // One-shot next_boot rides the SAME persisted commit as
                    // the attempt ack (RFC-0087 §4): consumed here, cleared
                    // on disk atomically with the tick.
                    let next = auth.boot.take_next_boot();
                    if let Some(target) = next {
                        auth.session_graph = target;
                    }
                    let payload = [
                        rolled_back.map(record::encode_slot).unwrap_or(0),
                        next.map(record::encode_target).unwrap_or(wire::TARGET_NONE),
                    ];
                    commit(auth, snapshot, rsp, op, &payload)
                }
                Err(err) => machine_fail(rsp, op, err),
            }
        }
        wire::OP_RESET => {
            // Defense in depth: kernel-attributed sender gate AND the
            // delegated `boot.reset` capability (deny-by-default).
            if !gates.reset_allowed(sender) || !policy_allows(sender, b"boot.reset") {
                return deny(rsp, op, sender);
            }
            let kind = match frame.get(4).copied() {
                Some(0) => nexus_abi::ResetKind::Reboot,
                Some(1) => nexus_abi::ResetKind::Poweroff,
                _ => return encode_status(rsp, op, wire::STATUS_MALFORMED),
            };
            emit(match kind {
                nexus_abi::ResetKind::Reboot => "bootctld: reset (reboot)",
                nexus_abi::ResetKind::Poweroff => "bootctld: reset (poweroff)",
            });
            // Does not return on success — the machine restarts; the caller
            // never sees a reply (its bounded wait dies with the boot).
            let _ = nexus_abi::system_reset(kind);
            emit("bootctld: reset refused");
            encode_status(rsp, op, wire::STATUS_FAILED)
        }
        wire::OP_SET_NEXT_BOOT => {
            if !policy_allows(sender, b"boot.target") {
                return deny(rsp, op, sender);
            }
            let Some(target) = frame.get(4).copied().and_then(|b| record::decode_target(b).ok())
            else {
                return encode_status(rsp, op, wire::STATUS_MALFORMED);
            };
            let snapshot = auth.boot.clone();
            auth.boot.set_next_boot(target);
            commit(auth, snapshot, rsp, op, &[])
        }
        wire::OP_SET_TARGET => {
            if !policy_allows(sender, b"boot.target") {
                return deny(rsp, op, sender);
            }
            let Some(target) = frame.get(4).copied().and_then(|b| record::decode_target(b).ok())
            else {
                return encode_status(rsp, op, wire::STATUS_MALFORMED);
            };
            let snapshot = auth.boot.clone();
            auth.boot.set_boot_target(target);
            commit(auth, snapshot, rsp, op, &[])
        }
        _ => encode_status(rsp, op, wire::STATUS_UNSUPPORTED),
    }
}

/// Persist-or-restore: the record on disk and the machine in RAM commit
/// together or not at all.
fn commit(
    auth: &mut Authority,
    snapshot: BootCtrl,
    rsp: &mut [u8; 32],
    op: u8,
    payload: &[u8],
) -> usize {
    commit_marked(auth, snapshot, rsp, op, payload, None)
}

/// `commit` that emits `marker` ONLY after the record persisted — a
/// scheduled-switch claim before the disk commit would be fake green.
fn commit_marked(
    auth: &mut Authority,
    snapshot: BootCtrl,
    rsp: &mut [u8; 32],
    op: u8,
    payload: &[u8],
    marker: Option<&str>,
) -> usize {
    match persist_record(&auth.client, &auth.boot) {
        Ok(()) => {
            if let Some(line) = marker {
                emit(line);
            }
            encode_payload(rsp, op, payload)
        }
        Err(_) => {
            auth.boot = snapshot;
            encode_status(rsp, op, wire::STATUS_FAILED)
        }
    }
}

fn machine_fail(rsp: &mut [u8; 32], op: u8, err: BootCtrlError) -> usize {
    // Deterministic machine rejects map to FAILED with the reason byte so
    // the client's audit detail stays truthful.
    let reason = match err {
        BootCtrlError::NotStaged => 1,
        BootCtrlError::AlreadyPending => 2,
        BootCtrlError::NotPending => 3,
        BootCtrlError::NoRollbackTarget => 4,
    };
    let base = encode_status(rsp, op, wire::STATUS_FAILED);
    rsp[5..7].copy_from_slice(&1u16.to_le_bytes());
    rsp[base] = reason;
    base + 1
}

/// Delegated capability check (deny-by-default; Unreachable = deny).
fn policy_allows(sender: u64, cap: &[u8]) -> bool {
    matches!(
        nexus_ipc::policyd::check_cap_on(
            POLICYD_SEND_SLOT,
            REPLY_SEND_SLOT,
            REPLY_RECV_SLOT,
            sender,
            cap,
        ),
        nexus_ipc::policyd::CapDecision::Allow
    )
}

fn deny(rsp: &mut [u8; 32], op: u8, sender: u64) -> usize {
    emit_deny(op, sender);
    encode_status(rsp, op, wire::STATUS_DENIED)
}

fn encode_status(rsp: &mut [u8; 32], op: u8, status: u8) -> usize {
    rsp[0] = wire::MAGIC0;
    rsp[1] = wire::MAGIC1;
    rsp[2] = wire::VERSION;
    rsp[3] = op | 0x80;
    rsp[4] = status;
    rsp[5] = 0;
    rsp[6] = 0;
    7
}

fn encode_payload(rsp: &mut [u8; 32], op: u8, payload: &[u8]) -> usize {
    let base = encode_status(rsp, op, wire::STATUS_OK);
    let len = payload.len().min(rsp.len() - base);
    rsp[5..7].copy_from_slice(&(len as u16).to_le_bytes());
    rsp[base..base + len].copy_from_slice(&payload[..len]);
    base + len
}

/// Load the record via the shared statefs client over the FIXED wired
/// slots (@reply inbox — never the shared response queue, never the
/// responder). `NotFound` = fresh image (defaults); corrupt = LOUD +
/// defaults (fatal in proof boots via the harness guard); wire trouble =
/// retry (bounded by the caller).
fn try_attach() -> Option<Authority> {
    let client = KernelClient::new_with_slots(STATEFS_SEND_SLOT, REPLY_RECV_SLOT).ok()?;
    let reply = KernelClient::new_with_slots(REPLY_SEND_SLOT, REPLY_RECV_SLOT).ok();
    let statefs = StatefsClient::from_clients(client, reply);
    let boot = match statefs.get(BOOT_RECORD_KEY) {
        Ok(bytes) => match record::open_record(&bytes) {
            Ok((boot, _seq)) => boot,
            Err(_) => {
                emit("bootctld: record corrupt (defaults)");
                BootCtrl::new(Slot::A)
            }
        },
        Err(StatefsError::NotFound) => BootCtrl::new(Slot::A),
        Err(_) => return None,
    };
    // Until the boot-attempt consumes a one-shot, this session's graph is
    // whatever the armed next_boot says (init WILL consume it) falling
    // back to the persistent target.
    let session_graph = boot.next_boot().unwrap_or(boot.boot_target());
    Some(Authority { boot, client: statefs, session_graph })
}

fn announce_target(boot: &BootCtrl) {
    let target = target_label(boot.boot_target());
    let next = boot.next_boot().map(target_label).unwrap_or("none");
    let mut line = [0u8; 48];
    let mut len = 0usize;
    for part in ["bootctld: target=", target, " next=", next] {
        let bytes = part.as_bytes();
        if len + bytes.len() > line.len() {
            return;
        }
        line[len..len + bytes.len()].copy_from_slice(bytes);
        len += bytes.len();
    }
    if let Ok(msg) = core::str::from_utf8(&line[..len]) {
        emit(msg);
    }
}

fn target_label(target: BootTarget) -> &'static str {
    match target {
        BootTarget::Normal => "normal",
        BootTarget::Recovery => "recovery",
        BootTarget::Safe => "safe",
    }
}

fn emit_deny(op: u8, sender: u64) {
    let mut line = [0u8; 64];
    let mut len = 0usize;
    let head = b"bootctld: denied op=0x";
    line[..head.len()].copy_from_slice(head);
    len += head.len();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    line[len] = HEX[(op >> 4) as usize];
    line[len + 1] = HEX[(op & 0xf) as usize];
    len += 2;
    let mid = b" sender=0x";
    line[len..len + mid.len()].copy_from_slice(mid);
    len += mid.len();
    for shift in (0..16).rev() {
        line[len] = HEX[((sender >> (shift * 4)) & 0xf) as usize];
        len += 1;
    }
    if let Ok(msg) = core::str::from_utf8(&line[..len]) {
        emit(msg);
    }
}

fn emit(message: &str) {
    let _ = nexus_abi::debug_println(message);
}
