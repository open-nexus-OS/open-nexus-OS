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

use crate::machine::{BootCtrl, BootTarget, Slot};
use crate::record::{self, BOOT_RECORD_KEY};
use crate::reply::{
    commit, commit_marked, deny, encode_payload, encode_status, machine_fail, policy_allows,
};
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
pub(crate) const REPLY_RECV_SLOT: u32 = 0x05;
pub(crate) const REPLY_SEND_SLOT: u32 = 0x06;
const STATEFS_SEND_SLOT: u32 = 0x07;
pub(crate) const POLICYD_SEND_SLOT: u32 = 0x08;

/// The loaded record + its statefs wire (present once the lazy attach ran).
pub(crate) struct Authority {
    pub(crate) boot: BootCtrl,
    pub(crate) client: StatefsClient,
    /// The graph THIS session runs on: the one-shot target bootctld handed
    /// to init with the boot-attempt ack (the record clears it, so the
    /// persistent field cannot answer "are we in recovery right now").
    session_graph: BootTarget,
    /// TASK-0036-B: blockproto client on the `bsb` partition (None = the
    /// attach failed; the record stays authoritative, projection is off
    /// and was announced loudly).
    pub(crate) bsb_dev: Option<storage::remote_blk::RemoteBlockDevice>,
    /// seq of the last known-good on-disk projection (GET_STATUS surface).
    pub(crate) bsb_seq: u64,
    pub(crate) bsb_synced: bool,
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
    let mut quorum_sids = [0u64; QUORUM_REPORTERS.len()];
    for (slot, name) in quorum_sids.iter_mut().zip(QUORUM_REPORTERS) {
        *slot = nexus_abi::service_id_from_name(name);
    }

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

        // TASK-0053: mutating ops may carry an inline 136-byte .nxra token
        // after the arg byte ([B,T,1,op,arg,token…] = 141 bytes).
        let mut inbuf = [0u8; 192];
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
                    Gates { sid_updated, sid_init_lite, sid_init, sid_selftest, quorum_sids },
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

/// Health-commit v2 quorum window (RFC-0089 §13): armed at switch as
/// `now + WINDOW`. Generous for QEMU/TCG — the OTA phase confirms within
/// seconds; expiry semantics are host-proven with injected clocks.
const COMMIT_DEADLINE_WINDOW_NS: u64 = 120_000_000_000;

/// Declared quorum reporter set (RFC-0089 §13) — data, single authority:
/// bootctld owns its reporter registry; bit = index, identities are
/// kernel-attributed sender ids resolved at bring-up. u8 mask ⇒ max 8.
/// v1 set: `updated` (the OTA facade's pass-through report — init's
/// health signal arrives through it) + `selftest-client` (the proof
/// reporter, confirming directly so the quorum is real, not a mask of 1).
const QUORUM_REPORTERS: &[&[u8]] = &[b"updated", b"selftest-client"];

#[derive(Clone, Copy)]
struct Gates {
    sid_updated: u64,
    sid_init_lite: u64,
    sid_init: u64,
    sid_selftest: u64,
    /// Kernel-attributed sender ids of `QUORUM_REPORTERS` (index = bit).
    quorum_sids: [u64; QUORUM_REPORTERS.len()],
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
    /// Quorum membership: the sender's declared reporter bit, or None for
    /// anyone outside the set (deny — RFC-0089 §13 unknown reporters).
    fn quorum_bit(&self, sender: u64) -> Option<u8> {
        self.quorum_sids.iter().position(|&sid| sid == sender).map(|idx| 1u8 << idx)
    }
    /// The complete quorum mask commit requires.
    fn quorum_full_mask(&self) -> u8 {
        ((1u16 << QUORUM_REPORTERS.len()) - 1) as u8
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
            // TASK-0036-B: additive tail — [4] = projection synced flag,
            // [5..13] = last projected BSB seq (LE). Old clients read the
            // first 4 bytes unchanged.
            let mut payload = [0u8; 13];
            payload[0] = record::encode_slot(auth.boot.active_slot());
            payload[1] = auth.boot.pending_slot().map(record::encode_slot).unwrap_or(0);
            payload[2] = auth.boot.tries_left();
            payload[3] = if auth.boot.health_ok() { 1 } else { 0 };
            payload[4] = u8::from(auth.bsb_synced);
            payload[5..13].copy_from_slice(&auth.bsb_seq.to_le_bytes());
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
            let Some(&tries) = frame.get(4) else {
                return encode_status(rsp, op, wire::STATUS_MALFORMED);
            };
            // Standing gate first; a valid .nxra token is the additive
            // break-glass path (RFC-0088 — never weakens updated's path).
            if !gates.ota_allowed(sender)
                && crate::nxra_gate::authorize(
                    &auth.client,
                    frame,
                    nxra::Action::SlotSwitch,
                    tries as u64,
                ) != crate::nxra_gate::Gate::Authorized
            {
                return deny(rsp, op, sender);
            }
            let snapshot = auth.boot.clone();
            // Health-commit v2 (RFC-0089 §13): arm the quorum deadline with
            // the switch — absolute wall clock, computed here so the
            // machine stays pure (host tests inject arbitrary clocks).
            let deadline_ns =
                nexus_abi::nsec().unwrap_or(0).saturating_add(COMMIT_DEADLINE_WINDOW_NS);
            match auth.boot.switch(tries, deadline_ns) {
                Ok(slot) => {
                    let payload = [record::encode_slot(slot)];
                    let marker = match slot {
                        Slot::A => "bootctld: switch scheduled (to=a)",
                        Slot::B => "bootctld: switch scheduled (to=b)",
                    };
                    let len = commit_marked(auth, snapshot, rsp, op, &payload, Some(marker));
                    if rsp[4] == wire::STATUS_OK {
                        emit("bootctld: commit deadline armed");
                    }
                    len
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
            // Health-commit v2 (RFC-0089 §13): the sender must be a DECLARED
            // quorum reporter (kernel-attributed identity → bit); anyone
            // else is denied. Commit fires exactly when the mask completes.
            let Some(bit) = gates.quorum_bit(sender) else {
                return deny(rsp, op, sender);
            };
            let snapshot = auth.boot.clone();
            match auth.boot.report_health(bit, gates.quorum_full_mask()) {
                Ok(progress) => {
                    let payload = [progress.have, progress.need, u8::from(progress.complete)];
                    let len = commit(auth, snapshot, rsp, op, &payload);
                    if progress.complete && rsp[4] == wire::STATUS_OK {
                        emit_quorum_ok(progress.have, progress.need);
                    }
                    len
                }
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
            match auth.boot.tick_boot_attempt(nexus_abi::nsec().unwrap_or(0)) {
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
            let raw = frame.get(4).copied();
            let kind = match raw {
                Some(0) => nexus_abi::ResetKind::Reboot,
                Some(1) => nexus_abi::ResetKind::Poweroff,
                _ => return encode_status(rsp, op, wire::STATUS_MALFORMED),
            };
            // Defense in depth: kernel-attributed sender gate AND the
            // delegated `boot.reset` capability (deny-by-default); a valid
            // .nxra token is the additive break-glass path (RFC-0088).
            if (!gates.reset_allowed(sender) || !policy_allows(sender, b"boot.reset"))
                && crate::nxra_gate::authorize(
                    &auth.client,
                    frame,
                    nxra::Action::Reset,
                    raw.unwrap_or(0) as u64,
                ) != crate::nxra_gate::Gate::Authorized
            {
                return deny(rsp, op, sender);
            }
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
            let Some(raw) = frame.get(4).copied() else {
                return encode_status(rsp, op, wire::STATUS_MALFORMED);
            };
            let Ok(target) = record::decode_target(raw) else {
                return encode_status(rsp, op, wire::STATUS_MALFORMED);
            };
            if !policy_allows(sender, b"boot.target")
                && crate::nxra_gate::authorize(
                    &auth.client,
                    frame,
                    nxra::Action::TargetSet,
                    raw as u64,
                ) != crate::nxra_gate::Gate::Authorized
            {
                return deny(rsp, op, sender);
            }
            let snapshot = auth.boot.clone();
            auth.boot.set_next_boot(target);
            commit(auth, snapshot, rsp, op, &[])
        }
        wire::OP_SET_TARGET => {
            let Some(raw) = frame.get(4).copied() else {
                return encode_status(rsp, op, wire::STATUS_MALFORMED);
            };
            let Ok(target) = record::decode_target(raw) else {
                return encode_status(rsp, op, wire::STATUS_MALFORMED);
            };
            if !policy_allows(sender, b"boot.target")
                && crate::nxra_gate::authorize(
                    &auth.client,
                    frame,
                    nxra::Action::TargetSet,
                    raw as u64,
                ) != crate::nxra_gate::Gate::Authorized
            {
                return deny(rsp, op, sender);
            }
            let snapshot = auth.boot.clone();
            auth.boot.set_boot_target(target);
            commit(auth, snapshot, rsp, op, &[])
        }
        _ => encode_status(rsp, op, wire::STATUS_UNSUPPORTED),
    }
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
    // TASK-0036-B: attach the bsb projection client and reconcile the
    // on-disk pair against the loaded record (bsb_os.rs).
    let (bsb_dev, bsb_seq, bsb_synced) = crate::bsb_os::attach_and_reconcile(&boot);
    Some(Authority { boot, client: statefs, session_graph, bsb_dev, bsb_seq, bsb_synced })
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

pub(crate) fn emit(message: &str) {
    let _ = nexus_abi::debug_println(message);
}

/// `bootctld: health quorum ok (n/n)` — bounded formatting (n ≤ 8).
fn emit_quorum_ok(have: u8, need: u8) {
    let mut line = [0u8; 40];
    let head = b"bootctld: health quorum ok (";
    let mut len = head.len();
    line[..len].copy_from_slice(head);
    line[len] = b'0' + have.min(8);
    line[len + 1] = b'/';
    line[len + 2] = b'0' + need.min(8);
    line[len + 3] = b')';
    len += 4;
    if let Ok(msg) = core::str::from_utf8(&line[..len]) {
        emit(msg);
    }
}
