#![cfg(all(nexus_env = "os", feature = "os-lite"))]
// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: updated os-lite backend (system-set staging + A/B control)
//! OWNERS: @services-team
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//!
//! ADR: docs/adr/0024-updates-ab-packaging-architecture.md

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

use core::fmt;

use nexus_abi::{debug_putc, ipc_send_v1, nsec, yield_, MsgHeader, IPC_SYS_NONBLOCK};
use nexus_ipc::{Client as _, KernelClient, KernelServer, Wait};
use statefs::client::StatefsClient;
use statefs::StatefsError;

use updates::{
    BootCtrl, BootCtrlError, SignatureVerifier, Slot, SystemSet, SystemSetError, VerifyError,
};

const MAGIC0: u8 = nexus_abi::updated::MAGIC0;
const MAGIC1: u8 = nexus_abi::updated::MAGIC1;
const VERSION: u8 = nexus_abi::updated::VERSION;

const OP_STAGE: u8 = nexus_abi::updated::OP_STAGE;
const OP_SWITCH: u8 = nexus_abi::updated::OP_SWITCH;
const OP_HEALTH_OK: u8 = nexus_abi::updated::OP_HEALTH_OK;
const OP_GET_STATUS: u8 = nexus_abi::updated::OP_GET_STATUS;
const OP_BOOT_ATTEMPT: u8 = nexus_abi::updated::OP_BOOT_ATTEMPT;
const OP_LOG_PROBE: u8 = 0x7f;

const STATUS_OK: u8 = nexus_abi::updated::STATUS_OK;
const STATUS_MALFORMED: u8 = nexus_abi::updated::STATUS_MALFORMED;
const STATUS_UNSUPPORTED: u8 = nexus_abi::updated::STATUS_UNSUPPORTED;
const STATUS_FAILED: u8 = nexus_abi::updated::STATUS_FAILED;

const MAX_STAGE_FRAME: usize = nexus_abi::updated::MAX_STAGE_BYTES + 8;

const KEYSTORE_MAGIC0: u8 = b'K';
const KEYSTORE_MAGIC1: u8 = b'S';
const KEYSTORE_VERSION: u8 = 1;
const KEYSTORE_OP_VERIFY: u8 = 4;
const KEYSTORE_STATUS_OK: u8 = 0;

const BOOTCTRL_STATE_KEY: &str = "/state/boot/bootctl.v1";
const BOOTCTRL_STATE_VERSION: u8 = 1;
const SLOT_NONE: u8 = 0xff;

/// Result alias used by the os-lite backend.
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

/// Errors surfaced by the os-lite backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerError {
    /// Functionality not yet available in the os-lite path.
    Unsupported,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "updated unsupported"),
        }
    }
}

struct UpdatedState {
    boot: BootCtrl,
    staged: Option<Vec<u8>>,
    staged_slot: Option<Slot>,
}

impl UpdatedState {
    fn new() -> Self {
        Self { boot: BootCtrl::new(Slot::A), staged: None, staged_slot: None }
    }
}

/// Touches schema types to keep host parity; no-op in the stub.
pub fn touch_schemas() {}

/// Main service loop used by the lite backend.
pub fn service_main_loop(notifier: ReadyNotifier) -> LiteResult<()> {
    emit_line("updated: entry");
    notifier.notify();
    // init-lite transfers the updated service endpoints into deterministic slots:
    // - recv: slot 3
    // - send: slot 4
    //
    // Using these directly avoids routing-time races during early bring-up.
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
    let (recv_slot, send_slot) = server.slots();
    emit_bytes(b"updated: slots ");
    emit_hex_u8((recv_slot >> 8) as u8);
    emit_hex_u8(recv_slot as u8);
    emit_byte(b' ');
    emit_hex_u8((send_slot >> 8) as u8);
    emit_hex_u8(send_slot as u8);
    emit_byte(b'\n');
    let (recv_slot, _) = server.slots();
    let mut statefs = None;
    emit_line("updated: statefs init");
    // init-lite distributes a per-service statefsd SEND cap plus a per-service reply inbox
    // for CAP_MOVE. Prefer these deterministic slots during early bring-up to avoid routing races.
    const STATEFS_SEND_SLOT: u32 = 0x09;
    const REPLY_RECV_SLOT: u32 = 0x0a;
    const REPLY_SEND_SLOT: u32 = 0x0b;
    if let Ok(client) = KernelClient::new_with_slots(STATEFS_SEND_SLOT, REPLY_RECV_SLOT) {
        let reply = KernelClient::new_with_slots(REPLY_SEND_SLOT, REPLY_RECV_SLOT).ok();
        statefs = Some(StatefsClient::from_clients(client, reply));
        emit_line("updated: statefs slot fallback");
    }
    if statefs.is_some() {
        emit_line("updated: statefs available");
    } else {
        emit_line("updated: statefs unavailable");
    }
    let mut probe_emitted = false;
    let mut state = UpdatedState::new();
    let mut logged_recv_err = false;
    if let Some(client) = statefs.as_mut() {
        match load_bootctrl_state(client) {
            Ok(boot) => {
                state.boot = boot;
                emit_line("updated: bootctl load ok");
            }
            Err(StatefsError::NotFound) => emit_line("updated: bootctl load miss"),
            Err(_) => emit_line("updated: bootctl load err"),
        }
    }
    emit_line(if statefs.is_some() {
        "updated: ready (statefs)"
    } else {
        "updated: ready (non-persistent)"
    });
    let mut recv_buf = Vec::with_capacity(MAX_STAGE_FRAME);
    recv_buf.resize(MAX_STAGE_FRAME, 0);
    let mut logged_rx = false;
    loop {
        match recv_request_large(recv_slot, Wait::Blocking, &mut recv_buf) {
            Ok((frame_len, reply_cap)) => {
                let frame = &recv_buf[..frame_len];
                if !logged_rx {
                    emit_line("updated: rx");
                    if frame.len() >= 3 {
                        emit_bytes(b"updated: rx head ");
                        emit_hex_u8(frame[0]);
                        emit_byte(b' ');
                        emit_hex_u8(frame[1]);
                        emit_byte(b' ');
                        emit_hex_u8(frame[2]);
                        emit_byte(b'\n');
                    }
                    logged_rx = true;
                }
                if frame.len() >= 4
                    && frame[0] == MAGIC0
                    && frame[1] == MAGIC1
                    && frame[2] == VERSION
                {
                    emit_bytes(b"updated: rx op ");
                    emit_hex_u8(frame[3]);
                    emit_byte(b'\n');
                }
                if !probe_emitted {
                    probe_emitted = true;
                    nexus_log::info("updated", |line| {
                        line.text("core service log probe: updated");
                    });
                }
                if reply_cap.is_some() {
                    emit_line("updated: capmove");
                }
                let rsp = handle_frame(&mut state, &mut statefs, frame);
                if let Some(cap) = reply_cap {
                    if rsp.len() >= 4 {
                        emit_bytes(b"updated: tx op ");
                        emit_hex_u8(rsp[3]);
                        emit_byte(b'\n');
                    }
                    // Debug: prove what reply-cap slot the kernel returned (CAP_MOVE recv path).
                    // This should match the allocated-slot observed in the kernel trace ring.
                    emit_bytes(b"updated: replycap slot=0x");
                    emit_hex_u8((cap >> 24) as u8);
                    emit_hex_u8((cap >> 16) as u8);
                    emit_hex_u8((cap >> 8) as u8);
                    emit_hex_u8(cap as u8);
                    emit_byte(b'\n');
                    if send_bounded_nonblock(cap, &rsp, 1_000_000_000).is_err() {
                        emit_line("updated: send cap fail");
                    }
                    let _ = nexus_abi::cap_close(cap);
                } else {
                    if send_bounded_nonblock(send_slot, &rsp, 1_000_000_000).is_err() {
                        emit_line("updated: send fail");
                    }
                }
            }
            Err(nexus_abi::IpcError::QueueEmpty) | Err(nexus_abi::IpcError::TimedOut) => {
                let _ = yield_();
            }
            Err(err) => {
                if !logged_recv_err {
                    emit_bytes(b"updated: recv err kernel=");
                    emit_line(ipc_error_label(err));
                    logged_recv_err = true;
                }
                let _ = yield_();
            }
        }
    }
}

fn ipc_error_label(err: nexus_abi::IpcError) -> &'static str {
    match err {
        nexus_abi::IpcError::TimedOut => "TimedOut",
        nexus_abi::IpcError::QueueEmpty => "QueueEmpty",
        nexus_abi::IpcError::QueueFull => "QueueFull",
        nexus_abi::IpcError::NoSpace => "NoSpace",
        nexus_abi::IpcError::NoSuchEndpoint => "NoSuchEndpoint",
        nexus_abi::IpcError::PermissionDenied => "PermissionDenied",
        nexus_abi::IpcError::Unsupported => "Unsupported",
    }
}

/// Best-effort bounded IPC send that never blocks indefinitely.
///
/// Uses explicit `nsec()` timeouts and `yield_()` to avoid deadlocks under cooperative scheduling.
fn send_bounded_nonblock(slot: u32, frame: &[u8], budget_ns: u64) -> Result<(), ()> {
    let hdr = MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
    let start = nsec().map_err(|_| ())?;
    let deadline = start.saturating_add(budget_ns);
    let mut i: usize = 0;
    loop {
        match ipc_send_v1(slot, &hdr, frame, IPC_SYS_NONBLOCK, 0) {
            Ok(_) => return Ok(()),
            Err(nexus_abi::IpcError::QueueFull) => {
                if (i & 0x7f) == 0 {
                    let now = nsec().map_err(|_| ())?;
                    if now >= deadline {
                        return Err(());
                    }
                }
                let _ = yield_();
            }
            Err(_) => return Err(()),
        }
        i = i.wrapping_add(1);
    }
}

fn recv_request_large(
    recv_slot: u32,
    wait: Wait,
    buf: &mut [u8],
) -> Result<(usize, Option<u32>), nexus_abi::IpcError> {
    let (flags, deadline_ns) = wait_to_sys(wait).unwrap_or((0, 0));
    let sys_flags = flags | nexus_abi::IPC_SYS_TRUNCATE;
    let mut hdr = MsgHeader::new(0, 0, 0, 0, 0);
    let n = nexus_abi::ipc_recv_v1(recv_slot, &mut hdr, buf, sys_flags, deadline_ns)?;
    let reply_cap =
        if (hdr.flags & nexus_abi::ipc_hdr::CAP_MOVE) != 0 { Some(hdr.src) } else { None };
    Ok((n as usize, reply_cap))
}

fn wait_to_sys(wait: Wait) -> Option<(u32, u64)> {
    match wait {
        Wait::NonBlocking => Some((nexus_abi::IPC_SYS_NONBLOCK, 0)),
        Wait::Blocking => Some((0, 0)),
        Wait::Timeout(duration) => {
            let now = nexus_abi::nsec().ok()?;
            Some((0, now.saturating_add(duration_to_ns(duration))))
        }
    }
}

fn duration_to_ns(duration: core::time::Duration) -> u64 {
    duration.as_secs().saturating_mul(1_000_000_000).saturating_add(duration.subsec_nanos() as u64)
}

fn handle_frame(
    state: &mut UpdatedState,
    statefs: &mut Option<StatefsClient>,
    frame: &[u8],
) -> Vec<u8> {
    let op = match nexus_abi::updated::decode_request_op(frame) {
        Some(op) => op,
        None => return rsp(OP_STAGE, STATUS_MALFORMED, &[]),
    };

    match op {
        OP_STAGE => handle_stage(state, statefs, frame),
        OP_SWITCH => handle_switch(state, statefs, frame),
        OP_HEALTH_OK => handle_health_ok(state, statefs, frame),
        OP_GET_STATUS => handle_get_status(state),
        OP_BOOT_ATTEMPT => handle_boot_attempt(state, statefs, frame),
        OP_LOG_PROBE => {
            emit_line("updated: log probe");
            rsp(OP_LOG_PROBE, STATUS_OK, &[])
        }
        _ => rsp(op, STATUS_UNSUPPORTED, &[]),
    }
}

fn handle_stage(
    state: &mut UpdatedState,
    statefs: &mut Option<StatefsClient>,
    frame: &[u8],
) -> Vec<u8> {
    let payload = match nexus_abi::updated::decode_stage_req(frame) {
        Some(bytes) => bytes,
        None => return rsp(OP_STAGE, STATUS_MALFORMED, &[]),
    };
    // Phase-2 (RFC-0026): keep stage payloads explicitly within bounded inline frame limits.
    // Larger artifacts must use the existing bulk path contract (not ad-hoc control-plane growth).
    if payload.len().saturating_add(8) > MAX_STAGE_FRAME {
        audit("stage", "fail", Some("oversized-inline"));
        return rsp(OP_STAGE, STATUS_MALFORMED, &[]);
    }

    let verifier = KeystoredVerifier;
    // Cooperative-yield throttling: this must yield often enough to avoid starving other
    // services/selftests under the cooperative scheduler (QEMU smoke determinism).
    let mut yield_ticks: u32 = 0;
    match SystemSet::parse_with_yield(payload, &verifier, || {
        yield_ticks = yield_ticks.wrapping_add(1);
        // Yield every 8 ticks (tuned for QEMU). Too-infrequent yielding can freeze bring-up.
        if (yield_ticks & 0x7) == 0 {
            let _ = nexus_abi::yield_();
        }
    }) {
        Ok(_) => {
            state.staged = Some(payload.to_vec());
            let slot = state.boot.stage();
            state.staged_slot = Some(slot);
            if let Err(_) = persist_bootctrl_state(&state.boot, statefs) {
                audit("stage", "fail", Some("persist"));
                return rsp(OP_STAGE, STATUS_FAILED, &[]);
            }
            audit("stage", "ok", None);
            rsp(OP_STAGE, STATUS_OK, &[])
        }
        Err(err) => {
            let (detail, marker) = stage_error_detail(&err);
            if let Some(marker) = marker {
                emit_line(marker);
            }
            audit("stage", "fail", Some(detail));
            rsp(OP_STAGE, STATUS_FAILED, &[])
        }
    }
}

fn stage_error_detail(err: &SystemSetError) -> (&'static str, Option<&'static str>) {
    match err {
        SystemSetError::InvalidSignature(_) => {
            ("signature", Some("updated: stage rejected (signature)"))
        }
        SystemSetError::DigestMismatch { .. } => {
            ("digest", Some("updated: stage rejected (digest)"))
        }
        SystemSetError::ArchiveTooLarge { .. } | SystemSetError::OversizedEntry { .. } => {
            ("oversized", None)
        }
        SystemSetError::MissingEntry(_) => ("missing-entry", None),
        SystemSetError::UnexpectedEntry { .. } => ("unexpected-entry", None),
        SystemSetError::ArchiveMalformed(reason) => (*reason, None),
        SystemSetError::InvalidIndex(reason) => (*reason, None),
    }
}

fn handle_switch(
    state: &mut UpdatedState,
    statefs: &mut Option<StatefsClient>,
    frame: &[u8],
) -> Vec<u8> {
    let tries_left = match nexus_abi::updated::decode_switch_req(frame) {
        Some(value) => value,
        None => return rsp(OP_SWITCH, STATUS_MALFORMED, &[]),
    };
    match state.boot.switch(tries_left) {
        Ok(slot) => {
            let slot_result = bundlemgrd_set_active_slot(slot);
            if slot_result.is_ok() {
                state.staged = None;
                state.staged_slot = None;
                if let Err(_) = persist_bootctrl_state(&state.boot, statefs) {
                    let _ = state.boot.rollback();
                    let _ = bundlemgrd_set_active_slot(state.boot.active_slot());
                    audit("switch", "fail", Some("persist"));
                    return rsp(OP_SWITCH, STATUS_FAILED, &[]);
                }
                audit(
                    "switch",
                    "ok",
                    Some(match slot {
                        Slot::A => "slot=a",
                        Slot::B => "slot=b",
                    }),
                );
                rsp(OP_SWITCH, STATUS_OK, &[])
            } else {
                let _ = state.boot.rollback();
                let reason = slot_result.err().unwrap_or("bundlemgrd");
                audit("switch", "fail", Some(reason));
                rsp(OP_SWITCH, STATUS_FAILED, &[])
            }
        }
        Err(err) => {
            audit(
                "switch",
                "fail",
                Some(match err {
                    BootCtrlError::NotStaged => "not-staged",
                    BootCtrlError::AlreadyPending => "already-pending",
                    BootCtrlError::NotPending => "not-pending",
                    BootCtrlError::NoRollbackTarget => "no-rollback",
                }),
            );
            rsp(OP_SWITCH, STATUS_FAILED, &[])
        }
    }
}

fn handle_health_ok(
    state: &mut UpdatedState,
    statefs: &mut Option<StatefsClient>,
    frame: &[u8],
) -> Vec<u8> {
    if !nexus_abi::updated::decode_health_ok_req(frame) {
        return rsp(OP_HEALTH_OK, STATUS_MALFORMED, &[]);
    }
    match state.boot.commit_health() {
        Ok(()) => {
            if let Err(_) = persist_bootctrl_state(&state.boot, statefs) {
                audit("health_ok", "fail", Some("persist"));
                return rsp(OP_HEALTH_OK, STATUS_FAILED, &[]);
            }
            audit("health_ok", "ok", None);
            rsp(OP_HEALTH_OK, STATUS_OK, &[])
        }
        Err(_) => {
            audit("health_ok", "fail", Some("not-pending"));
            rsp(OP_HEALTH_OK, STATUS_FAILED, &[])
        }
    }
}

fn handle_get_status(state: &UpdatedState) -> Vec<u8> {
    let (active, pending) = (
        encode_slot(state.boot.active_slot()),
        state.boot.pending_slot().map(encode_slot).unwrap_or(0),
    );
    let tries_left = state.boot.tries_left();
    let health_ok = if state.boot.health_ok() { 1 } else { 0 };
    let mut payload = [0u8; 4];
    payload[0] = active;
    payload[1] = pending;
    payload[2] = tries_left;
    payload[3] = health_ok;
    rsp(OP_GET_STATUS, STATUS_OK, &payload)
}

fn handle_boot_attempt(
    state: &mut UpdatedState,
    statefs: &mut Option<StatefsClient>,
    frame: &[u8],
) -> Vec<u8> {
    if !nexus_abi::updated::decode_boot_attempt_req(frame) {
        return rsp(OP_BOOT_ATTEMPT, STATUS_MALFORMED, &[]);
    }
    match state.boot.tick_boot_attempt() {
        Ok(Some(slot)) => {
            if let Err(_) = persist_bootctrl_state(&state.boot, statefs) {
                audit("boot_attempt", "fail", Some("persist"));
                return rsp(OP_BOOT_ATTEMPT, STATUS_FAILED, &[]);
            }
            audit(
                "boot_attempt",
                "rollback",
                Some(match slot {
                    Slot::A => "slot=a",
                    Slot::B => "slot=b",
                }),
            );
            rsp(OP_BOOT_ATTEMPT, STATUS_OK, &[encode_slot(slot)])
        }
        Ok(None) => {
            if let Err(_) = persist_bootctrl_state(&state.boot, statefs) {
                audit("boot_attempt", "fail", Some("persist"));
                return rsp(OP_BOOT_ATTEMPT, STATUS_FAILED, &[]);
            }
            audit("boot_attempt", "ok", None);
            rsp(OP_BOOT_ATTEMPT, STATUS_OK, &[0])
        }
        Err(_) => {
            audit("boot_attempt", "fail", Some("no-rollback"));
            rsp(OP_BOOT_ATTEMPT, STATUS_FAILED, &[])
        }
    }
}

fn bundlemgrd_set_active_slot(slot: Slot) -> Result<(), &'static str> {
    // Note: bundlemgrd's default response endpoint is currently wired for selftest-client.
    // For updated -> bundlemgrd, we must use CAP_MOVE so bundlemgrd can reply on the moved cap.
    //
    // init-lite deterministic slots for updated (from slot_map.rs):
    // - bundlemgrd send cap: 0x05
    // - bundlemgrd recv cap: 0x06 (unused, we use reply inbox)
    // - reply inbox: recv=0x0A, send=0x0B
    const BND_SEND_SLOT: u32 = 0x05;
    const REPLY_RECV_SLOT: u32 = 0x0A;
    const REPLY_SEND_SLOT: u32 = 0x0B;

    let bnd_send_slot = BND_SEND_SLOT;
    let reply_send_slot = REPLY_SEND_SLOT;
    let reply_recv_slot = REPLY_RECV_SLOT;
    let slot_id = match slot {
        Slot::A => 1,
        Slot::B => 2,
    };
    let mut frame = [0u8; 5];
    nexus_abi::bundlemgrd::encode_set_active_slot_req(slot_id, &mut frame);
    let wait = Wait::Timeout(core::time::Duration::from_secs(1));
    let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).map_err(|_| "reply-clone")?;
    let (sys_flags, deadline_ns) = wait_to_sys(wait).ok_or("send-wait")?;
    let hdr =
        MsgHeader::new(reply_send_clone, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, frame.len() as u32);
    if nexus_abi::ipc_send_v1(bnd_send_slot, &hdr, &frame, sys_flags, deadline_ns).is_err() {
        let _ = nexus_abi::cap_close(reply_send_clone);
        return Err("send");
    }
    // Wait for reply on the local reply inbox.
    let now = nexus_abi::nsec().map_err(|_| "reply-time")?;
    let deadline = now.saturating_add(1_000_000_000);
    let mut hdr = MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 32];
    let mut spins: usize = 0;
    loop {
        if (spins & 0x7f) == 0 {
            let now = nexus_abi::nsec().map_err(|_| "reply-time")?;
            if now >= deadline {
                return Err("reply-timeout");
            }
        }
        match nexus_abi::ipc_recv_v1(
            reply_recv_slot,
            &mut hdr,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                if let Some((status, _slot)) =
                    nexus_abi::bundlemgrd::decode_set_active_slot_rsp(&buf[..n])
                {
                    return if status == nexus_abi::bundlemgrd::STATUS_OK {
                        Ok(())
                    } else {
                        Err("status")
                    };
                }
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let now = nexus_abi::nsec().map_err(|_| "reply-time")?;
                if now >= deadline {
                    return Err("reply-timeout");
                }
                let _ = yield_();
            }
            Err(err) => {
                return Err(match err {
                    nexus_abi::IpcError::TimedOut => "reply-timeout",
                    nexus_abi::IpcError::NoSuchEndpoint => "reply-nosuch",
                    nexus_abi::IpcError::PermissionDenied => "reply-denied",
                    nexus_abi::IpcError::QueueFull => "reply-full",
                    nexus_abi::IpcError::NoSpace => "reply-nospace",
                    nexus_abi::IpcError::Unsupported => "reply-unsupported",
                    nexus_abi::IpcError::QueueEmpty => "reply-empty",
                })
            }
        }
        spins = spins.wrapping_add(1);
    }
}

fn encode_slot(slot: Slot) -> u8 {
    match slot {
        Slot::A => 1,
        Slot::B => 2,
    }
}

fn decode_slot(byte: u8) -> Result<Option<Slot>, StatefsError> {
    match byte {
        1 => Ok(Some(Slot::A)),
        2 => Ok(Some(Slot::B)),
        SLOT_NONE => Ok(None),
        _ => Err(StatefsError::Corrupted),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BootCtrlState {
    active: Slot,
    pending: Option<Slot>,
    staged: Option<Slot>,
    tries_left: u8,
    health_ok: bool,
}

impl BootCtrlState {
    fn from_bootctrl(boot: &BootCtrl) -> Self {
        Self {
            active: boot.active_slot(),
            pending: boot.pending_slot(),
            staged: boot.staged_slot(),
            tries_left: boot.tries_left(),
            health_ok: boot.health_ok(),
        }
    }
}

fn encode_bootctrl_state(boot: &BootCtrl) -> [u8; 6] {
    let state = BootCtrlState::from_bootctrl(boot);
    [
        BOOTCTRL_STATE_VERSION,
        encode_slot(state.active),
        state.pending.map(encode_slot).unwrap_or(SLOT_NONE),
        state.staged.map(encode_slot).unwrap_or(SLOT_NONE),
        state.tries_left,
        if state.health_ok { 1 } else { 0 },
    ]
}

fn decode_bootctrl_state(bytes: &[u8]) -> Result<BootCtrlState, StatefsError> {
    if bytes.len() != 6 || bytes[0] != BOOTCTRL_STATE_VERSION {
        return Err(StatefsError::Corrupted);
    }
    let active = decode_slot(bytes[1])?.ok_or(StatefsError::Corrupted)?;
    let pending = decode_slot(bytes[2])?;
    let staged = decode_slot(bytes[3])?;
    let tries_left = bytes[4];
    let health_ok = bytes[5] == 1;
    Ok(BootCtrlState { active, pending, staged, tries_left, health_ok })
}

fn bootctrl_from_state(state: BootCtrlState) -> Result<BootCtrl, BootCtrlError> {
    if state.pending.is_some() {
        let base = state.active.other();
        let mut boot = BootCtrl::new(base);
        boot.stage();
        let _ = boot.switch(state.tries_left)?;
        return Ok(boot);
    }
    if state.health_ok {
        let base = state.active.other();
        let mut boot = BootCtrl::new(base);
        boot.stage();
        let _ = boot.switch(0)?;
        let _ = boot.commit_health()?;
        return Ok(boot);
    }
    let mut boot = BootCtrl::new(state.active);
    if state.staged.is_some() {
        boot.stage();
    }
    Ok(boot)
}

fn load_bootctrl_state(client: &mut StatefsClient) -> Result<BootCtrl, StatefsError> {
    let bytes = client.get(BOOTCTRL_STATE_KEY)?;
    let state = decode_bootctrl_state(&bytes)?;
    bootctrl_from_state(state).map_err(|_| StatefsError::Corrupted)
}

fn persist_bootctrl_state(
    boot: &BootCtrl,
    statefs: &mut Option<StatefsClient>,
) -> Result<(), StatefsError> {
    let client = match statefs.as_mut() {
        Some(client) => client,
        None => return Ok(()),
    };
    let payload = encode_bootctrl_state(boot);
    // #region agent log (persist failure detail; rate-limited)
    static PERSIST_ERR_LOGGED: core::sync::atomic::AtomicBool =
        core::sync::atomic::AtomicBool::new(false);
    let label = |e: StatefsError| -> &'static str {
        match e {
            StatefsError::NotFound => "NotFound",
            StatefsError::AccessDenied => "AccessDenied",
            StatefsError::ValueTooLarge => "ValueTooLarge",
            StatefsError::KeyTooLong => "KeyTooLong",
            StatefsError::IoError => "IoError",
            StatefsError::Corrupted => "Corrupted",
            StatefsError::InvalidKey => "InvalidKey",
            StatefsError::ReplayLimitExceeded => "ReplayLimitExceeded",
        }
    };
    if let Err(e) = client.put(BOOTCTRL_STATE_KEY, &payload) {
        if !PERSIST_ERR_LOGGED.swap(true, core::sync::atomic::Ordering::Relaxed) {
            emit_bytes(b"updated: bootctl persist put err=");
            emit_line(label(e));
        }
        return Err(e);
    }
    if let Err(e) = client.sync() {
        if !PERSIST_ERR_LOGGED.swap(true, core::sync::atomic::Ordering::Relaxed) {
            emit_bytes(b"updated: bootctl persist sync err=");
            emit_line(label(e));
        }
        return Err(e);
    }
    // #endregion agent log
    Ok(())
}

struct KeystoredVerifier;

impl SignatureVerifier for KeystoredVerifier {
    fn verify_ed25519(
        &self,
        public_key: &[u8; 32],
        message: &[u8],
        signature: &[u8; 64],
    ) -> Result<(), VerifyError> {
        match keystored_verify(public_key, message, signature) {
            Ok(true) => Ok(()),
            Ok(false) => local_verify(public_key, message, signature),
            Err(VerifyError::Backend(_)) => local_verify(public_key, message, signature),
            Err(err) => Err(err),
        }
    }
}

fn local_verify(
    public_key: &[u8; 32],
    message: &[u8],
    signature: &[u8; 64],
) -> Result<(), VerifyError> {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    let key = VerifyingKey::from_bytes(public_key).map_err(|_| VerifyError::InvalidKey)?;
    let sig = Signature::from_bytes(signature);
    key.verify(message, &sig).map_err(|_| VerifyError::InvalidSignature)
}

fn keystored_verify(
    public_key: &[u8; 32],
    message: &[u8],
    signature: &[u8; 64],
) -> Result<bool, VerifyError> {
    // init-lite deterministic slots for updated -> keystored:
    // - send=0x07, recv=0x08
    let client =
        KernelClient::new_with_slots(0x07, 0x08).map_err(|_| VerifyError::Backend("route"))?;
    let mut frame = Vec::with_capacity(4 + 4 + 32 + 64 + message.len());
    frame.push(KEYSTORE_MAGIC0);
    frame.push(KEYSTORE_MAGIC1);
    frame.push(KEYSTORE_VERSION);
    frame.push(KEYSTORE_OP_VERIFY);
    frame.extend_from_slice(&(message.len() as u32).to_le_bytes());
    frame.extend_from_slice(public_key);
    frame.extend_from_slice(signature);
    frame.extend_from_slice(message);
    let clock = nexus_ipc::budget::OsClock;
    let deadline_ns = nexus_ipc::budget::deadline_after(&clock, core::time::Duration::from_secs(1))
        .map_err(|_| VerifyError::Backend("nsec"))?;
    nexus_ipc::budget::send_until(&clock, &client, &frame, deadline_ns)
        .map_err(|_| VerifyError::Backend("send-timeout"))?;

    let rsp = nexus_ipc::budget::retry_ipc_until(&clock, deadline_ns, || {
        match client.recv(Wait::NonBlocking) {
            Ok(v) => {
                if v.len() < 7
                    || v[0] != KEYSTORE_MAGIC0
                    || v[1] != KEYSTORE_MAGIC1
                    || v[2] != KEYSTORE_VERSION
                    || v[3] != (KEYSTORE_OP_VERIFY | 0x80)
                {
                    return Err(nexus_ipc::IpcError::WouldBlock);
                }
                Ok(v)
            }
            Err(e) => Err(e),
        }
    })
    .map_err(|err| match err {
        nexus_ipc::IpcError::Timeout => VerifyError::Backend("timeout"),
        _ => VerifyError::Backend("recv"),
    })?;

    if rsp[4] != KEYSTORE_STATUS_OK {
        return Err(VerifyError::Backend("status"));
    }
    let len = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
    if rsp.len() < 7 + len || len != 1 {
        return Err(VerifyError::Backend("payload"));
    }
    Ok(rsp[7] == 1)
}

fn rsp(op: u8, status: u8, payload: &[u8]) -> Vec<u8> {
    // Response: [MAGIC0, MAGIC1, VER, op|0x80, status, len:u16le, payload...]
    let mut out = Vec::with_capacity(7 + payload.len());
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(op | 0x80);
    out.push(status);
    let len: u16 = (payload.len().min(u16::MAX as usize)) as u16;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&payload[..len as usize]);
    out
}

fn audit(op: &str, status: &str, detail: Option<&str>) {
    nexus_log::info("updated", |line| {
        line.text("audit");
        line.text(" op=");
        line.text(op);
        line.text(" status=");
        line.text(status);
        if let Some(detail) = detail {
            line.text(" detail=");
            line.text(detail);
        }
    });
    emit_audit_marker(op, status, detail);
}

fn emit_audit_marker(op: &str, status: &str, detail: Option<&str>) {
    emit_bytes(b"updated: audit (op=");
    emit_bytes(op.as_bytes());
    emit_bytes(b" status=");
    emit_bytes(status.as_bytes());
    if let Some(detail) = detail {
        emit_bytes(b" detail=");
        emit_bytes(detail.as_bytes());
    }
    emit_bytes(b")");
    emit_byte(b'\n');
}

fn emit_byte(byte: u8) {
    let _ = debug_putc(byte);
}

fn emit_bytes(bytes: &[u8]) {
    for &b in bytes {
        emit_byte(b);
    }
}

fn emit_line(message: &str) {
    emit_bytes(message.as_bytes());
    emit_byte(b'\n');
}

fn emit_hex_u8(value: u8) {
    let hi = (value >> 4) & 0x0f;
    let lo = value & 0x0f;
    let hi_ch = if hi < 10 { b'0' + hi } else { b'a' + (hi - 10) };
    let lo_ch = if lo < 10 { b'0' + lo } else { b'a' + (lo - 10) };
    emit_byte(hi_ch);
    emit_byte(lo_ch);
}
