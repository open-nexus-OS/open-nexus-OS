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

use nexus_abi::{debug_putc, yield_};
use nexus_ipc::{KernelServer, Server as _, Wait};

use crate::protocol::*;
use crate::MAX_ENTROPY_BYTES;

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
    // Signal readiness
    notifier.notify();
    emit_line("rngd: ready");

    // Route to get our IPC endpoint
    let server = route_rngd_blocking().ok_or(RngdError::Ipc("route failed"))?;

    // Main IPC loop
    loop {
        match server.recv_request_with_meta(Wait::Blocking) {
            Ok((frame, sender_service_id, reply)) => {
                let rsp = handle_frame(sender_service_id, frame.as_slice());

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
    if name.is_empty() || name.len() > nexus_abi::routing::MAX_SERVICE_NAME_LEN {
        return None;
    }

    // Drain stale responses; routing has no nonce.
    for _ in 0..32 {
        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 32];
        match nexus_abi::ipc_recv_v1(
            CTRL_RECV_SLOT,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(_) => continue,
            Err(nexus_abi::IpcError::QueueEmpty) => break,
            Err(_) => break,
        }
    }

    let mut req = [0u8; 5 + nexus_abi::routing::MAX_SERVICE_NAME_LEN];
    let req_len = nexus_abi::routing::encode_route_get(name, &mut req)?;
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, req_len as u32);

    loop {
        loop {
            match nexus_abi::ipc_send_v1(
                CTRL_SEND_SLOT,
                &hdr,
                &req[..req_len],
                nexus_abi::IPC_SYS_NONBLOCK,
                0,
            ) {
                Ok(_) => break,
                Err(nexus_abi::IpcError::QueueFull) => {
                    let _ = yield_();
                }
                Err(_) => return None,
            }
        }

        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 32];
        loop {
            match nexus_abi::ipc_recv_v1(
                CTRL_RECV_SLOT,
                &mut rh,
                &mut buf,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(n) => {
                    let n = n as usize;
                    let (status, send_slot, recv_slot) =
                        nexus_abi::routing::decode_route_rsp(&buf[..n])?;
                    if status == nexus_abi::routing::STATUS_OK {
                        return Some((send_slot, recv_slot));
                    }
                    break;
                }
                Err(nexus_abi::IpcError::QueueEmpty) => {
                    let _ = yield_();
                }
                Err(_) => return None,
            }
        }
    }
}

fn route_rngd_blocking() -> Option<KernelServer> {
    let (send_slot, recv_slot) = route_blocking(b"rngd")?;
    KernelServer::new_with_slots(recv_slot, send_slot).ok()
}

/// Handle an incoming IPC frame.
///
/// # Security
/// - Never log entropy bytes
/// - Policy check on sender_service_id
fn handle_frame(sender_service_id: u64, frame: &[u8]) -> Vec<u8> {
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
        OP_GET_ENTROPY => handle_get_entropy(sender_service_id, frame),
        _ => rsp(op, STATUS_MALFORMED, &[]),
    }
}

fn handle_get_entropy(sender_service_id: u64, frame: &[u8]) -> Vec<u8> {
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
    if !policyd_allows(sender_service_id, CAP_RNG_ENTROPY) {
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
    // Virtio-mmio DeviceMmio cap is injected into OS processes at a fixed slot for bring-up.
    // This service is the single entropy authority.
    const MMIO_CAP_SLOT: u32 = 48;
    const MMIO_VA: usize = 0x2000_e000;
    const MAX_SLOTS: usize = 8;
    rng_virtio::read_entropy_via_virtio_mmio(MMIO_CAP_SLOT, MMIO_VA, MAX_SLOTS, n)
}

/// Check if the caller has the required capability via policyd.
fn policyd_allows(subject_id: u64, cap: &[u8]) -> bool {
    const MAGIC0: u8 = b'P';
    const MAGIC1: u8 = b'O';
    const VERSION: u8 = 1;
    // Delegated check: rngd is an enforcement point; policyd validates that rngd is allowed
    // to query policy for another subject id.
    const OP_CHECK_CAP_DELEGATED: u8 = 5;
    const STATUS_ALLOW: u8 = 0;

    if cap.is_empty() || cap.len() > 48 {
        return false;
    }

    let mut frame = Vec::with_capacity(13 + cap.len());
    frame.push(MAGIC0);
    frame.push(MAGIC1);
    frame.push(VERSION);
    frame.push(OP_CHECK_CAP_DELEGATED);
    frame.extend_from_slice(&subject_id.to_le_bytes());
    frame.push(cap.len() as u8);
    frame.extend_from_slice(cap);

    // Send to policyd and receive reply via CAP_MOVE on @reply.
    let (send_slot, _recv_slot) = match route_blocking(b"policyd") {
        Some(slots) => slots,
        None => {
            emit_line("rngd: policyd route fail");
            return false;
        }
    };
    let (reply_send_slot, reply_recv_slot) = match route_blocking(b"@reply") {
        Some(slots) => slots,
        None => return false,
    };
    let reply_send_clone = match nexus_abi::cap_clone(reply_send_slot) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let hdr = nexus_abi::MsgHeader::new(
        reply_send_clone,
        0,
        0,
        nexus_abi::ipc_hdr::CAP_MOVE,
        frame.len() as u32,
    );
    let start = match nexus_abi::nsec() {
        Ok(value) => value,
        Err(_) => return false,
    };
    let deadline = start.saturating_add(500_000_000);

    let mut i: usize = 0;
    let mut send_spins: u32 = 0;
    const MAX_SEND_SPINS: u32 = 200_000;
    loop {
        match nexus_abi::ipc_send_v1(send_slot, &hdr, &frame, nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => break,
            Err(nexus_abi::IpcError::QueueFull) => {
                if (i & 0x7f) == 0 {
                    let now = match nexus_abi::nsec() {
                        Ok(value) => value,
                        Err(_) => return false,
                    };
                    if now >= deadline {
                        let _ = nexus_abi::cap_close(reply_send_clone);
                        return false;
                    }
                }
                if send_spins >= MAX_SEND_SPINS {
                    let _ = nexus_abi::cap_close(reply_send_clone);
                    return false;
                }
                let _ = yield_();
            }
            Err(_) => return false,
        }
        i = i.wrapping_add(1);
        send_spins = send_spins.wrapping_add(1);
    }

    let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 16];
    let mut j: usize = 0;
    let mut recv_spins: u32 = 0;
    const MAX_RECV_SPINS: u32 = 200_000;
    loop {
        if (j & 0x7f) == 0 {
            let now = match nexus_abi::nsec() {
                Ok(value) => value,
                Err(_) => return false,
            };
            if now >= deadline {
                return false;
            }
        }
        match nexus_abi::ipc_recv_v1(
            reply_recv_slot,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                if n != 6 || buf[0] != MAGIC0 || buf[1] != MAGIC1 || buf[2] != VERSION {
                    continue;
                }
                if buf[3] != (OP_CHECK_CAP_DELEGATED | 0x80) {
                    continue;
                }
                return buf[4] == STATUS_ALLOW;
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => return false,
        }
        j = j.wrapping_add(1);
        recv_spins = recv_spins.wrapping_add(1);
        if recv_spins >= MAX_RECV_SPINS {
            return false;
        }
    }
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
