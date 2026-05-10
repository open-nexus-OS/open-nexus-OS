// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: os-lite runtime backend for timed daemon request handling
//! OWNERS: @runtime
//! PUBLIC API: service_main_loop(), TimedResult, TimedError, ReadyNotifier
//! DEPENDS_ON: nexus-abi IPC syscalls, nexus-ipc server transport, timed::TimerRegistry
//! INVARIANTS:
//! - ready marker emits once only after route/server are available
//! - timer registration rejects are deterministic and bounded
//! - shared-inbox routing replies are nonce-correlated and bounded

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use core::time::Duration;

use nexus_abi::{debug_putc, nsec, yield_};
use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};
use nexus_ipc::{KernelServer, Server as _, Wait};
use nexus_metrics::client::MetricsClient;

use crate::protocol::*;
use crate::{coalesced_deadline, RegisterReject, TimerRegistry};

static READY_MARKER_EMITTED: AtomicBool = AtomicBool::new(false);
static REGISTER_ALLOW_AUDIT_EMITTED: AtomicBool = AtomicBool::new(false);

/// Main timed service loop.
pub fn service_main_loop(notifier: ReadyNotifier) -> TimedResult<()> {
    let server = match route_timed_blocking() {
        Some(v) => v,
        None => return Err(TimedError::Ipc("route failed")),
    };
    notifier.notify();
    if !READY_MARKER_EMITTED.swap(true, Ordering::Relaxed) {
        emit_line("timed: ready");
    }
    let mut registry = TimerRegistry::new();

    loop {
        match server.recv_request_with_meta(Wait::Blocking) {
            Ok((frame, sender_service_id, reply)) => {
                let rsp = handle_frame(&mut registry, sender_service_id, frame.as_slice());
                if let Some(reply) = reply {
                    let _ = reply.reply_and_close(&rsp);
                } else {
                    let _ = server.send(&rsp, Wait::Blocking);
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(nexus_ipc::IpcError::Disconnected) => return Err(TimedError::Ipc("disconnected")),
            Err(_) => return Err(TimedError::Ipc("recv")),
        }
    }
}

fn handle_frame(registry: &mut TimerRegistry, sender_service_id: u64, frame: &[u8]) -> Vec<u8> {
    let op = frame.get(3).copied().unwrap_or(0);
    let nonce = read_u32(frame, 4).unwrap_or(0);
    if frame.len() < MIN_FRAME_LEN
        || frame[0] != MAGIC0
        || frame[1] != MAGIC1
        || frame[2] != VERSION
    {
        return rsp_status(op, STATUS_MALFORMED, nonce);
    }

    match op {
        OP_REGISTER => handle_register(registry, sender_service_id, frame),
        OP_CANCEL => handle_cancel(registry, sender_service_id, frame),
        OP_SLEEP_UNTIL => handle_sleep_until(frame),
        _ => rsp_status(op, STATUS_MALFORMED, nonce),
    }
}

fn handle_register(registry: &mut TimerRegistry, sender_service_id: u64, frame: &[u8]) -> Vec<u8> {
    if frame.len() != REGISTER_REQ_LEN {
        emit_register_audit("deny", "malformed");
        return rsp_status(OP_REGISTER, STATUS_MALFORMED, read_u32(frame, 4).unwrap_or(0));
    }
    let nonce = read_u32(frame, 4).unwrap_or(0);
    let qos_raw = frame[8];
    let deadline_ns = read_u64(frame, 10).unwrap_or(0);
    let Some(coalesced_ns) = coalesced_deadline(deadline_ns, qos_raw) else {
        emit_register_audit("deny", "invalid_args");
        return rsp_register(STATUS_INVALID_ARGS, nonce, 0, 0);
    };
    match registry.register(sender_service_id, coalesced_ns) {
        Ok(id) => {
            let delta_ns = coalesced_ns.saturating_sub(deadline_ns);
            metrics_hist_coalesce_delta(delta_ns);
            if !REGISTER_ALLOW_AUDIT_EMITTED.swap(true, Ordering::Relaxed) {
                emit_register_audit("allow", "applied");
            }
            rsp_register(STATUS_OK, nonce, id, coalesced_ns)
        }
        Err(RegisterReject::OverLimit) | Err(RegisterReject::NoSpace) => {
            emit_register_audit("deny", "over_limit");
            rsp_register(STATUS_OVER_LIMIT, nonce, 0, 0)
        }
    }
}

fn handle_cancel(registry: &mut TimerRegistry, sender_service_id: u64, frame: &[u8]) -> Vec<u8> {
    if frame.len() != CANCEL_REQ_LEN {
        return rsp_status(OP_CANCEL, STATUS_MALFORMED, read_u32(frame, 4).unwrap_or(0));
    }
    let nonce = read_u32(frame, 4).unwrap_or(0);
    let timer_id = read_u32(frame, 8).unwrap_or(0);
    if registry.cancel(sender_service_id, timer_id) {
        rsp_status(OP_CANCEL, STATUS_OK, nonce)
    } else {
        rsp_status(OP_CANCEL, STATUS_NOT_FOUND, nonce)
    }
}

fn handle_sleep_until(frame: &[u8]) -> Vec<u8> {
    if frame.len() != SLEEP_REQ_LEN {
        return rsp_sleep(STATUS_MALFORMED, read_u32(frame, 4).unwrap_or(0), 0);
    }
    let nonce = read_u32(frame, 4).unwrap_or(0);
    let qos_raw = frame[8];
    let deadline_ns = read_u64(frame, 10).unwrap_or(0);
    let Some(coalesced_ns) = coalesced_deadline(deadline_ns, qos_raw) else {
        return rsp_sleep(STATUS_INVALID_ARGS, nonce, 0);
    };
    let start = match nsec() {
        Ok(now) => now,
        Err(_) => return rsp_sleep(STATUS_INTERNAL, nonce, 0),
    };
    if coalesced_ns > start.saturating_add(crate::MAX_SLEEP_NS) {
        return rsp_sleep(STATUS_INVALID_ARGS, nonce, start);
    }
    loop {
        let now = match nsec() {
            Ok(v) => v,
            Err(_) => return rsp_sleep(STATUS_INTERNAL, nonce, 0),
        };
        if now >= coalesced_ns {
            return rsp_sleep(STATUS_OK, nonce, now);
        }
        let _ = yield_();
    }
}

fn route_timed_blocking() -> Option<KernelServer> {
    let (send_slot, recv_slot) = route_blocking(b"timed")?;
    KernelServer::new_with_slots(recv_slot, send_slot).ok()
}

fn route_blocking(name: &[u8]) -> Option<(u32, u32)> {
    const CTRL_SEND_SLOT: u32 = 1;
    const CTRL_RECV_SLOT: u32 = 2;
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

fn read_u32(frame: &[u8], offset: usize) -> Option<u32> {
    let bytes = frame.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u64(frame: &[u8], offset: usize) -> Option<u64> {
    let bytes = frame.get(offset..offset + 8)?;
    Some(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn rsp_status(op: u8, status: u8, nonce: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(9);
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(op | OP_RESPONSE);
    out.push(status);
    out.extend_from_slice(&nonce.to_le_bytes());
    out
}

fn rsp_register(status: u8, nonce: u32, timer_id: u32, coalesced_ns: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(21);
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(OP_REGISTER | OP_RESPONSE);
    out.push(status);
    out.extend_from_slice(&nonce.to_le_bytes());
    out.extend_from_slice(&timer_id.to_le_bytes());
    out.extend_from_slice(&coalesced_ns.to_le_bytes());
    out
}

fn rsp_sleep(status: u8, nonce: u32, wake_ns: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(17);
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(OP_SLEEP_UNTIL | OP_RESPONSE);
    out.push(status);
    out.extend_from_slice(&nonce.to_le_bytes());
    out.extend_from_slice(&wake_ns.to_le_bytes());
    out
}

fn emit_line(message: &str) {
    for byte in message.as_bytes().iter().copied().chain(core::iter::once(b'\n')) {
        let _ = debug_putc(byte);
    }
}

fn emit_register_audit(decision: &str, reason: &str) {
    emit_line_no_nl("timed: audit register decision=");
    for b in decision.as_bytes() {
        let _ = debug_putc(*b);
    }
    emit_line_no_nl(" reason=");
    for b in reason.as_bytes() {
        let _ = debug_putc(*b);
    }
    let _ = debug_putc(b'\n');
}

fn emit_line_no_nl(message: &str) {
    for b in message.as_bytes() {
        let _ = debug_putc(*b);
    }
}

fn metrics_hist_coalesce_delta(delta_ns: u64) {
    let Ok(metrics) = MetricsClient::new() else {
        return;
    };
    let _ = metrics.hist_observe("timed.coalesce.delta_ns", b"svc=timed\n", delta_ns);
}

/// Result type for timed service operations.
pub type TimedResult<T> = Result<T, TimedError>;

/// Errors returned by timed service loop.
#[derive(Debug)]
pub enum TimedError {
    Ipc(&'static str),
}

impl core::fmt::Display for TimedError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Ipc(msg) => write!(f, "ipc: {}", msg),
        }
    }
}

/// Notifies init when timed has started.
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    pub fn new<F>(func: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self(Box::new(func))
    }

    pub fn notify(self) {
        (self.0)();
    }
}
