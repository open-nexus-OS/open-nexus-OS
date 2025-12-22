#![cfg(all(nexus_env = "os", feature = "os-lite"))]

extern crate alloc;

use alloc::boxed::Box;

use core::fmt;

use nexus_abi::{debug_putc, yield_};
use nexus_ipc::{KernelServer, Server as _, Wait};

/// Result alias used by the lite policyd backend.
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

/// Errors surfaced by the lite policyd backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerError {
    /// Placeholder for the unimplemented runtime.
    Unsupported,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "policyd unsupported"),
        }
    }
}

/// Schema warmer placeholder for interface parity.
pub fn touch_schemas() {}

const MAGIC0: u8 = b'P';
const MAGIC1: u8 = b'O';
const VERSION: u8 = 1;

const OP_CHECK: u8 = 1;
const OP_ROUTE: u8 = 2;
const OP_EXEC: u8 = 3;

const STATUS_ALLOW: u8 = 0;
const STATUS_DENY: u8 = 1;
const STATUS_MALFORMED: u8 = 2;
const STATUS_UNSUPPORTED: u8 = 3;

/// Minimal kernel-IPC backed policyd loop.
///
/// NOTE: This is a bring-up implementation: it only supports an allow/deny check over IPC and
/// returns a deterministic decision:
/// - allow if subject != "demo.testsvc"
/// - deny if subject == "demo.testsvc"
pub fn service_main_loop(notifier: ReadyNotifier) -> LiteResult<()> {
    notifier.notify();
    emit_line("policyd: ready");
    let server = KernelServer::new_for("policyd").map_err(|_| ServerError::Unsupported)?;
    // Private init-lite -> policyd control channels.
    // Slot layout for policyd child (deterministic under current init-lite bring-up):
    // - slot 1/2: init-lite routing control REQ/RSP
    // - slot 3/4: selftest-client <-> policyd service RECV/SEND
    // - slot 5/6: init-lite <-> policyd route control RECV/SEND
    // - slot 7/8: init-lite <-> policyd exec control RECV/SEND
    let ctl_route = KernelServer::new_with_slots(5, 6).map_err(|_| ServerError::Unsupported)?;
    let ctl_exec = KernelServer::new_with_slots(7, 8).map_err(|_| ServerError::Unsupported)?;
    let init_lite_id = nexus_abi::service_id_from_name(b"init-lite");
    loop {
        // Multiplex both endpoints without blocking on one of them.
        let mut progressed = false;

        match ctl_route.recv_with_header_meta(Wait::NonBlocking) {
            Ok((_hdr, sender_service_id, frame)) => {
                progressed = true;
                let rsp = handle_frame(frame.as_slice(), sender_service_id, sender_service_id == init_lite_id);
                let _ = ctl_route.send(&rsp.buf[..rsp.len], Wait::Blocking);
            }
            Err(nexus_ipc::IpcError::WouldBlock) => {}
            Err(nexus_ipc::IpcError::Timeout) => {}
            Err(nexus_ipc::IpcError::Disconnected) => return Err(ServerError::Unsupported),
            Err(_) => return Err(ServerError::Unsupported),
        }

        match ctl_exec.recv_with_header_meta(Wait::NonBlocking) {
            Ok((_hdr, sender_service_id, frame)) => {
                progressed = true;
                let rsp = handle_frame(frame.as_slice(), sender_service_id, sender_service_id == init_lite_id);
                let _ = ctl_exec.send(&rsp.buf[..rsp.len], Wait::Blocking);
            }
            Err(nexus_ipc::IpcError::WouldBlock) => {}
            Err(nexus_ipc::IpcError::Timeout) => {}
            Err(nexus_ipc::IpcError::Disconnected) => return Err(ServerError::Unsupported),
            Err(_) => return Err(ServerError::Unsupported),
        }

        match server.recv_with_header_meta(Wait::NonBlocking) {
            Ok((_hdr, sender_service_id, frame)) => {
                progressed = true;
                let rsp = handle_frame(frame.as_slice(), sender_service_id, sender_service_id == init_lite_id);
                let _ = server.send(&rsp.buf[..rsp.len], Wait::Blocking);
            }
            Err(nexus_ipc::IpcError::WouldBlock) => {}
            Err(nexus_ipc::IpcError::Timeout) => {}
            Err(nexus_ipc::IpcError::Disconnected) => return Err(ServerError::Unsupported),
            Err(_) => return Err(ServerError::Unsupported),
        }

        if !progressed {
            let _ = yield_();
        }
    }
}

struct FrameOut {
    buf: [u8; 10],
    len: usize,
}

fn handle_frame(frame: &[u8], sender_service_id: u64, privileged_proxy: bool) -> FrameOut {
    // v1 CHECK request: [P, O, ver=1, OP_CHECK, name_len:u8, name...]
    // v1 ROUTE request: [P, O, ver=1, OP_ROUTE, req_len:u8, req..., tgt_len:u8, tgt...]
    // v1 EXEC request:  [P, O, ver=1, OP_EXEC, req_len:u8, req..., image_id:u8]
    // v1 response:      [P, O, ver=1, op|0x80, status:u8, _reserved:u8]
    //
    // v2 ROUTE request: [P, O, ver=2, OP_ROUTE, nonce:u32le, req_len:u8, req..., tgt_len:u8, tgt...]
    // v2 EXEC request:  [P, O, ver=2, OP_EXEC,  nonce:u32le, req_len:u8, req..., image_id:u8]
    // v2 response:      [P, O, ver=2, op|0x80, nonce:u32le, status:u8, _reserved:u8]
    if frame.len() < 6 || frame[0] != MAGIC0 || frame[1] != MAGIC1 {
        return rsp_v1(OP_CHECK, STATUS_MALFORMED);
    }
    let ver = frame[2];
    let op = frame[3];
    match (ver, op) {
        (VERSION, OP_CHECK) => {
            let n = frame[4] as usize;
            if frame.len() != 5 + n {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let name = core::str::from_utf8(&frame[5..]).unwrap_or("");
            if name == "demo.testsvc" {
                rsp_v1(op, STATUS_DENY)
            } else {
                rsp_v1(op, STATUS_ALLOW)
            }
        }
        (VERSION, OP_ROUTE) => {
            if frame.len() < 7 {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let req_len = frame[4] as usize;
            if frame.len() < 5 + req_len + 1 {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let req_start = 5;
            let req_end = req_start + req_len;
            let tgt_len = frame[req_end] as usize;
            let tgt_start = req_end + 1;
            let tgt_end = tgt_start + tgt_len;
            if frame.len() != tgt_end {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let requester = core::str::from_utf8(&frame[req_start..req_end]).unwrap_or("");
            let target = core::str::from_utf8(&frame[tgt_start..tgt_end]).unwrap_or("");
            if requester == "demo.testsvc" || (requester == "bundlemgrd" && target == "execd") {
                rsp_v1(op, STATUS_DENY)
            } else {
                rsp_v1(op, STATUS_ALLOW)
            }
        }
        (VERSION, OP_EXEC) => {
            if frame.len() < 6 + 1 {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let req_len = frame[4] as usize;
            if req_len == 0 || req_len > 48 || frame.len() != 6 + req_len {
                return rsp_v1(op, STATUS_MALFORMED);
            }
            let requester = core::str::from_utf8(&frame[5..5 + req_len]).unwrap_or("");
            let _image_id = frame[5 + req_len]; // reserved
            if requester == "demo.testsvc" {
                rsp_v1(op, STATUS_DENY)
            } else {
                rsp_v1(op, STATUS_ALLOW)
            }
        }
        (nexus_abi::policyd::VERSION_V2, nexus_abi::policyd::OP_ROUTE) => {
            let (nonce, requester, target) = match nexus_abi::policyd::decode_route_v2(frame) {
                Some(v) => v,
                None => return rsp_v2(nexus_abi::policyd::OP_ROUTE, 0, STATUS_MALFORMED),
            };
            if !privileged_proxy && nexus_abi::service_id_from_name(requester) != sender_service_id {
                return rsp_v2(nexus_abi::policyd::OP_ROUTE, nonce, STATUS_DENY);
            }
            let requester = core::str::from_utf8(requester).unwrap_or("");
            let target = core::str::from_utf8(target).unwrap_or("");
            let status = if requester == "demo.testsvc" || (requester == "bundlemgrd" && target == "execd") {
                STATUS_DENY
            } else {
                STATUS_ALLOW
            };
            rsp_v2(nexus_abi::policyd::OP_ROUTE, nonce, status)
        }
        (nexus_abi::policyd::VERSION_V3, nexus_abi::policyd::OP_ROUTE) => {
            let (nonce, requester_id, target_id) = match nexus_abi::policyd::decode_route_v3_id(frame) {
                Some(v) => v,
                None => return rsp_v2(nexus_abi::policyd::OP_ROUTE, 0, STATUS_MALFORMED),
            };
            if !privileged_proxy && requester_id != sender_service_id {
                let buf = nexus_abi::policyd::encode_rsp_v3(nexus_abi::policyd::OP_ROUTE, nonce, STATUS_DENY);
                return FrameOut { buf, len: 10 };
            }
            let deny_demo = nexus_abi::service_id_from_name(b"demo.testsvc");
            let deny_bundle = nexus_abi::service_id_from_name(b"bundlemgrd");
            let deny_target = nexus_abi::service_id_from_name(b"execd");
            let status = if requester_id == deny_demo || (requester_id == deny_bundle && target_id == deny_target) {
                STATUS_DENY
            } else {
                STATUS_ALLOW
            };
            let buf = nexus_abi::policyd::encode_rsp_v3(nexus_abi::policyd::OP_ROUTE, nonce, status);
            FrameOut { buf, len: 10 }
        }
        (nexus_abi::policyd::VERSION_V2, nexus_abi::policyd::OP_EXEC) => {
            let (nonce, requester, _image_id) = match nexus_abi::policyd::decode_exec_v2(frame) {
                Some(v) => v,
                None => return rsp_v2(nexus_abi::policyd::OP_EXEC, 0, STATUS_MALFORMED),
            };
            if !privileged_proxy && nexus_abi::service_id_from_name(requester) != sender_service_id {
                return rsp_v2(nexus_abi::policyd::OP_EXEC, nonce, STATUS_DENY);
            }
            let requester = core::str::from_utf8(requester).unwrap_or("");
            let status = if requester == "demo.testsvc" { STATUS_DENY } else { STATUS_ALLOW };
            rsp_v2(nexus_abi::policyd::OP_EXEC, nonce, status)
        }
        (nexus_abi::policyd::VERSION_V3, nexus_abi::policyd::OP_EXEC) => {
            let (nonce, requester_id, _image_id) = match nexus_abi::policyd::decode_exec_v3_id(frame) {
                Some(v) => v,
                None => return rsp_v2(nexus_abi::policyd::OP_EXEC, 0, STATUS_MALFORMED),
            };
            if !privileged_proxy && requester_id != sender_service_id {
                let buf = nexus_abi::policyd::encode_rsp_v3(nexus_abi::policyd::OP_EXEC, nonce, STATUS_DENY);
                return FrameOut { buf, len: 10 };
            }
            let deny_demo = nexus_abi::service_id_from_name(b"demo.testsvc");
            let status = if requester_id == deny_demo { STATUS_DENY } else { STATUS_ALLOW };
            let buf = nexus_abi::policyd::encode_rsp_v3(nexus_abi::policyd::OP_EXEC, nonce, status);
            FrameOut { buf, len: 10 }
        }
        _ => rsp_v1(op, STATUS_UNSUPPORTED),
    }
}

fn rsp_v1(op: u8, status: u8) -> FrameOut {
    let mut buf = [0u8; 10];
    buf[..6].copy_from_slice(&[MAGIC0, MAGIC1, VERSION, op | 0x80, status, 0]);
    FrameOut { buf, len: 6 }
}

fn rsp_v2(op: u8, nonce: nexus_abi::policyd::Nonce, status: u8) -> FrameOut {
    let buf = nexus_abi::policyd::encode_rsp_v2(op, nonce, status);
    FrameOut { buf, len: 10 }
}

/// Stub transport runner retained for cross-module linkage.
pub fn run_with_transport_ready<T>(_: &mut T, notifier: ReadyNotifier) -> LiteResult<()> {
    notifier.notify();
    emit_line("policyd: ready (stub transport)");
    Err(ServerError::Unsupported)
}

fn emit_line(message: &str) {
    for byte in message
        .as_bytes()
        .iter()
        .copied()
        .chain(core::iter::once(b'\n'))
    {
        let _ = debug_putc(byte);
    }
}
