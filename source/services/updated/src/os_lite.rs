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
//! ADR: docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

use core::fmt;

use nexus_abi::{debug_putc, ipc_send_v1, nsec, yield_, MsgHeader, IPC_SYS_NONBLOCK};
use nexus_ipc::{Client as _, KernelClient, KernelServer, Wait};

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
    notifier.notify();
    emit_line("updated: ready (non-persistent)");
    let server = match KernelServer::new_for("updated") {
        Ok(server) => server,
        Err(err) => {
            emit_line(match err {
                nexus_ipc::IpcError::Timeout => "updated: route err timeout",
                nexus_ipc::IpcError::NoSpace => "updated: route err nospace",
                nexus_ipc::IpcError::WouldBlock => "updated: route err wouldblock",
                nexus_ipc::IpcError::Disconnected => "updated: route err disconnected",
                nexus_ipc::IpcError::Unsupported => "updated: route err unsupported",
                nexus_ipc::IpcError::Kernel(_) => "updated: route err kernel",
                _ => "updated: route err other",
            });
            emit_line("updated: route fallback");
            KernelServer::new_with_slots(3, 4).map_err(|_| ServerError::Unsupported)?
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
    let mut probe_emitted = false;
    let mut state = UpdatedState::new();
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
                let rsp = handle_frame(&mut state, frame);
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
            Err(_) => {
                emit_line("updated: recv err");
                let _ = yield_();
            }
        }
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

fn handle_frame(state: &mut UpdatedState, frame: &[u8]) -> Vec<u8> {
    let op = match nexus_abi::updated::decode_request_op(frame) {
        Some(op) => op,
        None => return rsp(OP_STAGE, STATUS_MALFORMED, &[]),
    };

    match op {
        OP_STAGE => handle_stage(state, frame),
        OP_SWITCH => handle_switch(state, frame),
        OP_HEALTH_OK => handle_health_ok(state, frame),
        OP_GET_STATUS => handle_get_status(state),
        OP_BOOT_ATTEMPT => handle_boot_attempt(state, frame),
        OP_LOG_PROBE => {
            emit_line("updated: log probe");
            rsp(OP_LOG_PROBE, STATUS_OK, &[])
        }
        _ => rsp(op, STATUS_UNSUPPORTED, &[]),
    }
}

fn handle_stage(state: &mut UpdatedState, frame: &[u8]) -> Vec<u8> {
    let payload = match nexus_abi::updated::decode_stage_req(frame) {
        Some(bytes) => bytes,
        None => return rsp(OP_STAGE, STATUS_MALFORMED, &[]),
    };

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

fn handle_switch(state: &mut UpdatedState, frame: &[u8]) -> Vec<u8> {
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

fn handle_health_ok(state: &mut UpdatedState, frame: &[u8]) -> Vec<u8> {
    if !nexus_abi::updated::decode_health_ok_req(frame) {
        return rsp(OP_HEALTH_OK, STATUS_MALFORMED, &[]);
    }
    match state.boot.commit_health() {
        Ok(()) => {
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

fn handle_boot_attempt(state: &mut UpdatedState, frame: &[u8]) -> Vec<u8> {
    if !nexus_abi::updated::decode_boot_attempt_req(frame) {
        return rsp(OP_BOOT_ATTEMPT, STATUS_MALFORMED, &[]);
    }
    match state.boot.tick_boot_attempt() {
        Ok(Some(slot)) => {
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
    let client = match KernelClient::new_for("bundlemgrd") {
        Ok(client) => client,
        Err(_) => return Err("route"),
    };
    let reply = KernelClient::new_for("@reply").map_err(|_| "reply-route")?;
    let (reply_send_slot, reply_recv_slot) = reply.slots();
    if reply_send_slot == 0 || reply_recv_slot == 0 {
        return Err("reply-zero");
    }
    let (bnd_send_slot, _bnd_recv_slot) = client.slots();
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
    loop {
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
    }
}

fn encode_slot(slot: Slot) -> u8 {
    match slot {
        Slot::A => 1,
        Slot::B => 2,
    }
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
    let client = KernelClient::new_for("keystored").map_err(|_| VerifyError::Backend("route"))?;
    let mut frame = Vec::with_capacity(4 + 4 + 32 + 64 + message.len());
    frame.push(KEYSTORE_MAGIC0);
    frame.push(KEYSTORE_MAGIC1);
    frame.push(KEYSTORE_VERSION);
    frame.push(KEYSTORE_OP_VERIFY);
    frame.extend_from_slice(&(message.len() as u32).to_le_bytes());
    frame.extend_from_slice(public_key);
    frame.extend_from_slice(signature);
    frame.extend_from_slice(message);
    // Avoid kernel deadline-based blocking IPC here; use explicit nsec()-bounded NONBLOCK retry.
    // This keeps bring-up deterministic/bounded even if deadline semantics are flaky.
    {
        let start_ns = nexus_abi::nsec().map_err(|_| VerifyError::Backend("nsec"))?;
        let deadline_ns = start_ns.saturating_add(1_000_000_000); // 1s
        let mut i: usize = 0;
        loop {
            match client.send(&frame, Wait::NonBlocking) {
                Ok(()) => break,
                Err(nexus_ipc::IpcError::WouldBlock) => {
                    if (i & 0x7f) == 0 {
                        let now = nexus_abi::nsec().map_err(|_| VerifyError::Backend("nsec"))?;
                        if now >= deadline_ns {
                            return Err(VerifyError::Backend("send-timeout"));
                        }
                    }
                    let _ = yield_();
                }
                Err(_) => return Err(VerifyError::Backend("send")),
            }
            i = i.wrapping_add(1);
        }
    }
    for _ in 0..512 {
        match client.recv(Wait::NonBlocking) {
            Ok(rsp) => {
                if rsp.len() < 7 {
                    continue;
                }
                if rsp[0] != KEYSTORE_MAGIC0
                    || rsp[1] != KEYSTORE_MAGIC1
                    || rsp[2] != KEYSTORE_VERSION
                {
                    continue;
                }
                if rsp[3] != (KEYSTORE_OP_VERIFY | 0x80) {
                    continue;
                }
                if rsp[4] != KEYSTORE_STATUS_OK {
                    return Err(VerifyError::Backend("status"));
                }
                let len = u16::from_le_bytes([rsp[5], rsp[6]]) as usize;
                if rsp.len() < 7 + len || len != 1 {
                    return Err(VerifyError::Backend("payload"));
                }
                return Ok(rsp[7] == 1);
            }
            Err(nexus_ipc::IpcError::WouldBlock) => {
                let _ = yield_();
            }
            Err(_) => return Err(VerifyError::Backend("recv")),
        }
    }
    Err(VerifyError::Backend("timeout"))
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
