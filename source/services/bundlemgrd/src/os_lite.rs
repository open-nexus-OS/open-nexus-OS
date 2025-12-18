#![cfg(all(nexus_env = "os", feature = "os-lite"))]

extern crate alloc;

use alloc::boxed::Box;

use core::fmt;

use nexus_abi::{debug_putc, yield_};
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

/// Main service loop used by the lite shim.
pub fn service_main_loop(notifier: ReadyNotifier, _artifacts: ArtifactStore) -> LiteResult<()> {
    notifier.notify();
    emit_line("bundlemgrd: ready (stub)");
    let server = KernelServer::new_for("bundlemgrd").map_err(|_| ServerError::Unsupported)?;
    loop {
        match server.recv(Wait::Blocking) {
            Ok(frame) => {
                // Minimal protocol: echo requests back to caller.
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
