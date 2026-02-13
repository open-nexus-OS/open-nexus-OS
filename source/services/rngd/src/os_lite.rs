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

use nexus_abi::{debug_putc, yield_};
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
    if name.is_empty() || name.len() > nexus_abi::routing::MAX_SERVICE_NAME_LEN {
        return None;
    }
    static ROUTE_NONCE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);
    let nonce = ROUTE_NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

    // Routing v1+nonce extension:
    // GET: [R,T,1,OP_ROUTE_GET, name_len, name..., nonce:u32le]
    // RSP: [R,T,1,OP_ROUTE_RSP, status, send_slot:u32le, recv_slot:u32le, nonce:u32le]
    let mut req = [0u8; 5 + nexus_abi::routing::MAX_SERVICE_NAME_LEN + 4];
    let base_len = nexus_abi::routing::encode_route_get(name, &mut req[..5 + name.len()])?;
    req[base_len..base_len + 4].copy_from_slice(&nonce.to_le_bytes());
    let req_len = base_len + 4;
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
                    if n == 17 {
                        let (status, send_slot, recv_slot) =
                            nexus_abi::routing::decode_route_rsp(&buf[..13])?;
                        let got_nonce = u32::from_le_bytes([buf[13], buf[14], buf[15], buf[16]]);
                        if got_nonce != nonce {
                            continue;
                        }
                        if status == nexus_abi::routing::STATUS_OK {
                            return Some((send_slot, recv_slot));
                        }
                        break;
                    }
                    // Ignore legacy/non-correlated control frames.
                    let _ = yield_();
                    continue;
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
fn policyd_allows(pending: &mut reqrep::ReplyBuffer<16, 512>, subject_id: u64, cap: &[u8]) -> bool {
    const MAGIC0: u8 = b'P';
    const MAGIC1: u8 = b'O';
    const VERSION_V2: u8 = 2;
    // Delegated check: rngd is an enforcement point; policyd validates that rngd is allowed
    // to query policy for another subject id.
    const OP_CHECK_CAP_DELEGATED: u8 = 5;
    const STATUS_ALLOW: u8 = 0;

    if cap.is_empty() || cap.len() > 48 {
        return false;
    }

    // v2 request: [P,O,ver=2,op, nonce:u32le, subject_id:u64le, cap_len:u8, cap...]
    static NONCE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);
    let nonce = NONCE.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let mut frame = Vec::with_capacity(17 + cap.len());
    frame.push(MAGIC0);
    frame.push(MAGIC1);
    frame.push(VERSION_V2);
    frame.push(OP_CHECK_CAP_DELEGATED);
    frame.extend_from_slice(&nonce.to_le_bytes());
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

    // Close our local clone of the reply send cap (it has been moved to policyd).
    let _ = nexus_abi::cap_close(reply_send_clone);

    // Deterministic receive: wait for nonce-correlated v2 reply, buffer unrelated replies.
    struct ReplyInboxV1 {
        recv_slot: u32,
    }
    impl nexus_ipc::Client for ReplyInboxV1 {
        fn send(&self, _frame: &[u8], _wait: Wait) -> nexus_ipc::Result<()> {
            Err(nexus_ipc::IpcError::Unsupported)
        }
        fn recv(&self, _wait: Wait) -> nexus_ipc::Result<Vec<u8>> {
            let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
            let mut buf = [0u8; 512];
            match nexus_abi::ipc_recv_v1(
                self.recv_slot,
                &mut rh,
                &mut buf,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(n) => Ok(buf[..core::cmp::min(n as usize, buf.len())].to_vec()),
                Err(nexus_abi::IpcError::QueueEmpty) => Err(nexus_ipc::IpcError::WouldBlock),
                Err(other) => Err(nexus_ipc::IpcError::Kernel(other)),
            }
        }
    }

    let clock = budget::OsClock;
    let deadline_ns = match budget::deadline_after(&clock, core::time::Duration::from_millis(500)) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let inbox = ReplyInboxV1 { recv_slot: reply_recv_slot };
    let rsp = match reqrep::recv_match_until(
        &clock,
        &inbox,
        pending,
        nonce as u64,
        deadline_ns,
        |frame| {
            if frame.len() == 10
                && frame[0] == MAGIC0
                && frame[1] == MAGIC1
                && frame[2] == VERSION_V2
            {
                Some(u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]) as u64)
            } else {
                None
            }
        },
    ) {
        Ok(v) => v,
        Err(_) => return false,
    };

    crate::decode_delegated_cap_decision(&rsp, nonce) == Some(STATUS_ALLOW)
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
