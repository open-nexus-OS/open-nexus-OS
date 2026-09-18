// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]
//! CONTEXT: SAMGR OS-lite service loop – minimal registry semantics for bring-up and selftests
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU selftest markers (scripts/qemu-test.sh)
//! ADR: docs/adr/0017-service-architecture.md

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use core::fmt;

use nexus_abi::{debug_putc, yield_};
use nexus_ipc::{KernelServer, Server as _, Wait};

/// Result alias surfaced by the lite SAMgr backend.
pub type LiteResult<T> = Result<T, ServerError>;

/// Ready notifier invoked when the service startup finishes.
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
    /// Placeholder error until the real backend lands.
    Unsupported,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "samgrd unsupported"),
        }
    }
}

/// Schema warmer placeholder for API parity.
pub fn touch_schemas() {}

const MAGIC0: u8 = b'S';
const MAGIC1: u8 = b'M';
const VERSION: u8 = 1;

const OP_REGISTER: u8 = 1;
const OP_LOOKUP: u8 = 2;
const OP_PING_CAP_MOVE: u8 = 3;
const OP_SENDER_PID: u8 = 4;
const OP_SENDER_SERVICE_ID: u8 = 5;
const OP_RESOLVE_STATUS: u8 = 6;
const OP_LOG_PROBE: u8 = 0x7f;

const STATUS_OK: u8 = 0;
const STATUS_NOT_FOUND: u8 = 1;
const STATUS_MALFORMED: u8 = 2;
const STATUS_UNSUPPORTED: u8 = 3;
/// Minimal samgrd bring-up service loop.

pub fn service_main_loop(notifier: ReadyNotifier) -> LiteResult<()> {
    notifier.notify();
    let _ = nexus_service_entry::ready("samgrd: ready");
    nexus_abi::service_verdict_flush("samgrd");
    // TASK-0288 sweep: transient errors continue; only a consecutive-error
    // run marks our own endpoint defect (fleet-collapse lesson).
    let mut breaker = nexus_ipc::resilience::CircuitBreaker::new(64, 3);
    emit_line("samgrd: mode os-lite");
    // The declared server pair (TASK-0324 P4), pinned before this task runs. No route ask at
    // start-up (P7-b): an ask has no clock and init may be blocked in a synchronous exchange
    // with a service that, in turn, waits for THIS server — the ask made that a deadlock.
    let slots = nexus_service_topology::slots::samgrd::SERVER;
    let server = KernelServer::new_with_slots(slots.recv, slots.send)
        .map_err(|_| ServerError::Unsupported)?;
    let (recv_slot, send_slot) = server.slots();
    emit_line("samgrd: slots logging");
    emit_bytes(b"samgrd: slots ");
    emit_hex_u32(recv_slot);
    emit_byte(b' ');
    emit_hex_u32(send_slot);
    emit_byte(b'\n');
    // Identity-binding hardening (bring-up semantics):
    //
    // Samgrd v1 currently moves *slot numbers* around (not endpoint caps), which is not a secure
    // global service registry. To avoid ambient/global poisoning, we scope registrations to the
    // kernel-derived sender service identity.
    //
    // This keeps the selftests honest (register/lookup roundtrip) while preventing one service
    // from registering entries that another service will observe.
    let mut registry: BTreeMap<(u64, Vec<u8>), (u32, u32)> = BTreeMap::new();
    let mut logged_capmove = false;
    let mut logged_register = false;
    let mut logged_any = false;
    // ONE request buffer for the service lifetime: the os-lite heap never frees, so an
    // allocating recv is a countdown (TASK-0054C P2-g/P5b). Transport-capped, never truncates.
    let mut recv_frame = alloc::vec![0u8; nexus_abi::IPC_PAYLOAD_MAX];
    // TASK-0054C P5b-2: the answer to the PREVIOUS request rides out with the wait for the next
    // one. A served request used to cost three kernel entries — `send_on_cap`, `cap_close`, then
    // the receive; `reply_recv` does all three. Deferring the reply to the loop top costs the
    // client nothing: every arm below `continue`s straight here. A cap-less request leaves this
    // empty, which keeps P2-c's rule intact — only senders that moved a capability are answered.
    let mut pending = nexus_ipc::PendingReply::new();
    loop {
        match server.serve_next(&mut pending, Wait::Blocking, &mut recv_frame) {
            Ok((hdr, frame_len, sid, mut reply)) => {
                let frame = &recv_frame[..frame_len];
                // The moved reply capability IS what the header's CAP_MOVE flag used to say
                // (TASK-0054C P5b); it is parked and sent by the next `reply_recv` (P5b-2).
                let has_reply_cap = reply.is_some();
                breaker.on_success();
                let sender_service_id = sid as u64;
                if !logged_any {
                    emit_line("samgrd: rx");
                    logged_any = true;
                }
                if has_reply_cap && !logged_capmove {
                    emit_line("samgrd: capmove seen");
                    logged_capmove = true;
                }
                if !logged_register
                    && frame.len() >= 4
                    && frame[0] == MAGIC0
                    && frame[1] == MAGIC1
                    && frame[2] == VERSION
                    && frame[3] == OP_REGISTER
                {
                    emit_line("samgrd: register seen");
                    logged_register = true;
                }
                // TASK-0006: core service wiring proof (structured log via nexus-log -> logd).
                // Probe is request-driven to avoid dependency on startup ordering.
                if frame.len() >= 4
                    && frame[0] == MAGIC0
                    && frame[1] == MAGIC1
                    && frame[2] == VERSION
                    && frame[3] == OP_LOG_PROBE
                {
                    let status =
                        if append_probe_to_logd() { STATUS_OK } else { STATUS_UNSUPPORTED };
                    let rsp = [MAGIC0, MAGIC1, VERSION, OP_LOG_PROBE | 0x80, status];
                    if has_reply_cap {
                        if let Some(c) = reply.take() {
                            pending.park(c, &rsp);
                        }
                    } else {
                        if server.send(&rsp, Wait::Blocking).is_err() {
                            emit_line("samgrd: send fail");
                        }
                    }
                    continue;
                }
                // Phase-2 scalability: if the client moved a reply cap, we can reply directly on it.
                if frame.len() >= 4
                    && frame[0] == MAGIC0
                    && frame[1] == MAGIC1
                    && frame[2] == VERSION
                    && frame[3] == OP_PING_CAP_MOVE
                {
                    // Reply on the moved cap slot (allocated into this process as reply_slot).
                    // Always best-effort and non-blocking for bring-up.
                    if frame.len() == 12 {
                        // Optional nonce correlation (RFC-0019 adoption): echo u64 nonce at end.
                        let mut rsp = [0u8; 12];
                        rsp[0..4].copy_from_slice(b"PONG");
                        rsp[4..12].copy_from_slice(&frame[4..12]);
                        if let Some(c) = reply.take() {
                            pending.park(c, &rsp);
                        }
                    } else {
                        if let Some(c) = reply.take() {
                            pending.park(c, b"PONG");
                        }
                    }
                    continue;
                }

                // Sender attribution probe: reply with observed sender pid (hdr.dst).
                if frame.len() >= 4
                    && frame[0] == MAGIC0
                    && frame[1] == MAGIC1
                    && frame[2] == VERSION
                    && frame[3] == OP_SENDER_PID
                    && has_reply_cap
                {
                    if frame.len() == 16 {
                        // Optional nonce correlation: request appends u64 nonce; reply echoes it at end.
                        let mut rsp = [0u8; 17];
                        rsp[0] = MAGIC0;
                        rsp[1] = MAGIC1;
                        rsp[2] = VERSION;
                        rsp[3] = OP_SENDER_PID | 0x80;
                        rsp[4] = STATUS_OK;
                        rsp[5..9].copy_from_slice(&hdr.dst.to_le_bytes());
                        rsp[9..17].copy_from_slice(&frame[8..16]);
                        if let Some(c) = reply.take() {
                            pending.park(c, &rsp);
                        }
                    } else {
                        let mut rsp = [0u8; 9];
                        rsp[0] = MAGIC0;
                        rsp[1] = MAGIC1;
                        rsp[2] = VERSION;
                        rsp[3] = OP_SENDER_PID | 0x80;
                        rsp[4] = STATUS_OK;
                        rsp[5..9].copy_from_slice(&hdr.dst.to_le_bytes());
                        if let Some(c) = reply.take() {
                            pending.park(c, &rsp);
                        }
                    }
                    continue;
                }

                // Sender service-id attribution probe: reply with kernel-derived sender service id.
                if frame.len() >= 4
                    && frame[0] == MAGIC0
                    && frame[1] == MAGIC1
                    && frame[2] == VERSION
                    && frame[3] == OP_SENDER_SERVICE_ID
                    && has_reply_cap
                {
                    if frame.len() == 12 {
                        // Optional nonce correlation: request appends u64 nonce; reply echoes it at end.
                        let mut rsp = [0u8; 21];
                        rsp[0] = MAGIC0;
                        rsp[1] = MAGIC1;
                        rsp[2] = VERSION;
                        rsp[3] = OP_SENDER_SERVICE_ID | 0x80;
                        rsp[4] = STATUS_OK;
                        rsp[5..13].copy_from_slice(&sender_service_id.to_le_bytes());
                        rsp[13..21].copy_from_slice(&frame[4..12]);
                        if let Some(c) = reply.take() {
                            pending.park(c, &rsp);
                        }
                    } else {
                        let mut rsp = [0u8; 13];
                        rsp[0] = MAGIC0;
                        rsp[1] = MAGIC1;
                        rsp[2] = VERSION;
                        rsp[3] = OP_SENDER_SERVICE_ID | 0x80;
                        rsp[4] = STATUS_OK;
                        rsp[5..13].copy_from_slice(&sender_service_id.to_le_bytes());
                        if let Some(c) = reply.take() {
                            pending.park(c, &rsp);
                        }
                    }
                    continue;
                }

                let rsp = handle_frame(&mut registry, sender_service_id, frame);
                // If a reply cap was moved, reply on it and close it.
                if has_reply_cap {
                    if let Some(c) = reply.take() {
                        pending.park(c, &rsp);
                    }
                } else {
                    if server.send(&rsp, Wait::Blocking).is_err() {
                        emit_line("samgrd: send fail");
                    }
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(nexus_ipc::IpcError::Disconnected) => {
                emit_line("samgrd: recv disconnected");
                return Err(ServerError::Unsupported);
            }
            Err(_) => {
                let (should_log, verdict) = breaker.on_error();
                if should_log {
                    emit_line("samgrd: recv error (transient)");
                }
                match verdict {
                    nexus_ipc::resilience::BreakerVerdict::Continue => {
                        let _ = yield_();
                    }
                    nexus_ipc::resilience::BreakerVerdict::EndpointDefect => {
                        emit_line("samgrd: endpoint defect (consecutive error limit)");
                        return Err(ServerError::Unsupported);
                    }
                }
            }
        }
    }
}

fn handle_frame(
    registry: &mut BTreeMap<(u64, Vec<u8>), (u32, u32)>,
    sender_service_id: u64,
    frame: &[u8],
) -> [u8; 13] {
    // REGISTER request:
    //   [S, M, ver, OP_REGISTER, name_len:u8, send_slot:u32le, recv_slot:u32le, name...]
    // REGISTER response:
    //   [S, M, ver, OP_REGISTER|0x80, status, 0,0,0,0,0,0,0,0]
    //
    // LOOKUP request:
    //   [S, M, ver, OP_LOOKUP, name_len:u8, name...]
    // LOOKUP response:
    //   [S, M, ver, OP_LOOKUP|0x80, status, send_slot:u32le, recv_slot:u32le]
    if frame.len() < 5 || frame[0] != MAGIC0 || frame[1] != MAGIC1 {
        return rsp(OP_LOOKUP, STATUS_MALFORMED, 0, 0);
    }
    if frame[2] != VERSION {
        return rsp(frame[3], STATUS_UNSUPPORTED, 0, 0);
    }
    let op = frame[3];
    match op {
        OP_REGISTER => {
            if frame.len() < 5 + 8 {
                return rsp(op, STATUS_MALFORMED, 0, 0);
            }
            let n = frame[4] as usize;
            if n == 0 || frame.len() != 13 + n {
                return rsp(op, STATUS_MALFORMED, 0, 0);
            }
            let send_slot = u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]);
            let recv_slot = u32::from_le_bytes([frame[9], frame[10], frame[11], frame[12]]);
            let name = &frame[13..];
            registry.insert((sender_service_id, name.to_vec()), (send_slot, recv_slot));
            rsp(op, STATUS_OK, 0, 0)
        }
        OP_LOOKUP => {
            let n = frame[4] as usize;
            if n == 0 || frame.len() != 5 + n {
                return rsp(op, STATUS_MALFORMED, 0, 0);
            }
            let name = &frame[5..];
            match registry.get(&(sender_service_id, name.to_vec())).copied() {
                Some((send_slot, recv_slot)) => rsp(op, STATUS_OK, send_slot, recv_slot),
                None => rsp(op, STATUS_NOT_FOUND, 0, 0),
            }
        }
        OP_RESOLVE_STATUS => {
            // RESOLVE_STATUS request:
            //   [S, M, ver, OP_RESOLVE_STATUS, name_len:u8, name...]
            // Response: [S, M, ver, OP_RESOLVE_STATUS|0x80, status, 0,0,0,0,0,0,0,0]
            //
            // Security note (TASK-0005): this op returns ONLY status, never capability slots.
            let n = frame[4] as usize;
            if n == 0 || n > nexus_abi::routing::MAX_SERVICE_NAME_LEN || frame.len() != 5 + n {
                return rsp(op, STATUS_MALFORMED, 0, 0);
            }
            let name = &frame[5..];
            // Phase-3: query the registry (populated by init-lite at boot).
            // Look across all sender scopes to find any registration for this name.
            let found =
                registry.iter().any(|((_sid, reg_name), _slots)| reg_name.as_slice() == name);
            if found {
                rsp(op, STATUS_OK, 0, 0)
            } else {
                rsp(op, STATUS_NOT_FOUND, 0, 0)
            }
        }
        _ => rsp(op, STATUS_UNSUPPORTED, 0, 0),
    }
}

fn rsp(op: u8, status: u8, send_slot: u32, recv_slot: u32) -> [u8; 13] {
    let mut out = [0u8; 13];
    out[0] = MAGIC0;
    out[1] = MAGIC1;
    out[2] = VERSION;
    out[3] = op | 0x80;
    out[4] = status;
    out[5..9].copy_from_slice(&send_slot.to_le_bytes());
    out[9..13].copy_from_slice(&recv_slot.to_le_bytes());
    out
}

fn emit_line(message: &str) {
    // Verdict folding → `samgrd N/N` (interactive); post-`ready` runtime markers fold into recall;
    // failures & proof boots print live & raw.
    // One atomic `debug_write` (via `debug_println`, which also owns the verdict
    // folding): the per-byte `debug_putc` fallback tears mid-line against the
    // kernel's locked log records and DROPS the tail — a torn ready marker is a
    // red ladder gate.
    let _ = nexus_abi::debug_println(message);
}

fn emit_bytes(bytes: &[u8]) {
    for byte in bytes.iter().copied() {
        let _ = debug_putc(byte);
    }
}

fn emit_byte(byte: u8) {
    let _ = debug_putc(byte);
}

fn emit_hex_u32(value: u32) {
    for shift in (0..8).rev() {
        let nib = (value >> (shift * 4)) & 0x0f;
        let ch = if nib < 10 { b'0' + nib as u8 } else { b'a' + (nib as u8 - 10) };
        let _ = debug_putc(ch);
    }
}

fn append_probe_to_logd() -> bool {
    const MAGIC0: u8 = b'L';
    const MAGIC1: u8 = b'O';
    const VERSION: u8 = 2;
    const OP_APPEND: u8 = 1;
    const LEVEL_INFO: u8 = 2;
    const STATUS_OK: u8 = 0;
    static NONCE: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);

    let scope: &[u8] = b"samgrd";
    let msg: &[u8] = b"core service log probe: samgrd";
    // The declared legs (TASK-0324 P7-d): logd's request endpoint and our reply inbox; the
    // append is ONE waited exchange — logd's ack (nonce-matched) or logd's death ends it.
    let logd = nexus_service_topology::slots::samgrd::LOGD;
    let reply = nexus_service_topology::slots::samgrd::REPLY;
    let nonce = NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let mut frame = alloc::vec::Vec::with_capacity(12 + 1 + 1 + 2 + 2 + scope.len() + msg.len());
    frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_APPEND]);
    frame.extend_from_slice(&nonce.to_le_bytes());
    frame.push(LEVEL_INFO);
    frame.push(scope.len() as u8);
    frame.extend_from_slice(&(msg.len() as u16).to_le_bytes());
    frame.extend_from_slice(&0u16.to_le_bytes()); // fields_len
    frame.extend_from_slice(scope);
    frame.extend_from_slice(msg);
    let mut buf = [0u8; 64];
    let accept = |rsp: &[u8]| -> Option<bool> {
        if rsp.len() < 13 || rsp[0] != MAGIC0 || rsp[1] != MAGIC1 || rsp[2] != VERSION {
            return None;
        }
        if rsp[3] != (OP_APPEND | 0x80) {
            return None;
        }
        let (status, got_nonce) =
            nexus_ipc::logd_wire::parse_append_response_v2_prefix(rsp).ok()?;
        (got_nonce == nonce).then_some(status == STATUS_OK)
    };
    nexus_ipc::exchange::call_matching(
        logd.send,
        nexus_service_topology::SlotPair::new(reply.send, reply.recv),
        &frame,
        &mut buf,
        accept,
    )
    .unwrap_or(false)
}
