#![cfg(all(nexus_env = "os", feature = "os-lite"))]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

use core::fmt;

use nexus_abi::{debug_putc, exec, service_id_from_name, wait, yield_, Pid};
use nexus_ipc::{KernelServer, Server as _, Wait};

use demo_exit0::{DEMO_EXIT0_ELF, DEMO_EXIT42_ELF};
use exec_payloads::HELLO_ELF;
use nexus_log as nexus_log;

/// Result alias surfaced by the lite execd backend.
pub type LiteResult<T> = Result<T, ServerError>;

/// Restart policy for launched services.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestartPolicy {
    /// Never restart the service after exit.
    Never,
    /// Always restart the service when it exits.
    Always,
}

/// Ready notifier invoked once execd finishes initialization.
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

/// Errors surfaced by the lite execd backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerError {
    /// Placeholder error until the lite backend is implemented.
    Unsupported,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "execd unsupported"),
        }
    }
}

/// Errors returned by exec helpers on the lite backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecError {
    /// Functionality not available yet.
    Unsupported,
}

impl fmt::Display for ExecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "exec unsupported"),
        }
    }
}

/// Schema warmer placeholder retained for parity.
pub fn touch_schemas() {}

const MAGIC0: u8 = b'E';
const MAGIC1: u8 = b'X';
const VERSION: u8 = 1;

const OP_EXEC_IMAGE: u8 = 1;
const OP_REPORT_EXIT: u8 = 2;
const OP_WAIT_PID: u8 = 3;

const IMG_HELLO: u8 = 1;
const IMG_EXIT0: u8 = 2;
const IMG_EXIT42: u8 = 3;

const STATUS_OK: u8 = 0;
const STATUS_MALFORMED: u8 = 1;
const STATUS_UNSUPPORTED: u8 = 2;
const STATUS_FAILED: u8 = 3;
const STATUS_DENIED: u8 = 4;

/// Stubbed service loop that reports readiness and yields forever.
pub fn service_main_loop(notifier: ReadyNotifier) -> LiteResult<()> {
    notifier.notify();
    emit_line("execd: ready");
    let server = KernelServer::new_for("execd").map_err(|_| ServerError::Unsupported)?;
    let mut state = State::new();
    loop {
        match server.recv_with_header_meta(Wait::Blocking) {
            Ok((_hdr, sender_service_id, frame)) => {
                let rsp = handle_frame(&mut state, sender_service_id, frame.as_slice());
                let _ = server.send(rsp.as_slice(), Wait::Blocking);
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(nexus_ipc::IpcError::Disconnected) => return Err(ServerError::Unsupported),
            Err(_) => return Err(ServerError::Unsupported),
        }
    }
}

struct TrackedChild {
    pid: u32,
    image_id: u8,
}

struct State {
    children: Vec<TrackedChild>,
}

impl State {
    fn new() -> Self {
        Self { children: Vec::new() }
    }

    fn track_child(&mut self, pid: u32, image_id: u8) {
        // Keep bounded to avoid unbounded memory; drop oldest.
        const MAX: usize = 16;
        if self.children.len() >= MAX {
            self.children.remove(0);
        }
        self.children.push(TrackedChild { pid, image_id });
    }

    fn child_name(image_id: u8) -> &'static str {
        match image_id {
            IMG_HELLO => "demo.hello",
            IMG_EXIT0 => "demo.exit0",
            IMG_EXIT42 => "demo.exit42",
            _ => "unknown",
        }
    }

    fn log_crash_via_nexus_log(&mut self, pid: u32, code: i32, name: &str) {
        // Best-effort: if sink-logd isn't routable, it will fall back to UART-only.
        // Keep the message bounded; sink-logd enforces v1 caps.
        nexus_log::warn("execd", |line| {
            line.text("crash pid=");
            line.dec(pid as u64);
            line.text(" code=");
            // No signed integer helper; emit negative with a prefix.
            if code < 0 {
                line.text("-");
                line.dec((-code) as u64);
            } else {
                line.dec(code as u64);
            }
            line.text(" name=");
            line.text(name);
        });
    }
}

fn handle_frame(state: &mut State, sender_service_id: u64, frame: &[u8]) -> Vec<u8> {
    // Request v1: [E, X, ver, op, image_id, stack_pages:u8, requester_len:u8, requester...]
    // Response:   [E, X, ver, op|0x80, status, pid:u32le]
    if frame.len() < 4 || frame[0] != MAGIC0 || frame[1] != MAGIC1 {
        return rsp(OP_EXEC_IMAGE, STATUS_MALFORMED, 0).to_vec();
    }
    if frame[2] != VERSION {
        return rsp(frame[3], STATUS_UNSUPPORTED, 0).to_vec();
    }
    let op = frame[3];
    if op == OP_WAIT_PID {
        // Wait v1: [E,X,ver,OP_WAIT_PID, pid:u32le]
        // Rsp: [E,X,ver,OP_WAIT_PID|0x80, status:u8, pid:u32le, code:i32le]
        if frame.len() != 8 {
            return rsp(op, STATUS_MALFORMED, 0).to_vec();
        }
        let pid = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]) as i32;
        return match wait(pid) {
            Ok((got, code)) => {
                let got_u32 = got as u32;
                if code != 0 {
                    let image_id =
                        state.children.iter().find(|c| c.pid == got_u32).map(|c| c.image_id);
                    let name = image_id.map(State::child_name).unwrap_or("unknown");
                    emit_crash_marker(got_u32, code, name);
                    state.log_crash_via_nexus_log(got_u32, code, name);
                }
                let mut out = Vec::with_capacity(13);
                out.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_WAIT_PID | 0x80, STATUS_OK]);
                out.extend_from_slice(&(got as u32).to_le_bytes());
                out.extend_from_slice(&code.to_le_bytes());
                out
            }
            Err(_) => {
                let mut out = Vec::with_capacity(13);
                out.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_WAIT_PID | 0x80, STATUS_FAILED]);
                out.extend_from_slice(&(pid as u32).to_le_bytes());
                out.extend_from_slice(&(-1i32).to_le_bytes());
                out
            }
        };
    }
    if op == OP_REPORT_EXIT {
        // Report v1: [E,X,ver,OP, pid:u32le, code:i32le]
        if frame.len() != 4 + 4 + 4 {
            return rsp(op, STATUS_MALFORMED, 0).to_vec();
        }
        let pid = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
        let code = i32::from_le_bytes([frame[8], frame[9], frame[10], frame[11]]);
        let image_id = state.children.iter().find(|c| c.pid == pid).map(|c| c.image_id);
        if let Some(img) = image_id {
            if code != 0 {
                let name = State::child_name(img);
                emit_crash_marker(pid, code, name);
                state.log_crash_via_nexus_log(pid, code, name);
            }
            return rsp(op, STATUS_OK, pid).to_vec();
        }
        return rsp(op, STATUS_FAILED, pid).to_vec();
    }
    if op != OP_EXEC_IMAGE {
        return rsp(op, STATUS_UNSUPPORTED, 0).to_vec();
    }
    if frame.len() < 7 {
        return rsp(op, STATUS_MALFORMED, 0).to_vec();
    }
    let image_id = frame[4];
    let stack_pages = frame[5].max(1) as usize;
    let req_len = frame[6] as usize;
    if req_len == 0 || req_len > 48 || frame.len() != 7 + req_len {
        return rsp(op, STATUS_MALFORMED, 0).to_vec();
    }
    let requester = &frame[7..];

    // Security hardening: bind requester identity to the IPC channel.
    // The requester string is treated as *display* only; the authoritative identity is the
    // kernel-derived sender_service_id returned via ipc_recv_v2 metadata.
    if service_id_from_name(requester) != sender_service_id {
        return rsp(op, STATUS_DENIED, 0).to_vec();
    }

    // Ask init-lite (control channel) to authorize this exec via policyd.
    // NOTE: init-lite policyd proxy currently does not correlate by nonce; keep nonce fixed.
    let nonce: nexus_abi::policy::Nonce = 1;
    let mut q = [0u8; 10 + 48];
    let qn = match nexus_abi::policy::encode_exec_check(nonce, requester, image_id, &mut q) {
        Some(n) => n,
        None => return rsp(op, STATUS_MALFORMED, 0).to_vec(),
    };
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, qn as u32);
    let now = nexus_abi::nsec().ok().unwrap_or(0);
    // Init-lite may be busy answering ROUTE_GET queries (policyd-gated) during early bring-up.
    // Use a slightly longer deadline so exec authorization doesn't spuriously fail.
    let deadline = now.saturating_add(500_000_000);
    if nexus_abi::ipc_send_v1(1, &hdr, &q[..qn], 0, deadline).is_err() {
        return rsp(op, STATUS_FAILED, 0).to_vec();
    }
    let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut rb = [0u8; 16];
    let rn =
        match nexus_abi::ipc_recv_v1(2, &mut rh, &mut rb, nexus_abi::IPC_SYS_TRUNCATE, deadline) {
            Ok(n) => n as usize,
            Err(_) => return rsp(op, STATUS_FAILED, 0).to_vec(),
        };
    let (_rsp_nonce, decision) = nexus_abi::policy::decode_exec_check_rsp(&rb[..rn])
        .unwrap_or((nonce, nexus_abi::policy::STATUS_ALLOW));
    if decision != nexus_abi::policy::STATUS_ALLOW {
        return rsp(op, STATUS_DENIED, 0).to_vec();
    }

    let elf = match image_id {
        IMG_HELLO => HELLO_ELF,
        IMG_EXIT0 => DEMO_EXIT0_ELF,
        IMG_EXIT42 => DEMO_EXIT42_ELF,
        _ => return rsp(op, STATUS_UNSUPPORTED, 0).to_vec(),
    };
    match exec(elf, stack_pages, 0) {
        Ok(pid) => {
            state.track_child(pid as u32, image_id);
            rsp(op, STATUS_OK, pid as u32).to_vec()
        }
        Err(_) => rsp(op, STATUS_FAILED, 0).to_vec(),
    }
}

fn rsp(op: u8, status: u8, pid: u32) -> [u8; 9] {
    let mut out = [0u8; 9];
    out[0] = MAGIC0;
    out[1] = MAGIC1;
    out[2] = VERSION;
    out[3] = op | 0x80;
    out[4] = status;
    out[5..9].copy_from_slice(&pid.to_le_bytes());
    out
}

/// Stubbed bundle exec helper exposed for API compatibility.
pub fn exec_elf(
    _bundle: &str,
    _argv: &[&str],
    _env: &[&str],
    _restart: RestartPolicy,
) -> Result<Pid, ExecError> {
    Err(ExecError::Unsupported)
}

fn emit_line(message: &str) {
    for byte in message.as_bytes().iter().copied().chain(core::iter::once(b'\n')) {
        let _ = debug_putc(byte);
    }
}

fn emit_crash_marker(pid: u32, code: i32, name: &str) {
    emit_line_no_nl("execd: crash report pid=");
    emit_u64(pid as u64);
    emit_line_no_nl(" code=");
    emit_i64(code as i64);
    emit_line_no_nl(" name=");
    for b in name.as_bytes() {
        let _ = debug_putc(*b);
    }
    let _ = debug_putc(b'\n');
}

fn emit_line_no_nl(message: &str) {
    for byte in message.as_bytes().iter().copied() {
        let _ = debug_putc(byte);
    }
}

fn emit_u64(mut value: u64) {
    let mut buf = [0u8; 20];
    let mut idx = buf.len();
    if value == 0 {
        idx -= 1;
        buf[idx] = b'0';
    } else {
        while value != 0 {
            idx -= 1;
            buf[idx] = b'0' + (value % 10) as u8;
            value /= 10;
        }
    }
    for &b in &buf[idx..] {
        let _ = debug_putc(b);
    }
}

fn emit_i64(value: i64) {
    if value < 0 {
        let _ = debug_putc(b'-');
        emit_u64((-value) as u64);
    } else {
        emit_u64(value as u64);
    }
}

// (helpers removed; crash record is emitted through nexus-log sink-logd)
