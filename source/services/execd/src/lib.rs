// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! execd daemon: executes service bundles after policy approval.

#![forbid(unsafe_code)]

use std::fmt;
use std::io::Cursor;

use nexus_ipc::{self, Wait};
use thiserror::Error;

#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!(
    "nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '--cfg nexus_env=\"os\"'.",
);

#[cfg(not(feature = "idl-capnp"))]
compile_error!("Enable the `idl-capnp` feature to build execd handlers.");

#[cfg(feature = "idl-capnp")]
use capnp::message::{Builder, ReaderOptions};
#[cfg(feature = "idl-capnp")]
use capnp::serialize;
#[cfg(feature = "idl-capnp")]
use nexus_idl_runtime::execd_capnp::{exec_request, exec_response};

const OPCODE_EXEC: u8 = 1;

/// Trait implemented by transports capable of delivering execution requests.
pub trait Transport {
    type Error: Into<TransportError>;

    fn recv(&mut self) -> Result<Option<Vec<u8>>, Self::Error>;

    fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error>;
}

/// Errors emitted by transports interacting with execd.
#[derive(Debug)]
pub enum TransportError {
    Closed,
    Io(std::io::Error),
    Unsupported,
    Other(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => write!(f, "transport closed"),
            Self::Io(err) => write!(f, "transport io error: {err}"),
            Self::Unsupported => write!(f, "transport unsupported"),
            Self::Other(msg) => write!(f, "transport error: {msg}"),
        }
    }
}

impl std::error::Error for TransportError {}

impl From<std::io::Error> for TransportError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<String> for TransportError {
    fn from(msg: String) -> Self {
        Self::Other(msg)
    }
}

impl From<&str> for TransportError {
    fn from(msg: &str) -> Self {
        Self::Other(msg.to_string())
    }
}

impl From<nexus_ipc::IpcError> for TransportError {
    fn from(err: nexus_ipc::IpcError) -> Self {
        match err {
            nexus_ipc::IpcError::Disconnected => Self::Closed,
            nexus_ipc::IpcError::Unsupported => Self::Unsupported,
            nexus_ipc::IpcError::WouldBlock | nexus_ipc::IpcError::Timeout => {
                Self::Other("operation timed out".to_string())
            }
            nexus_ipc::IpcError::Kernel(inner) => {
                Self::Other(format!("kernel ipc error: {inner:?}"))
            }
        }
    }
}

/// Notifies the init process that the daemon has completed its startup sequence.
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    pub fn new<F>(func: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self(Box::new(func))
    }

    pub fn notify(self) {
        (self.0)();
    }
}

/// Transport backed by the [`nexus-ipc`] runtime.
pub struct IpcTransport<T> {
    server: T,
}

impl<T> IpcTransport<T> {
    pub fn new(server: T) -> Self {
        Self { server }
    }
}

impl<T> Transport for IpcTransport<T>
where
    T: nexus_ipc::Server + Send,
{
    type Error = nexus_ipc::IpcError;

    fn recv(&mut self) -> Result<Option<Vec<u8>>, Self::Error> {
        match self.server.recv(Wait::Blocking) {
            Ok(frame) => Ok(Some(frame)),
            Err(nexus_ipc::IpcError::Disconnected) => Ok(None),
            Err(nexus_ipc::IpcError::WouldBlock | nexus_ipc::IpcError::Timeout) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error> {
        self.server.send(frame, Wait::Blocking)
    }
}

/// Errors surfaced by execd when processing requests.
#[derive(Debug, Error)]
pub enum ServerError {
    #[error("transport error: {0}")]
    Transport(TransportError),
    #[error("decode error: {0}")]
    Decode(String),
    #[cfg(feature = "idl-capnp")]
    #[error("encode error: {0}")]
    Encode(#[from] capnp::Error),
}

impl From<TransportError> for ServerError {
    fn from(err: TransportError) -> Self {
        Self::Transport(err)
    }
}

struct ExecService;

impl ExecService {
    fn new() -> Self {
        Self
    }

    fn handle_frame(&self, frame: &[u8]) -> Result<Vec<u8>, ServerError> {
        if frame.is_empty() {
            return Err(ServerError::Decode("empty request".to_string()));
        }
        match frame[0] {
            OPCODE_EXEC => self.handle_exec(&frame[1..]),
            other => Err(ServerError::Decode(format!("unknown opcode {other}"))),
        }
    }

    fn handle_exec(&self, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        #[cfg(feature = "idl-capnp")]
        {
            let mut cursor = Cursor::new(payload);
            let message = serialize::read_message(&mut cursor, ReaderOptions::new())
                .map_err(|err| ServerError::Decode(format!("read exec request: {err}")))?;
            let reader = message
                .get_root::<exec_request::Reader<'_>>()
                .map_err(|err| ServerError::Decode(format!("exec request root: {err}")))?;
            let name = reader
                .get_name()
                .map_err(|err| ServerError::Decode(format!("exec name read: {err}")))?
                .to_str()
                .map_err(|err| ServerError::Decode(format!("exec name utf8: {err}")))?
                .to_string();

            println!("execd: exec {name}");

            let mut response = Builder::new_default();
            {
                let mut builder = response.init_root::<exec_response::Builder<'_>>();
                builder.set_ok(true);
                builder.set_message("");
            }

            let mut body = Vec::new();
            serialize::write_message(&mut body, &response)?;
            let mut frame = Vec::with_capacity(1 + body.len());
            frame.push(OPCODE_EXEC);
            frame.extend_from_slice(&body);
            Ok(frame)
        }

        #[cfg(not(feature = "idl-capnp"))]
        {
            let _ = payload;
            Err(ServerError::Decode("capnp support disabled".to_string()))
        }
    }
}

/// Runs the daemon main loop using the default transport backend.
pub fn service_main_loop(notifier: ReadyNotifier) -> Result<(), ServerError> {
    #[cfg(nexus_env = "host")]
    {
        let (client, server) = nexus_ipc::loopback_channel();
        let _client_guard = client;
        let mut transport = IpcTransport::new(server);
        run_with_transport_ready(&mut transport, notifier)
    }

    #[cfg(nexus_env = "os")]
    {
        let server = nexus_ipc::KernelServer::new()
            .map_err(|err| ServerError::Transport(TransportError::from(err)))?;
        let mut transport = IpcTransport::new(server);
        run_with_transport_ready(&mut transport, notifier)
    }
}

/// Runs the daemon using the provided transport and emits readiness markers.
pub fn run_with_transport_ready<T: Transport>(
    transport: &mut T,
    notifier: ReadyNotifier,
) -> Result<(), ServerError> {
    touch_schemas();
    let service = ExecService::new();
    notifier.notify();
    println!("execd: ready");
    serve(&service, transport)
}

/// Runs the daemon using the provided transport without emitting readiness markers.
pub fn run_with_transport<T: Transport>(transport: &mut T) -> Result<(), ServerError> {
    touch_schemas();
    let service = ExecService::new();
    serve(&service, transport)
}

fn serve<T>(service: &ExecService, transport: &mut T) -> Result<(), ServerError>
where
    T: Transport,
{
    loop {
        match transport.recv().map_err(|err| ServerError::Transport(err.into()))? {
            Some(frame) => {
                let response = service.handle_frame(&frame)?;
                transport.send(&response).map_err(|err| ServerError::Transport(err.into()))?;
            }
            None => return Ok(()),
        }
    }
}

/// Runs the daemon entry point until termination.
pub fn daemon_main<R: FnOnce() + Send + 'static>(notify: R) -> ! {
    touch_schemas();
    if let Err(err) = service_main_loop(ReadyNotifier::new(notify)) {
        eprintln!("execd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}

/// Creates a loopback transport pair for host-side tests.
#[cfg(nexus_env = "host")]
pub fn loopback_transport() -> (nexus_ipc::LoopbackClient, IpcTransport<nexus_ipc::LoopbackServer>)
{
    let (client, server) = nexus_ipc::loopback_channel();
    (client, IpcTransport::new(server))
}

/// Touches the Cap'n Proto schema so release builds keep the generated module.
pub fn touch_schemas() {
    #[cfg(feature = "idl-capnp")]
    {
        let _ = core::any::type_name::<exec_request::Reader<'static>>();
        let _ = core::any::type_name::<exec_response::Reader<'static>>();
    }
}
