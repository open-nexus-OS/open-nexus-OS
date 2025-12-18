#![cfg(all(nexus_env = "os", feature = "os-lite"))]

extern crate alloc;

use alloc::boxed::Box;

use core::fmt;

use nexus_abi::{debug_putc, yield_};
use nexus_ipc::{KernelServer, Server as _, Wait};

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

/// Stubbed service loop that announces readiness then yields forever.
pub fn service_main_loop(notifier: ReadyNotifier) -> LiteResult<()> {
    notifier.notify();
    emit_line("samgrd: ready (stub)");
    let server = KernelServer::new_for("samgrd").map_err(|_| ServerError::Unsupported)?;
    loop {
        match server.recv(Wait::Blocking) {
            Ok(frame) => {
                // Minimal bring-up protocol: echo back the bytes to prove routing+IPC.
                let _ = server.send(&frame, Wait::Blocking);
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(nexus_ipc::IpcError::Disconnected) => return Err(ServerError::Unsupported),
            Err(_) => return Err(ServerError::Unsupported),
        }
    }
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
