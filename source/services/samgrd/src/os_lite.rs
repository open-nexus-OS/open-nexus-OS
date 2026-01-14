#![cfg(all(nexus_env = "os", feature = "os-lite"))]

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use core::fmt;

use nexus_abi::{cap_close, debug_putc, yield_};
use nexus_ipc::{KernelClient, KernelServer, Server as _, Wait};

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
    emit_line("samgrd: ready");
    let server = KernelServer::new_for("samgrd").map_err(|_| ServerError::Unsupported)?;
    // Identity-binding hardening (bring-up semantics):
    //
    // Samgrd v1 currently moves *slot numbers* around (not endpoint caps), which is not a secure
    // global service registry. To avoid ambient/global poisoning, we scope registrations to the
    // kernel-derived sender service identity.
    //
    // This keeps the selftests honest (register/lookup roundtrip) while preventing one service
    // from registering entries that another service will observe.
    let mut registry: BTreeMap<(u64, Vec<u8>), (u32, u32)> = BTreeMap::new();
    loop {
        match server.recv_with_header_meta(Wait::Blocking) {
            Ok((hdr, sender_service_id, frame)) => {
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
                    if (hdr.flags & nexus_abi::ipc_hdr::CAP_MOVE) != 0 {
                        let _ = KernelServer::send_on_cap(hdr.src, &rsp);
                        let _ = cap_close(hdr.src as u32);
                    } else {
                        let _ = server.send(&rsp, Wait::Blocking);
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
                    // Reply on the moved cap slot (allocated into this process as hdr.src).
                    // Always best-effort and non-blocking for bring-up.
                    let _ = KernelServer::send_on_cap(hdr.src, b"PONG");
                    let _ = cap_close(hdr.src as u32);
                    continue;
                }

                // Sender attribution probe: reply with observed sender pid (hdr.dst).
                if frame.len() >= 4
                    && frame[0] == MAGIC0
                    && frame[1] == MAGIC1
                    && frame[2] == VERSION
                    && frame[3] == OP_SENDER_PID
                    && (hdr.flags & nexus_abi::ipc_hdr::CAP_MOVE) != 0
                {
                    let mut rsp = [0u8; 9];
                    rsp[0] = MAGIC0;
                    rsp[1] = MAGIC1;
                    rsp[2] = VERSION;
                    rsp[3] = OP_SENDER_PID | 0x80;
                    rsp[4] = STATUS_OK;
                    rsp[5..9].copy_from_slice(&hdr.dst.to_le_bytes());
                    let _ = KernelServer::send_on_cap(hdr.src, &rsp);
                    let _ = cap_close(hdr.src as u32);
                    continue;
                }

                // Sender service-id attribution probe: reply with kernel-derived sender service id.
                if frame.len() >= 4
                    && frame[0] == MAGIC0
                    && frame[1] == MAGIC1
                    && frame[2] == VERSION
                    && frame[3] == OP_SENDER_SERVICE_ID
                    && (hdr.flags & nexus_abi::ipc_hdr::CAP_MOVE) != 0
                {
                    let mut rsp = [0u8; 13];
                    rsp[0] = MAGIC0;
                    rsp[1] = MAGIC1;
                    rsp[2] = VERSION;
                    rsp[3] = OP_SENDER_SERVICE_ID | 0x80;
                    rsp[4] = STATUS_OK;
                    rsp[5..13].copy_from_slice(&sender_service_id.to_le_bytes());
                    let _ = KernelServer::send_on_cap(hdr.src, &rsp);
                    let _ = cap_close(hdr.src as u32);
                    continue;
                }

                let rsp = handle_frame(&mut registry, sender_service_id, frame.as_slice());
                // If a reply cap was moved, reply on it and close it.
                if (hdr.flags & nexus_abi::ipc_hdr::CAP_MOVE) != 0 {
                    let _ = KernelServer::send_on_cap(hdr.src, &rsp);
                    let _ = cap_close(hdr.src as u32);
                } else {
                    let _ = server.send(&rsp, Wait::Blocking);
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(nexus_ipc::IpcError::Disconnected) => return Err(ServerError::Unsupported),
            Err(_) => return Err(ServerError::Unsupported),
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
            // Response uses the common 13-byte shape:
            //   [S, M, ver, OP_RESOLVE_STATUS|0x80, status, 0,0,0,0,0,0,0,0]
            //
            // Security note (TASK-0005): this op returns ONLY status, never capability slots.
            let n = frame[4] as usize;
            if n == 0 || n > nexus_abi::routing::MAX_SERVICE_NAME_LEN || frame.len() != 5 + n {
                return rsp(op, STATUS_MALFORMED, 0, 0);
            }
            let name = &frame[5..];
            // Bring-up semantics (TASK-0005): resolve is a *status* API (no capability transfer).
            //
            // Under os-lite bring-up, not every service has full routing to every other service yet.
            // For RESOLVE_STATUS we therefore answer based on a bounded allowlist of known core services
            // that are expected to be present in the image.
            let ok = matches!(
                name,
                b"keystored"
                    | b"policyd"
                    | b"samgrd"
                    | b"bundlemgrd"
                    | b"packagefsd"
                    | b"vfsd"
                    | b"execd"
                    | b"netstackd"
                    | b"dsoftbusd"
            );
            if ok {
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
    for byte in message.as_bytes().iter().copied().chain(core::iter::once(b'\n')) {
        let _ = debug_putc(byte);
    }
}

fn append_probe_to_logd() -> bool {
    const MAGIC0: u8 = b'L';
    const MAGIC1: u8 = b'O';
    const VERSION: u8 = 1;
    const OP_APPEND: u8 = 1;
    const LEVEL_INFO: u8 = 2;

    let logd = match KernelClient::new_for("logd") {
        Ok(c) => c,
        Err(_) => return false,
    };
    let reply = match KernelClient::new_for("@reply") {
        Ok(c) => c,
        Err(_) => return false,
    };
    let (reply_send, _reply_recv) = reply.slots();
    let moved = match nexus_abi::cap_clone(reply_send) {
        Ok(slot) => slot,
        Err(_) => return false,
    };

    let scope: &[u8] = b"samgrd";
    let msg: &[u8] = b"core service log probe: samgrd";
    if scope.len() > 64 || msg.len() > 256 {
        return false;
    }

    let mut frame = alloc::vec::Vec::with_capacity(4 + 1 + 1 + 2 + 2 + scope.len() + msg.len());
    frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_APPEND]);
    frame.push(LEVEL_INFO);
    frame.push(scope.len() as u8);
    frame.extend_from_slice(&(msg.len() as u16).to_le_bytes());
    frame.extend_from_slice(&0u16.to_le_bytes()); // fields_len
    frame.extend_from_slice(scope);
    frame.extend_from_slice(msg);

    logd.send_with_cap_move_wait(&frame, moved, Wait::NonBlocking).is_ok()
}
