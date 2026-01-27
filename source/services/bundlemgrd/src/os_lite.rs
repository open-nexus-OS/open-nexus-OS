#![cfg(all(nexus_env = "os", feature = "os-lite"))]
//! CONTEXT: Bundlemgrd os-lite service loop
//! INTENT: Provide minimal bundle manager ops for bring-up and selftests
//! IDL (target): list, route_status, fetch_image, set_active_slot
//! DEPS: nexus-ipc, nexus-abi, nexus-log
//! READINESS: emit "bundlemgrd: ready" once service loop is live
//! TESTS: scripts/qemu-test.sh (selftest markers)

extern crate alloc;

use alloc::boxed::Box;

use core::fmt;

use nexus_abi::{debug_putc, yield_, MsgHeader};
use nexus_ipc::{KernelServer, Server as _, Wait};

/// Result type surfaced by the lite bundle manager shim.
pub type LiteResult<T> = Result<T, ServerError>;

/// Placeholder artifact store used by the shim backend.
#[derive(Clone, Copy, Debug, Default)]
pub struct ArtifactStore;

impl ArtifactStore {
    /// Creates a new artifact store placeholder.
    pub fn new() -> Self {
        Self
    }
}

/// Ready notifier invoked once the service finishes initialization.
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

/// Errors reported by the lite shim implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerError {
    /// Functionality not yet available in the os-lite path.
    Unsupported,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "bundlemgrd unsupported"),
        }
    }
}

/// No-op schema warmer retained for API parity.
pub fn touch_schemas() {}

const MAGIC0: u8 = nexus_abi::bundlemgrd::MAGIC0;
const MAGIC1: u8 = nexus_abi::bundlemgrd::MAGIC1;
const VERSION: u8 = nexus_abi::bundlemgrd::VERSION;

const OP_LIST: u8 = nexus_abi::bundlemgrd::OP_LIST;
const OP_ROUTE_STATUS: u8 = nexus_abi::bundlemgrd::OP_ROUTE_STATUS;
const OP_FETCH_IMAGE: u8 = nexus_abi::bundlemgrd::OP_FETCH_IMAGE;
const OP_SET_ACTIVE_SLOT: u8 = nexus_abi::bundlemgrd::OP_SET_ACTIVE_SLOT;
const OP_LOG_PROBE: u8 = 0x7f;

const STATUS_OK: u8 = nexus_abi::bundlemgrd::STATUS_OK;
const STATUS_MALFORMED: u8 = nexus_abi::bundlemgrd::STATUS_MALFORMED;
const STATUS_UNSUPPORTED: u8 = nexus_abi::bundlemgrd::STATUS_UNSUPPORTED;

const SLOT_A: u8 = 1;
const SLOT_B: u8 = 2;

static ACTIVE_SLOT: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(SLOT_A);

fn active_slot_label() -> u8 {
    match ACTIVE_SLOT.load(core::sync::atomic::Ordering::Relaxed) {
        SLOT_B => b'b',
        _ => b'a',
    }
}

/// Main service loop used by the lite shim.
pub fn service_main_loop(notifier: ReadyNotifier, _artifacts: ArtifactStore) -> LiteResult<()> {
    notifier.notify();
    emit_line("bundlemgrd: ready");
    let server = match KernelServer::new_for("bundlemgrd") {
        Ok(server) => server,
        Err(err) => {
            emit_line(match err {
                nexus_ipc::IpcError::Timeout => "bundlemgrd: route err timeout",
                nexus_ipc::IpcError::NoSpace => "bundlemgrd: route err nospace",
                nexus_ipc::IpcError::WouldBlock => "bundlemgrd: route err wouldblock",
                nexus_ipc::IpcError::Disconnected => "bundlemgrd: route err disconnected",
                nexus_ipc::IpcError::Unsupported => "bundlemgrd: route err unsupported",
                nexus_ipc::IpcError::Kernel(_) => "bundlemgrd: route err kernel",
                _ => "bundlemgrd: route err other",
            });
            emit_line("bundlemgrd: route fallback");
            KernelServer::new_with_slots(3, 4).map_err(|_| ServerError::Unsupported)?
        }
    };
    // TASK-0006: core service wiring proof (structured log via nexus-log -> logd).
    // Emit on first request (not at process start) so init-lite has time to provision logd/@reply routes.
    let mut probe_emitted = false;
    let mut logged_capmove = false;
    loop {
        match server.recv_request_with_meta(Wait::Blocking) {
            Ok((frame, sender_service_id, reply)) => {
                let _ = sender_service_id;
                if reply.is_some() && !logged_capmove {
                    logged_capmove = true;
                }
                if !probe_emitted {
                    probe_emitted = true;
                    nexus_log::info("bundlemgrd", |line| {
                        line.text("core service log probe: bundlemgrd");
                    });
                }
                let rsp = handle_frame_vec(frame.as_slice());
                if let Some(reply) = reply {
                    let _ = reply.reply_and_close_wait(&rsp, Wait::Blocking);
                } else {
                    let _ = server.send(&rsp, Wait::Blocking);
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(nexus_ipc::IpcError::Disconnected) => {
                emit_line("bundlemgrd: recv disconnected");
                return Err(ServerError::Unsupported);
            }
            Err(nexus_ipc::IpcError::NoSpace) => {
                emit_line("bundlemgrd: recv nospace");
                return Err(ServerError::Unsupported);
            }
            Err(nexus_ipc::IpcError::Unsupported) => {
                emit_line("bundlemgrd: recv unsupported");
                return Err(ServerError::Unsupported);
            }
            Err(nexus_ipc::IpcError::Kernel(_)) => {
                emit_line("bundlemgrd: recv kernel");
                return Err(ServerError::Unsupported);
            }
            Err(_) => {
                emit_line("bundlemgrd: recv other");
                return Err(ServerError::Unsupported);
            }
        }
    }
}

const CTRL_SEND_SLOT: u32 = 1;
const CTRL_RECV_SLOT: u32 = 2;

fn route_status(target: &str) -> Option<u8> {
    let name = target.as_bytes();
    // Routing v1 has no nonce; drain stale control replies to avoid consuming an unrelated ROUTE_RSP.
    for _ in 0..32 {
        let mut rh = MsgHeader::new(0, 0, 0, 0, 0);
        let mut tmp = [0u8; 32];
        match nexus_abi::ipc_recv_v1(
            CTRL_RECV_SLOT,
            &mut rh,
            &mut tmp,
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
    let hdr = MsgHeader::new(0, 0, 0, 0, req_len as u32);
    // Avoid deadline-based blocking IPC; use bounded NONBLOCK loops.
    let start = nexus_abi::nsec().ok()?;
    let deadline = start.saturating_add(2_000_000_000); // 2s
    let mut i: usize = 0;
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
                if (i & 0x7f) == 0 {
                    let now = nexus_abi::nsec().ok()?;
                    if now >= deadline {
                        return None;
                    }
                }
                let _ = yield_();
            }
            Err(_) => return None,
        }
        i = i.wrapping_add(1);
    }
    let mut rh = MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 32];
    let mut j: usize = 0;
    let n = loop {
        if (j & 0x7f) == 0 {
            let now = nexus_abi::nsec().ok()?;
            if now >= deadline {
                return None;
            }
        }
        match nexus_abi::ipc_recv_v1(
            CTRL_RECV_SLOT,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => break n as usize,
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => return None,
        }
        j = j.wrapping_add(1);
    };
    let (status, _send, _recv) = nexus_abi::routing::decode_route_rsp(&buf[..n])?;
    Some(status)
}

fn handle_frame(frame: &[u8]) -> [u8; 8] {
    // LIST request: [B, N, ver, OP_LIST]
    // LIST response: [B, N, ver, OP_LIST|0x80, status:u8, count:u16le, _reserved:u8]
    //
    // ROUTE_STATUS request: [B, N, ver, OP_ROUTE_STATUS, name_len:u8, name...]
    // ROUTE_STATUS response:
    //   [B, N, ver, OP_ROUTE_STATUS|0x80, status:u8, route_status:u8, _reserved:u8, _reserved:u8]
    //
    // FETCH_IMAGE request: [B, N, ver, OP_FETCH_IMAGE]
    // FETCH_IMAGE response: [B, N, ver, OP_FETCH_IMAGE|0x80, status:u8, len:u32le, bytes...]
    //
    // SET_ACTIVE_SLOT request: [B, N, ver, OP_SET_ACTIVE_SLOT, slot:u8]
    // SET_ACTIVE_SLOT response:
    //   [B, N, ver, OP_SET_ACTIVE_SLOT|0x80, status:u8, slot:u8, _reserved:u8, _reserved:u8]
    if frame.len() < 4 || frame[0] != MAGIC0 || frame[1] != MAGIC1 {
        return rsp(OP_LIST, STATUS_MALFORMED, 0);
    }
    if frame[2] != VERSION {
        return rsp(frame[3], STATUS_UNSUPPORTED, 0);
    }
    let op = frame[3];
    match op {
        OP_LOG_PROBE => {
            let ok = append_probe_to_logd();
            rsp(op, if ok { STATUS_OK } else { STATUS_UNSUPPORTED }, 0)
        }
        OP_LIST => {
            if frame.len() != 4 {
                return rsp(op, STATUS_MALFORMED, 0);
            }
            // Bring-up: one deterministic bundle image is available.
            rsp(op, STATUS_OK, 1)
        }
        OP_ROUTE_STATUS => {
            if frame.len() < 5 {
                return rsp2(op, STATUS_MALFORMED, 0);
            }
            let n = frame[4] as usize;
            if n == 0 || n > nexus_abi::routing::MAX_SERVICE_NAME_LEN || frame.len() != 5 + n {
                return rsp2(op, STATUS_MALFORMED, 0);
            }
            let name = core::str::from_utf8(&frame[5..]).unwrap_or("");
            let code = route_status(name).unwrap_or(nexus_abi::routing::STATUS_MALFORMED);
            rsp2(op, STATUS_OK, code)
        }
        OP_FETCH_IMAGE => {
            if frame.len() != 4 {
                return rsp(op, STATUS_MALFORMED, 0);
            }
            // For now, we return OK and let the caller fetch the static image from a separate
            // service endpoint (see handle_frame_vec).
            rsp(op, STATUS_OK, 0)
        }
        OP_SET_ACTIVE_SLOT => {
            if frame.len() != 5 {
                return rsp2(op, STATUS_MALFORMED, 0);
            }
            let slot = frame[4];
            if slot != SLOT_A && slot != SLOT_B {
                return rsp2(op, STATUS_MALFORMED, 0);
            }
            ACTIVE_SLOT.store(slot, core::sync::atomic::Ordering::Relaxed);
            emit_line(if slot == SLOT_A {
                "bundlemgrd: slot a active"
            } else {
                "bundlemgrd: slot b active"
            });
            rsp2(op, STATUS_OK, slot)
        }
        _ => rsp(op, STATUS_UNSUPPORTED, 0),
    }
}

fn handle_frame_vec(frame: &[u8]) -> alloc::vec::Vec<u8> {
    use alloc::vec::Vec;

    if frame.len() >= 4 && frame[0] == MAGIC0 && frame[1] == MAGIC1 && frame[2] == VERSION {
        if frame[3] == OP_FETCH_IMAGE {
            let slot = active_slot_label();
            let version = if slot == b'a' { b"1.0.0-a" } else { b"1.0.0-b" };
            let mut build_prop = Vec::new();
            build_prop.extend_from_slice(b"ro.nexus.build=dev\nro.nexus.slot=");
            build_prop.push(slot);
            build_prop.push(b'\n');
            // Encode image inline (small and deterministic).
            let mut img = Vec::new();
            img.extend_from_slice(b"NXBI");
            img.push(1); // VERSION
            img.extend_from_slice(&1u16.to_le_bytes()); // entry_count
                                                        // entry:
            img.push(6); // "system"
            img.extend_from_slice(b"system");
            img.push(version.len() as u8);
            img.extend_from_slice(version);
            let path = b"build.prop";
            img.extend_from_slice(&(path.len() as u16).to_le_bytes());
            img.extend_from_slice(path);
            img.extend_from_slice(&0u16.to_le_bytes()); // KIND_FILE
            img.extend_from_slice(&(build_prop.len() as u32).to_le_bytes());
            img.extend_from_slice(&build_prop);

            let mut out = Vec::with_capacity(9 + img.len());
            out.push(MAGIC0);
            out.push(MAGIC1);
            out.push(VERSION);
            out.push(OP_FETCH_IMAGE | 0x80);
            out.push(STATUS_OK);
            out.extend_from_slice(&(img.len() as u32).to_le_bytes());
            out.extend_from_slice(&img);
            return out;
        }
    }
    // Fallback: fixed-size responses.
    let rsp = handle_frame(frame);
    let mut out = Vec::with_capacity(rsp.len());
    out.extend_from_slice(&rsp);
    out
}

fn rsp(op: u8, status: u8, count: u16) -> [u8; 8] {
    let mut out = [0u8; 8];
    out[0] = MAGIC0;
    out[1] = MAGIC1;
    out[2] = VERSION;
    out[3] = op | 0x80;
    out[4] = status;
    out[5..7].copy_from_slice(&count.to_le_bytes());
    out[7] = 0;
    out
}

fn rsp2(op: u8, status: u8, route_status: u8) -> [u8; 8] {
    let mut out = [0u8; 8];
    out[0] = MAGIC0;
    out[1] = MAGIC1;
    out[2] = VERSION;
    out[3] = op | 0x80;
    out[4] = status;
    out[5] = route_status;
    out[6] = 0;
    out[7] = 0;
    out
}

fn append_probe_to_logd() -> bool {
    const MAGIC0: u8 = b'L';
    const MAGIC1: u8 = b'O';
    const VERSION: u8 = 1;
    const OP_APPEND: u8 = 1;
    const LEVEL_INFO: u8 = 2;

    let logd = match nexus_ipc::KernelClient::new_for("logd") {
        Ok(c) => c,
        Err(_) => return false,
    };
    let reply = match nexus_ipc::KernelClient::new_for("@reply") {
        Ok(c) => c,
        Err(_) => return false,
    };
    let (reply_send, _reply_recv) = reply.slots();
    let moved = match nexus_abi::cap_clone(reply_send) {
        Ok(slot) => slot,
        Err(_) => return false,
    };

    let scope: &[u8] = b"bundlemgrd";
    let msg: &[u8] = b"core service log probe: bundlemgrd";
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

fn emit_line(message: &str) {
    for byte in message.as_bytes().iter().copied().chain(core::iter::once(b'\n')) {
        let _ = debug_putc(byte);
    }
}

#[cfg(all(test, nexus_env = "os", feature = "os-lite"))]
mod tests {
    use super::*;

    fn build_req(op: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, op]);
        v.extend_from_slice(payload);
        v
    }

    #[test]
    fn test_list_v1_ok() {
        let rsp = handle_frame_vec(&build_req(OP_LIST, &[]));
        assert_eq!(rsp.len(), 8);
        assert_eq!(rsp[0], MAGIC0);
        assert_eq!(rsp[1], MAGIC1);
        assert_eq!(rsp[2], VERSION);
        assert_eq!(rsp[3], OP_LIST | 0x80);
        assert_eq!(rsp[4], STATUS_OK);
        // count=u16le at bytes 5..7
        assert_eq!(u16::from_le_bytes(rsp[5..7].try_into().unwrap()), 1);
    }

    #[test]
    fn test_set_active_slot_updates_fetch_image_slot_marker() {
        // Set slot B.
        let rsp = handle_frame_vec(&build_req(OP_SET_ACTIVE_SLOT, &[SLOT_B]));
        assert_eq!(rsp.len(), 8);
        assert_eq!(rsp[3], OP_SET_ACTIVE_SLOT | 0x80);
        assert_eq!(rsp[4], STATUS_OK);
        assert_eq!(rsp[5], SLOT_B);

        // Fetch image and verify build.prop reflects slot b.
        let img = handle_frame_vec(&build_req(OP_FETCH_IMAGE, &[]));
        assert!(img.len() > 16);
        assert_eq!(img[0], MAGIC0);
        assert_eq!(img[1], MAGIC1);
        assert_eq!(img[2], VERSION);
        assert_eq!(img[3], OP_FETCH_IMAGE | 0x80);
        assert_eq!(img[4], STATUS_OK);
        let n = u32::from_le_bytes(img[5..9].try_into().unwrap()) as usize;
        assert_eq!(img.len(), 9 + n);

        let payload = &img[9..];
        assert!(
            payload.windows(b"ro.nexus.slot=b\n".len()).any(|w| w == b"ro.nexus.slot=b\n"),
            "expected build.prop to include ro.nexus.slot=b"
        );
    }

    #[test]
    fn test_reject_malformed_set_active_slot_value() {
        let rsp = handle_frame_vec(&build_req(OP_SET_ACTIVE_SLOT, &[0xFF]));
        assert_eq!(rsp.len(), 8);
        assert_eq!(rsp[3], OP_SET_ACTIVE_SLOT | 0x80);
        assert_eq!(rsp[4], STATUS_MALFORMED);
    }

    #[test]
    fn test_reject_malformed_route_status_frame_sizes() {
        // Too short
        let rsp = handle_frame_vec(&build_req(OP_ROUTE_STATUS, &[]));
        assert_eq!(rsp.len(), 8);
        assert_eq!(rsp[3], OP_ROUTE_STATUS | 0x80);
        assert_eq!(rsp[4], STATUS_MALFORMED);

        // name_len present but missing bytes
        let rsp = handle_frame_vec(&build_req(OP_ROUTE_STATUS, &[3, b'a', b'b']));
        assert_eq!(rsp[4], STATUS_MALFORMED);
    }
}
