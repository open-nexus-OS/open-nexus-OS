#![cfg(all(nexus_env = "os", feature = "os-lite"))]
#![forbid(unsafe_code)]

//! CONTEXT: statefsd os-lite backend (kernel IPC server; byte-frame protocol v1)
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

use core::fmt;

use nexus_abi::{debug_putc, yield_};
use nexus_ipc::{KernelServer, Server as _, Wait};

use statefs::protocol::{self as proto, Request};
use statefs::{JournalEngine, StatefsError};
use storage::virtio_blk::VirtioBlkDevice;
use storage::BlockDevice;
use storage::MemBlockDevice;

/// Result alias surfaced by the lite statefsd backend.
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
    /// Functionality not yet available in the os-lite path.
    Unsupported,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "statefsd unsupported"),
        }
    }
}

/// Schema warmer placeholder for API parity.
pub fn touch_schemas() {}

const BLOCK_SIZE: usize = 512;
const BLOCK_COUNT: u64 = 64;
const MAX_LIST_RESPONSE_BYTES: usize = 512;

const CAP_READ: &str = "statefs.read";
const CAP_WRITE: &str = "statefs.write";
const CAP_KEYSTORE: &str = "statefs.keystore";
const CAP_BOOT: &str = "statefs.boot";

enum Backend {
    Virtio(VirtioBlkDevice),
    Mem(MemBlockDevice),
}

impl BlockDevice for Backend {
    fn block_size(&self) -> usize {
        match self {
            Backend::Virtio(dev) => dev.block_size(),
            Backend::Mem(dev) => dev.block_size(),
        }
    }

    fn block_count(&self) -> u64 {
        match self {
            Backend::Virtio(dev) => dev.block_count(),
            Backend::Mem(dev) => dev.block_count(),
        }
    }

    fn read_block(&self, block_idx: u64, buf: &mut [u8]) -> Result<(), storage::BlockError> {
        match self {
            Backend::Virtio(dev) => dev.read_block(block_idx, buf),
            Backend::Mem(dev) => dev.read_block(block_idx, buf),
        }
    }

    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), storage::BlockError> {
        match self {
            Backend::Virtio(dev) => dev.write_block(block_idx, buf),
            Backend::Mem(dev) => dev.write_block(block_idx, buf),
        }
    }

    fn sync(&mut self) -> Result<(), storage::BlockError> {
        match self {
            Backend::Virtio(dev) => dev.sync(),
            Backend::Mem(dev) => dev.sync(),
        }
    }
}

/// Main statefsd bring-up service loop (os-lite).
pub fn service_main_loop(notifier: ReadyNotifier) -> LiteResult<()> {
    emit_line("statefsd: entry");
    // init-lite transfers the statefsd service endpoints into deterministic slots:
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

    // Start with an in-memory backend so we can become ready deterministically even if
    // virtio-blk MMIO caps are granted later in bring-up.
    let mut engine =
        match JournalEngine::open(Backend::Mem(MemBlockDevice::new(BLOCK_SIZE, BLOCK_COUNT))) {
            Ok(engine) => engine,
            Err(err) => {
                emit_line("statefsd: journal open failed (mem)");
                emit_statefs_error(err);
                return Err(ServerError::Unsupported);
            }
        };
    // Track whether we've processed any mutating operations yet. We'll only "upgrade"
    // to the virtio-blk backend while still pristine, to avoid losing in-memory state.
    let mut pristine = true;
    let mut virtio_upgraded = false;
    let mut virtio_retry_count = 0u8;
    const VIRTIO_MAX_RETRIES: u8 = 5;

    notifier.notify();
    emit_line("statefsd: ready");

    loop {
        match server.recv_request_with_meta(Wait::Blocking) {
            Ok((frame, sender_service_id, reply)) => {
                if pristine && !virtio_upgraded && virtio_retry_count < VIRTIO_MAX_RETRIES {
                    let mut q = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
                    let mmio_ready = nexus_abi::cap_query(48, &mut q).is_ok() && q.kind_tag == 2;
                    if mmio_ready {
                        if let Ok(blk) = VirtioBlkDevice::new(48) {
                            emit_blk_marker(&blk);
                            match JournalEngine::open(Backend::Virtio(blk)) {
                                Ok(new_engine) => {
                                    engine = new_engine;
                                    virtio_upgraded = true;
                                    emit_line("statefsd: virtio upgrade ok");
                                }
                                Err(err) => {
                                    virtio_retry_count += 1;
                                    emit_line("statefsd: journal open failed (virtio)");
                                    emit_statefs_error(err);
                                    // Delay before next retry to let QEMU virtio settle
                                    if virtio_retry_count < VIRTIO_MAX_RETRIES {
                                        emit_line("statefsd: virtio retry scheduled");
                                        for _ in 0..100 {
                                            let _ = yield_();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                let rsp = handle_frame(&mut engine, sender_service_id, frame.as_slice());
                // Once we accept a mutating op, we no longer allow backend upgrade.
                if let Some(op) = frame.get(3).copied() {
                    if matches!(
                        op,
                        proto::OP_PUT | proto::OP_DEL | proto::OP_SYNC | proto::OP_REOPEN
                    ) {
                        pristine = false;
                    }
                }
                if let Some(reply) = reply {
                    if reply.reply_and_close(&rsp).is_err() {
                        emit_line("statefsd: reply send fail");
                    }
                } else {
                    if server.send(&rsp, Wait::Blocking).is_err() {
                        emit_line("statefsd: send fail");
                    }
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::PermissionDenied)) => {
                // Treat as transient; the control plane can still be settling during bring-up.
                let _ = yield_();
            }
            Err(err) => {
                emit_line("statefsd: ipc error");
                emit_ipc_error(err);
                return Err(ServerError::Unsupported);
            }
        }
    }
}

fn handle_frame(
    engine: &mut JournalEngine<Backend>,
    sender_service_id: u64,
    frame: &[u8],
) -> Vec<u8> {
    let op_hint = frame.get(3).copied().unwrap_or(proto::OP_GET);
    let (request, nonce) = match proto::decode_request_with_nonce(frame) {
        Ok(v) => v,
        Err(status) => return proto::encode_status_response_with_nonce(op_hint, status, None),
    };

    match request {
        Request::Put { key, value } => {
            if !policy_allows(sender_service_id, proto::OP_PUT, key) {
                emit_access_denied(key, sender_service_id);
                return proto::encode_status_response_with_nonce(
                    proto::OP_PUT,
                    proto::STATUS_ACCESS_DENIED,
                    nonce,
                );
            }
            match engine.put(key, value) {
                Ok(()) => {
                    proto::encode_status_response_with_nonce(proto::OP_PUT, proto::STATUS_OK, nonce)
                }
                Err(err) => proto::encode_status_response_with_nonce(
                    proto::OP_PUT,
                    proto::status_from_error(err),
                    nonce,
                ),
            }
        }
        Request::Get { key } => {
            if !policy_allows(sender_service_id, proto::OP_GET, key) {
                emit_access_denied(key, sender_service_id);
                return proto::encode_get_response_with_nonce(
                    proto::STATUS_ACCESS_DENIED,
                    &[],
                    nonce,
                );
            }
            match engine.get(key) {
                Ok(value) => proto::encode_get_response_with_nonce(proto::STATUS_OK, &value, nonce),
                Err(err) => {
                    proto::encode_get_response_with_nonce(proto::status_from_error(err), &[], nonce)
                }
            }
        }
        Request::Delete { key } => {
            if !policy_allows(sender_service_id, proto::OP_DEL, key) {
                emit_access_denied(key, sender_service_id);
                return proto::encode_status_response_with_nonce(
                    proto::OP_DEL,
                    proto::STATUS_ACCESS_DENIED,
                    nonce,
                );
            }
            match engine.delete(key) {
                Ok(()) => {
                    proto::encode_status_response_with_nonce(proto::OP_DEL, proto::STATUS_OK, nonce)
                }
                Err(err) => proto::encode_status_response_with_nonce(
                    proto::OP_DEL,
                    proto::status_from_error(err),
                    nonce,
                ),
            }
        }
        Request::List { prefix, limit } => {
            if !policy_allows(sender_service_id, proto::OP_LIST, prefix) {
                emit_access_denied(prefix, sender_service_id);
                return proto::encode_list_response_with_nonce(
                    proto::STATUS_ACCESS_DENIED,
                    &[],
                    MAX_LIST_RESPONSE_BYTES,
                    nonce,
                );
            }
            match engine.list(prefix, limit as usize) {
                Ok(keys) => proto::encode_list_response_with_nonce(
                    proto::STATUS_OK,
                    &keys,
                    MAX_LIST_RESPONSE_BYTES,
                    nonce,
                ),
                Err(err) => proto::encode_list_response_with_nonce(
                    proto::status_from_error(err),
                    &[],
                    MAX_LIST_RESPONSE_BYTES,
                    nonce,
                ),
            }
        }
        Request::Sync => {
            // Sync is a durability boundary for all writers. Allow if the caller has either:
            // - boot authority (`statefs.boot`) or
            // - generic state writer (`statefs.write`)
            let allowed = policyd_allows(sender_service_id, CAP_BOOT.as_bytes())
                || policyd_allows(sender_service_id, CAP_WRITE.as_bytes());
            if !allowed {
                emit_access_denied("/state", sender_service_id);
                return proto::encode_status_response_with_nonce(
                    proto::OP_SYNC,
                    proto::STATUS_ACCESS_DENIED,
                    nonce,
                );
            }
            match engine.sync() {
                Ok(()) => proto::encode_status_response_with_nonce(
                    proto::OP_SYNC,
                    proto::STATUS_OK,
                    nonce,
                ),
                Err(err) => proto::encode_status_response_with_nonce(
                    proto::OP_SYNC,
                    proto::status_from_error(err),
                    nonce,
                ),
            }
        }
        Request::Reopen => {
            let allowed = policyd_allows(sender_service_id, CAP_BOOT.as_bytes())
                || policyd_allows(sender_service_id, CAP_WRITE.as_bytes());
            if !allowed {
                emit_access_denied("/state", sender_service_id);
                return proto::encode_status_response_with_nonce(
                    proto::OP_REOPEN,
                    proto::STATUS_ACCESS_DENIED,
                    nonce,
                );
            }
            match engine.reopen() {
                Ok(()) => proto::encode_status_response_with_nonce(
                    proto::OP_REOPEN,
                    proto::STATUS_OK,
                    nonce,
                ),
                Err(err) => proto::encode_status_response_with_nonce(
                    proto::OP_REOPEN,
                    proto::status_from_error(err),
                    nonce,
                ),
            }
        }
    }
}

fn policy_allows(sender_service_id: u64, op: u8, path: &str) -> bool {
    let cap = required_cap(op, path);
    policyd_allows(sender_service_id, cap.as_bytes())
}

fn required_cap(op: u8, path: &str) -> &'static str {
    if path.starts_with("/state/keystore/") {
        CAP_KEYSTORE
    } else if path.starts_with("/state/boot/") {
        CAP_BOOT
    } else if matches!(op, proto::OP_PUT | proto::OP_DEL | proto::OP_SYNC | proto::OP_REOPEN) {
        CAP_WRITE
    } else {
        CAP_READ
    }
}

fn policyd_allows(subject_id: u64, cap: &[u8]) -> bool {
    const MAGIC0: u8 = b'P';
    const MAGIC1: u8 = b'O';
    const VERSION_V2: u8 = 2;
    const OP_CHECK_CAP_DELEGATED: u8 = 5;
    const STATUS_ALLOW: u8 = 0;

    if cap.is_empty() || cap.len() > 48 {
        return false;
    }
    // v2 delegated CAP check request (nonce-correlated):
    // [P, O, ver=2, OP_CHECK_CAP_DELEGATED, nonce:u32le, subject_id:u64le, cap_len:u8, cap...]
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

    // init-lite deterministic slots for statefsd:
    // - reply inbox: recv=5, send=6
    // - policyd send cap: 7
    const POL_SEND_SLOT: u32 = 0x07;
    const REPLY_RECV_SLOT: u32 = 0x05;
    const REPLY_SEND_SLOT: u32 = 0x06;
    let reply_send_clone = match nexus_abi::cap_clone(REPLY_SEND_SLOT) {
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
    let deadline = start.saturating_add(2_000_000_000);

    let mut i: usize = 0;
    loop {
        match nexus_abi::ipc_send_v1(POL_SEND_SLOT, &hdr, &frame, nexus_abi::IPC_SYS_NONBLOCK, 0) {
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
                let _ = yield_();
            }
            Err(_) => return false,
        }
        i = i.wrapping_add(1);
    }
    // Best-effort close: keep local cap table bounded even though the cap was moved.
    let _ = nexus_abi::cap_close(reply_send_clone);

    let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 16];
    let mut j: usize = 0;
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
            REPLY_RECV_SLOT,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                if n != 10 || buf[0] != MAGIC0 || buf[1] != MAGIC1 || buf[2] != VERSION_V2 {
                    continue;
                }
                if buf[3] != (OP_CHECK_CAP_DELEGATED | 0x80) {
                    continue;
                }
                let got_nonce = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
                if got_nonce != nonce {
                    continue;
                }
                return buf[8] == STATUS_ALLOW;
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => return false,
        }
        j = j.wrapping_add(1);
    }
}

fn emit_access_denied(path: &str, sender_service_id: u64) {
    let mut buf = [0u8; 160];
    let mut len = 0usize;
    let _ = push_bytes(&mut buf, &mut len, b"statefsd: access denied path=");
    let _ = push_bytes(&mut buf, &mut len, path.as_bytes());
    let _ = push_bytes(&mut buf, &mut len, b" sender=0x");
    write_hex_u64(&mut buf, &mut len, sender_service_id);
    let msg = core::str::from_utf8(&buf[..len]).unwrap_or("statefsd: access denied");
    emit_line(msg);
    append_logd_audit(msg.as_bytes());
}

fn emit_line(message: &str) {
    for byte in message.as_bytes().iter().copied().chain(core::iter::once(b'\n')) {
        let _ = debug_putc(byte);
    }
}

fn emit_statefs_error(err: StatefsError) {
    let msg = match err {
        StatefsError::NotFound => "statefsd: err not-found",
        StatefsError::AccessDenied => "statefsd: err access-denied",
        StatefsError::ValueTooLarge => "statefsd: err value-too-large",
        StatefsError::KeyTooLong => "statefsd: err key-too-long",
        StatefsError::IoError => "statefsd: err io",
        StatefsError::Corrupted => "statefsd: err corrupted",
        StatefsError::InvalidKey => "statefsd: err invalid-key",
        StatefsError::ReplayLimitExceeded => "statefsd: err replay-limit",
    };
    emit_line(msg);
}

fn emit_ipc_error(err: nexus_ipc::IpcError) {
    let msg = match err {
        nexus_ipc::IpcError::WouldBlock => "statefsd: ipc would-block",
        nexus_ipc::IpcError::Timeout => "statefsd: ipc timeout",
        nexus_ipc::IpcError::Disconnected => "statefsd: ipc disconnected",
        nexus_ipc::IpcError::NoSpace => "statefsd: ipc no-space",
        nexus_ipc::IpcError::Kernel(err) => match err {
            nexus_abi::IpcError::NoSuchEndpoint => "statefsd: ipc no-such-endpoint",
            nexus_abi::IpcError::QueueFull => "statefsd: ipc queue-full",
            nexus_abi::IpcError::QueueEmpty => "statefsd: ipc queue-empty",
            nexus_abi::IpcError::PermissionDenied => "statefsd: ipc permission-denied",
            nexus_abi::IpcError::TimedOut => "statefsd: ipc timed-out",
            nexus_abi::IpcError::NoSpace => "statefsd: ipc no-space",
            nexus_abi::IpcError::Unsupported => "statefsd: ipc unsupported",
        },
        nexus_ipc::IpcError::Unsupported => "statefsd: ipc unsupported",
        _ => "statefsd: ipc other",
    };
    emit_line(msg);
}

fn emit_blk_marker(dev: &VirtioBlkDevice) {
    let ss = dev.sector_size();
    let nsec = dev.capacity_sectors();
    emit_line("blk: virtio-blk up");
    let mut buf = [0u8; 64];
    let mut len = 0usize;
    let _ = push_bytes(&mut buf, &mut len, b"blk: virtio-blk up (ss=");
    push_u32(&mut buf, &mut len, ss);
    let _ = push_bytes(&mut buf, &mut len, b" nsec=");
    push_u64(&mut buf, &mut len, nsec);
    let _ = push_bytes(&mut buf, &mut len, b")");
    let msg = core::str::from_utf8(&buf[..len]).unwrap_or("blk: virtio-blk up");
    emit_line(msg);
}

fn push_bytes(buf: &mut [u8], len: &mut usize, bytes: &[u8]) -> bool {
    let available = buf.len().saturating_sub(*len);
    if bytes.len() > available {
        return false;
    }
    buf[*len..*len + bytes.len()].copy_from_slice(bytes);
    *len += bytes.len();
    true
}

fn write_hex_u64(buf: &mut [u8], len: &mut usize, value: u64) {
    if buf.len().saturating_sub(*len) < 16 {
        return;
    }
    for shift in (0..16).rev() {
        let nibble = ((value >> (shift * 4)) & 0xF) as u8;
        let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
        buf[*len] = ch;
        *len += 1;
    }
}

fn push_u32(buf: &mut [u8], len: &mut usize, value: u32) {
    push_u64(buf, len, value as u64);
}

fn push_u64(buf: &mut [u8], len: &mut usize, mut value: u64) {
    let mut tmp = [0u8; 20];
    let mut pos = 0usize;
    if value == 0 {
        tmp[0] = b'0';
        pos = 1;
    } else {
        while value > 0 && pos < tmp.len() {
            tmp[pos] = b'0' + (value % 10) as u8;
            value /= 10;
            pos += 1;
        }
        tmp[..pos].reverse();
    }
    let _ = push_bytes(buf, len, &tmp[..pos]);
}

fn append_logd_audit(msg: &[u8]) {
    const MAGIC0: u8 = b'L';
    const MAGIC1: u8 = b'O';
    const VERSION: u8 = 1;
    const OP_APPEND: u8 = 1;
    const LEVEL_INFO: u8 = 2;
    const SCOPE: &[u8] = b"statefsd.audit";

    if msg.len() > 256 || SCOPE.len() > 64 {
        return;
    }

    // init-lite deterministic slots for statefsd:
    // - logd send cap: 0x08
    // - reply inbox: recv=0x05, send=0x06
    let send_slot = 0x08;
    let reply_send_slot = 0x06;
    let _reply_recv_slot = 0x05;
    let reply_send_clone = match nexus_abi::cap_clone(reply_send_slot) {
        Ok(c) => c,
        Err(_) => return,
    };

    let mut frame = [0u8; 512];
    let mut len = 0usize;
    frame[len..len + 4].copy_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_APPEND]);
    len += 4;
    frame[len] = LEVEL_INFO;
    len += 1;
    frame[len] = SCOPE.len() as u8;
    len += 1;
    frame[len..len + 2].copy_from_slice(&(msg.len() as u16).to_le_bytes());
    len += 2;
    frame[len..len + 2].copy_from_slice(&0u16.to_le_bytes()); // fields_len
    len += 2;
    frame[len..len + SCOPE.len()].copy_from_slice(SCOPE);
    len += SCOPE.len();
    frame[len..len + msg.len()].copy_from_slice(msg);
    len += msg.len();

    let hdr =
        nexus_abi::MsgHeader::new(reply_send_clone, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, len as u32);
    let _ = nexus_abi::ipc_send_v1(send_slot, &hdr, &frame[..len], nexus_abi::IPC_SYS_NONBLOCK, 0);
    let _ = _reply_recv_slot;
}
