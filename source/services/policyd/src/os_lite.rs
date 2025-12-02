#![cfg(all(nexus_env = "os", feature = "os-lite"))]

use alloc::boxed::Box;

use core::fmt;

use nexus_abi::{debug_putc, yield_};

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

/// Stubbed service loop that reports readiness then yields forever.
pub fn service_main_loop(notifier: ReadyNotifier) -> LiteResult<()> {
    notifier.notify();
    emit_line("policyd: ready (stub)");
    loop {
        let _ = yield_();
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
