#![cfg(all(nexus_env = "os", feature = "os-lite"))]

extern crate alloc;

use alloc::boxed::Box;

use core::fmt;

use nexus_abi::{debug_putc, yield_};
use nexus_ipc::{KernelClient, KernelServer, Server as _, Wait};

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

const OPCODE_CHECK: u8 = 1;

/// Minimal kernel-IPC backed policyd loop.
///
/// NOTE: This is a bring-up implementation: it only supports an allow/deny check over IPC and
/// returns a deterministic decision:
/// - allow if subject != "demo.testsvc"
/// - deny if subject == "demo.testsvc"
pub fn service_main_loop(notifier: ReadyNotifier) -> LiteResult<()> {
    notifier.notify();
    emit_line("policyd: ready (stub)");
    let server = KernelServer::new_for("policyd").map_err(|_| ServerError::Unsupported)?;
    loop {
        match server.recv(Wait::Blocking) {
            Ok(frame) => {
                if frame.is_empty() {
                    continue;
                }
                if frame[0] != OPCODE_CHECK || frame.len() < 2 {
                    continue;
                }
                let n = frame[1] as usize;
                if 2 + n > frame.len() {
                    continue;
                }
                let name = core::str::from_utf8(&frame[2..2 + n]).unwrap_or("");
                let allowed = name != "demo.testsvc";
                let reply = [OPCODE_CHECK, if allowed { 1 } else { 0 }];
                let _ = server.send(&reply, Wait::Blocking);
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(nexus_ipc::IpcError::Disconnected) => return Err(ServerError::Unsupported),
            Err(_) => return Err(ServerError::Unsupported),
        }
    }
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
