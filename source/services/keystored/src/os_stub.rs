extern crate alloc;

use alloc::boxed::Box;

use core::fmt;
use core::marker::PhantomData;

use nexus_abi::{debug_putc, yield_};
use nexus_ipc::{KernelServer, Server as _, Wait};

/// Result type surfaced by the lite keystored shim.
pub type LiteResult<T> = Result<T, ServerError>;

/// Placeholder transport trait retained for API compatibility.
pub trait Transport {
    /// Associated error type for the transport.
    type Error;
}

/// Stub transport wrapper; no runtime transport support in os-lite yet.
pub struct IpcTransport<T> {
    _marker: PhantomData<T>,
}

impl<T> IpcTransport<T> {
    /// Constructs the transport wrapper.
    pub fn new(_server: T) -> Self {
        Self {
            _marker: PhantomData,
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

/// Transport level errors surfaced by the shim implementation.
#[derive(Debug)]
pub enum TransportError {
    /// Transport support is not yet implemented in the os-lite runtime.
    Unsupported,
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "transport unsupported"),
        }
    }
}

/// Server level errors.
#[derive(Debug)]
pub enum ServerError {
    /// Functionality not yet implemented in the os-lite path.
    Unsupported(&'static str),
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(msg) => write!(f, "{msg} unsupported"),
        }
    }
}

impl From<TransportError> for ServerError {
    fn from(_err: TransportError) -> Self {
        Self::Unsupported("transport")
    }
}

/// Runs the keystored daemon with the provided transport (stubbed).
pub fn run_with_transport<T: Transport>(_transport: &mut T) -> LiteResult<()> {
    Err(ServerError::Unsupported("keystored run_with_transport"))
}

/// Runs the keystored daemon using the default transport (stubbed).
pub fn run_default() -> LiteResult<()> {
    Err(ServerError::Unsupported("keystored run_default"))
}

/// Runs the keystored daemon using the default transport and anchor set (stubbed).
pub fn run_with_transport_default_anchors<T: Transport>(_transport: &mut T) -> LiteResult<()> {
    Err(ServerError::Unsupported(
        "keystored run_with_transport_default_anchors",
    ))
}

/// Main service loop; notifies readiness and yields cooperatively.
pub fn service_main_loop(notifier: ReadyNotifier) -> LiteResult<()> {
    notifier.notify();
    emit_line("keystored: ready (stub)");
    let server = KernelServer::new_for("keystored").map_err(|_| ServerError::Unsupported("ipc"))?;
    loop {
        match server.recv(Wait::Blocking) {
            Ok(frame) => {
                let _ = server.send(&frame, Wait::Blocking);
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(nexus_ipc::IpcError::Disconnected) => return Err(ServerError::Unsupported("ipc")),
            Err(_) => return Err(ServerError::Unsupported("ipc")),
        }
    }
}

/// Touches schema types to keep host parity; no-op in the stub.
pub fn touch_schemas() {}

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
