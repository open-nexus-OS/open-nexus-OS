// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! OS-lite backend for rngd — single entropy authority.
//!
//! SECURITY INVARIANTS:
//! - Entropy bytes are NEVER logged
//! - All requests are policy-gated via sender_service_id
//! - Requests are bounded to MAX_ENTROPY_BYTES

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use core::time::Duration;

use nexus_abi::{debug_putc, yield_};
use nexus_ipc::budget::{NonceMismatchBudget, RouteRetryOutcome};
use nexus_ipc::{budget, reqrep};
use nexus_ipc::{KernelServer, Server as _, Wait};

use crate::protocol::*;
use crate::MAX_ENTROPY_BYTES;

/// Flag to emit MMIO proof marker only once (proves designated owner service mapped its window).
static MMIO_PROOF_EMITTED: AtomicBool = AtomicBool::new(false);

/// Result type for rngd operations.
pub type RngdResult<T> = Result<T, RngdError>;

/// Errors from the rngd service.
#[derive(Debug)]
pub enum RngdError {
    /// IPC error.
    Ipc(&'static str),
    /// Device not available.
    DeviceUnavailable,
}

impl core::fmt::Display for RngdError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Ipc(msg) => write!(f, "ipc: {}", msg),
            Self::DeviceUnavailable => write!(f, "rng device unavailable"),
        }
    }
}

/// Notifies init once the service reports readiness.
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

/// Main service loop for rngd.
///
/// # Security
/// - All entropy requests are policy-gated via `policyd` OP_CHECK_CAP.
/// - Entropy bytes are NEVER logged.
/// - Bounded requests only (max 256 bytes).
pub fn service_main_loop(notifier: ReadyNotifier) -> RngdResult<()> {
    // Signal readiness to init (service started).
    notifier.notify();

    // Emit readiness marker early to keep `scripts/qemu-test.sh` marker ordering stable.
    emit_line("rngd: ready");

    // Route to get our IPC endpoint.
    let server = route_rngd_blocking().ok_or(RngdError::Ipc("route failed"))?;

    // Shared CAP_MOVE reply inbox buffer (nonce-correlated policyd replies).
    let mut pending_replies: reqrep::ReplyBuffer<16, 512> = reqrep::ReplyBuffer::new();

    // Main IPC loop
    loop {
        match server.recv_request_with_meta(Wait::Blocking) {
            Ok((frame, sender_service_id, reply)) => {
                let rsp = handle_frame(&mut pending_replies, sender_service_id, frame.as_slice());

                if let Some(reply) = reply {
                    let _ = reply.reply_and_close(&rsp);
                } else {
                    let _ = server.send(&rsp, Wait::Blocking);
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(nexus_ipc::IpcError::Disconnected) => {
                emit_line("rngd: recv disconnected");
                return Err(RngdError::Ipc("disconnected"));
            }
            Err(_) => {
                emit_line("rngd: recv error");
                return Err(RngdError::Ipc("recv"));
            }
        }
    }
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

fn route_rngd_blocking() -> Option<KernelServer> {
    if let Some((send_slot, recv_slot)) = route_blocking(b"rngd") {
        return KernelServer::new_with_slots(recv_slot, send_slot).ok();
    }
    // Routing budget expired (slow boots — e.g. the virgl GPU bringup delays
    // init's wiring past the 2s budget). Fall back to the deterministic slots
    // init wires via cap_transfer (recv first → 3, send second → 4).
    emit_line("rngd: route fallback slots");
    KernelServer::new_with_slots(RNGD_RECV_SLOT, RNGD_SEND_SLOT).ok()
}

/// Deterministic slots wired by init's cap_transfer for rngd.
const RNGD_RECV_SLOT: u32 = 0x03;
const RNGD_SEND_SLOT: u32 = 0x04;

/// Handle an incoming IPC frame.
///
/// # Security
/// - Never log entropy bytes
/// - Policy check on sender_service_id
fn handle_frame(
    pending: &mut reqrep::ReplyBuffer<16, 512>,
    sender_service_id: u64,
    frame: &[u8],
) -> Vec<u8> {
    // Validate magic and version
    if frame.len() < MIN_FRAME_LEN || frame[0] != MAGIC0 || frame[1] != MAGIC1 {
        return rsp(OP_GET_ENTROPY, STATUS_MALFORMED, &[]);
    }

    let version = frame[2];
    let op = frame[3];

    if version != VERSION {
        return rsp(op, STATUS_MALFORMED, &[]);
    }

    match op {
        OP_GET_ENTROPY => handle_get_entropy(pending, sender_service_id, frame),
        _ => rsp(op, STATUS_MALFORMED, &[]),
    }
}

fn handle_get_entropy(
    pending: &mut reqrep::ReplyBuffer<16, 512>,
    sender_service_id: u64,
    frame: &[u8],
) -> Vec<u8> {
    // GET_ENTROPY request: [MAGIC0, MAGIC1, VERSION, OP, nonce:u32le, n:u16le]
    if frame.len() != GET_ENTROPY_REQ_LEN {
        return rsp(OP_GET_ENTROPY, STATUS_MALFORMED, &[]);
    }

    let nonce = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    let n = u16::from_le_bytes([frame[8], frame[9]]) as usize;

    // Bounds check
    if n == 0 || n > MAX_ENTROPY_BYTES {
        return rsp_with_nonce(OP_GET_ENTROPY, STATUS_OVERSIZED, nonce, &[]);
    }

    // Policy check via policyd
    emit_line("rngd: policy check");
    if !policyd_allows(pending, sender_service_id, CAP_RNG_ENTROPY) {
        // Audit: denial is logged by policyd; we just return status
        emit_line("rngd: entropy denied");
        return rsp_with_nonce(OP_GET_ENTROPY, STATUS_DENIED, nonce, &[]);
    }

    // Read entropy from virtio-rng
    // NOTE: In real implementation, we would use the rng-virtio library here.
    // For bring-up, we use a simplified entropy source.
    match read_entropy_from_device(n) {
        Ok(entropy) => {
            // SECURITY: Do NOT log entropy bytes!
            emit_line("rngd: entropy ok");
            rsp_with_nonce(OP_GET_ENTROPY, STATUS_OK, nonce, &entropy)
        }
        Err(_) => rsp_with_nonce(OP_GET_ENTROPY, STATUS_UNAVAILABLE, nonce, &[]),
    }
}

/// Read entropy from the virtio-rng device.
///
/// # Security
/// - Entropy bytes are NEVER logged
fn read_entropy_from_device(n: usize) -> Result<Vec<u8>, rng_virtio::RngError> {
    // DeviceMmio cap is distributed by init (policy-gated) into a deterministic slot.
    const MMIO_CAP_SLOT: u32 = 48;
    const MMIO_VA: usize = 0x2000_e000;
    const MAX_SLOTS: usize = 1;

    let result = rng_virtio::read_entropy_via_virtio_mmio(MMIO_CAP_SLOT, MMIO_VA, MAX_SLOTS, n);

    // Emit proof marker on first successful read (proves owner service mapped its MMIO window).
    if result.is_ok() && !MMIO_PROOF_EMITTED.swap(true, Ordering::Relaxed) {
        emit_line("rngd: mmio window mapped ok");
    }

    result
}

/// Check if the caller has the required capability via policyd.
fn policyd_allows(_pending: &mut reqrep::ReplyBuffer<16, 512>, subject_id: u64, cap: &[u8]) -> bool {
    // RFC-0066: the shared route-based CAP_MOVE policy check
    // (nexus_ipc::policyd::check_cap_delegated). The ~140-line hand-rolled copy
    // was removed; the reply-buffer plumbing is retained but now unused.
    matches!(
        nexus_ipc::policyd::check_cap_delegated(subject_id, cap),
        nexus_ipc::policyd::CapDecision::Allow
    )
}

fn rsp(op: u8, status: u8, value: &[u8]) -> Vec<u8> {
    // Response: [MAGIC0, MAGIC1, VERSION, OP|0x80, STATUS, val...]
    let mut out = Vec::with_capacity(5 + value.len());
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(op | OP_RESPONSE);
    out.push(status);
    out.extend_from_slice(value);
    out
}

fn rsp_with_nonce(op: u8, status: u8, nonce: u32, value: &[u8]) -> Vec<u8> {
    // Response: [MAGIC0, MAGIC1, VERSION, OP|0x80, STATUS, nonce:u32le, val...]
    let mut out = Vec::with_capacity(9 + value.len());
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(op | OP_RESPONSE);
    out.push(status);
    out.extend_from_slice(&nonce.to_le_bytes());
    out.extend_from_slice(value);
    out
}

fn emit_line(message: &str) {
    for byte in message.as_bytes().iter().copied().chain(core::iter::once(b'\n')) {
        let _ = debug_putc(byte);
    }
}
