// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::fmt;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

use nexus_ipc::{self, Wait};

use bundlemgr::{service::InstallRequest as DomainInstallRequest, Service, ServiceError};

#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!("nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '...\"os\"'");

#[cfg(not(feature = "idl-capnp"))]
compile_error!("Enable the `idl-capnp` feature to build bundlemgrd handlers.");

#[cfg(feature = "idl-capnp")]
use capnp::message::{Builder, HeapAllocator, ReaderOptions};
#[cfg(feature = "idl-capnp")]
use capnp::serialize;
#[cfg(feature = "idl-capnp")]
use nexus_idl_runtime::bundlemgr_capnp::{
    install_request, install_response, query_request, query_response, InstallError,
};

const OPCODE_INSTALL: u8 = 1;
const OPCODE_QUERY: u8 = 2;

/// Trait implemented by transports that deliver frames to bundlemgrd.
pub trait Transport {
    /// Error type surfaced by the transport implementation.
    type Error: Into<TransportError>;

    /// Receives the next request frame if one is available.
    fn recv(&mut self) -> Result<Option<Vec<u8>>, Self::Error>;

    /// Sends a response frame back to the caller.
    fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error>;
}

/// Errors emitted by transports.
#[derive(Debug)]
pub enum TransportError {
    /// Transport has been shut down by the peer.
    Closed,
    /// I/O level error occurred.
    Io(std::io::Error),
    /// Current platform lacks a transport implementation.
    Unsupported,
    /// Any other transport issue.
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
    fn from(value: String) -> Self {
        Self::Other(value)
    }
}

impl From<&str> for TransportError {
    fn from(value: &str) -> Self {
        Self::Other(value.to_string())
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

/// Notifies init that the bundle manager daemon is ready to serve requests.
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    /// Creates a new notifier from a closure.
    pub fn new<F>(func: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self(Box::new(func))
    }

    /// Signals readiness.
    pub fn notify(self) {
        (self.0)();
    }
}

/// IPC transport adapter backed by [`nexus-ipc`].
pub struct IpcTransport<T> {
    server: T,
}

impl<T> IpcTransport<T> {
    /// Wraps the provided server instance.
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

/// Errors returned by the server.
#[derive(Debug)]
pub enum ServerError {
    /// Transport level issue.
    Transport(TransportError),
    /// Failed to decode an incoming frame.
    Decode(String),
    /// Failed to encode a response frame.
    Encode(capnp::Error),
    /// Domain level error from the bundle manager service.
    Service(ServiceError),
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(err) => write!(f, "transport error: {err}"),
            Self::Decode(msg) => write!(f, "decode error: {msg}"),
            Self::Encode(err) => write!(f, "encode error: {err}"),
            Self::Service(err) => write!(f, "service error: {err}"),
        }
    }
}

impl std::error::Error for ServerError {}

impl From<TransportError> for ServerError {
    fn from(err: TransportError) -> Self {
        Self::Transport(err)
    }
}

impl From<ServiceError> for ServerError {
    fn from(err: ServiceError) -> Self {
        Self::Service(err)
    }
}

#[derive(Clone, Default)]
pub struct ArtifactStore {
    inner: Arc<Mutex<HashMap<u32, Vec<u8>>>>,
}

impl ArtifactStore {
    /// Creates an empty artifact store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts artifact bytes associated with `handle`.
    pub fn insert(&self, handle: u32, bytes: Vec<u8>) {
        match self.inner.lock() {
            Ok(mut guard) => {
                guard.insert(handle, bytes);
            }
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.insert(handle, bytes);
            }
        }
    }

    /// Removes and returns artifact bytes for `handle` if they exist.
    pub fn take(&self, handle: u32) -> Option<Vec<u8>> {
        match self.inner.lock() {
            Ok(mut guard) => guard.remove(&handle),
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.remove(&handle)
            }
        }
    }
}

struct Server {
    service: Service,
    artifacts: ArtifactStore,
}

impl Server {
    fn new(service: Service, artifacts: ArtifactStore) -> Self {
        Self { service, artifacts }
    }

    #[cfg(feature = "idl-capnp")]
    fn handle_frame(&mut self, opcode: u8, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        match opcode {
            OPCODE_INSTALL => self.handle_install(payload),
            OPCODE_QUERY => self.handle_query(payload),
            other => Err(ServerError::Decode(format!("unknown opcode {other}"))),
        }
    }

    #[cfg(feature = "idl-capnp")]
    fn handle_install(&mut self, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        let mut cursor = Cursor::new(payload);
        let message = serialize::read_message(&mut cursor, ReaderOptions::new())
            .map_err(|err| ServerError::Decode(format!("install read: {err}")))?;
        let request = message
            .get_root::<install_request::Reader<'_>>()
            .map_err(|err| ServerError::Decode(format!("install root: {err}")))?;

        let name = request
            .get_name()
            .map_err(|err| ServerError::Decode(format!("install name: {err}")))?
            .to_str()
            .map_err(|err| ServerError::Decode(format!("install name utf8: {err}")))?
            .to_string();
        let expected_len = request.get_bytes_len() as usize;
        let handle = request.get_vmo_handle();
        let mut response = Builder::new_default();
        let mut builder = response.init_root::<install_response::Builder<'_>>();

        let bytes = match self.artifacts.take(handle) {
            Some(bytes) => bytes,
            None => {
                builder.set_ok(false);
                builder.set_err(InstallError::Enoent);
                return Self::encode_response(OPCODE_INSTALL, &response);
            }
        };

        if bytes.len() != expected_len {
            builder.set_ok(false);
            builder.set_err(InstallError::Einval);
            return Self::encode_response(OPCODE_INSTALL, &response);
        }

        let manifest = match std::str::from_utf8(&bytes) {
            Ok(value) => value,
            Err(_) => {
                builder.set_ok(false);
                builder.set_err(InstallError::Einval);
                return Self::encode_response(OPCODE_INSTALL, &response);
            }
        };

        match self.service.install(DomainInstallRequest { name: &name, manifest }) {
            Ok(_) => {
                builder.set_ok(true);
                builder.set_err(InstallError::None);
            }
            Err(err) => {
                builder.set_ok(false);
                builder.set_err(map_install_error(&err));
            }
        }

        Self::encode_response(OPCODE_INSTALL, &response)
    }

    #[cfg(feature = "idl-capnp")]
    fn handle_query(&mut self, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        let mut cursor = Cursor::new(payload);
        let message = serialize::read_message(&mut cursor, ReaderOptions::new())
            .map_err(|err| ServerError::Decode(format!("query read: {err}")))?;
        let request = message
            .get_root::<query_request::Reader<'_>>()
            .map_err(|err| ServerError::Decode(format!("query root: {err}")))?;
        let name = request
            .get_name()
            .map_err(|err| ServerError::Decode(format!("query name: {err}")))?
            .to_str()
            .map_err(|err| ServerError::Decode(format!("query name utf8: {err}")))?
            .to_string();

        let mut response = Builder::new_default();
        {
            let mut builder = response.init_root::<query_response::Builder<'_>>();
            match self.service.query(&name).map_err(ServerError::from)? {
                Some(bundle) => {
                    builder.set_installed(true);
                    let version = bundle.version.to_string();
                    builder.set_version(&version);
                }
                None => {
                    builder.set_installed(false);
                    builder.set_version("");
                }
            }
        }
        Self::encode_response(OPCODE_QUERY, &response)
    }

    #[cfg(feature = "idl-capnp")]
    fn encode_response(
        opcode: u8,
        message: &Builder<HeapAllocator>,
    ) -> Result<Vec<u8>, ServerError> {
        let mut payload = Vec::new();
        serialize::write_message(&mut payload, message).map_err(ServerError::Encode)?;
        let mut frame = Vec::with_capacity(1 + payload.len());
        frame.push(opcode);
        frame.extend_from_slice(&payload);
        Ok(frame)
    }
}

/// Runs the server with the provided transport and artifact store.
#[cfg(feature = "idl-capnp")]
pub fn run_with_transport<T: Transport>(
    transport: &mut T,
    artifacts: ArtifactStore,
) -> Result<(), ServerError> {
    let service = Service::new();
    serve_with_components(transport, service, artifacts)
}

/// Serves requests using injected service and artifact store.
#[cfg(feature = "idl-capnp")]
pub fn serve_with_components<T: Transport>(
    transport: &mut T,
    service: Service,
    artifacts: ArtifactStore,
) -> Result<(), ServerError> {
    let mut server = Server::new(service, artifacts);
    while let Some(frame) = transport.recv().map_err(|err| ServerError::Transport(err.into()))? {
        if frame.is_empty() {
            continue;
        }
        let (opcode, payload) =
            frame.split_first().ok_or_else(|| ServerError::Decode("empty frame".into()))?;
        let response = server.handle_frame(*opcode, payload)?;
        transport.send(&response).map_err(|err| ServerError::Transport(err.into()))?;
    }
    Ok(())
}

#[cfg(feature = "idl-capnp")]
fn map_install_error(error: &ServiceError) -> InstallError {
    match error {
        ServiceError::AlreadyInstalled => InstallError::Ebusy,
        ServiceError::InvalidSignature => InstallError::Eacces,
        ServiceError::Manifest(_) => InstallError::Einval,
        ServiceError::Unsupported => InstallError::Einval,
    }
}

/// Executes the server using the default system transport and a fresh artifact store.
pub fn run_default() -> Result<(), ServerError> {
    service_main_loop(ReadyNotifier::new(|| ()), ArtifactStore::new())
}

/// Runs the server using the default IPC transport and artifact store.
pub fn service_main_loop(
    notifier: ReadyNotifier,
    artifacts: ArtifactStore,
) -> Result<(), ServerError> {
    #[cfg(nexus_env = "host")]
    {
        let (client, server) = nexus_ipc::loopback_channel();
        let _client_guard = client;
        let mut transport = IpcTransport::new(server);
        notifier.notify();
        println!("bundlemgrd: ready");
        run_with_transport(&mut transport, artifacts)
    }

    #[cfg(nexus_env = "os")]
    {
        let server = nexus_ipc::KernelServer::new()
            .map_err(|err| ServerError::Transport(TransportError::from(err)))?;
        let mut transport = IpcTransport::new(server);
        notifier.notify();
        println!("bundlemgrd: ready");
        run_with_transport(&mut transport, artifacts)
    }
}

/// Creates a loopback transport pair for host-side tests.
#[cfg(nexus_env = "host")]
pub fn loopback_transport() -> (nexus_ipc::LoopbackClient, IpcTransport<nexus_ipc::LoopbackServer>)
{
    let (client, server) = nexus_ipc::loopback_channel();
    (client, IpcTransport::new(server))
}

/// Touches Cap'n Proto schemas to keep generated code linked.
pub fn touch_schemas() {
    #[cfg(feature = "idl-capnp")]
    {
        let _ = core::any::type_name::<install_request::Reader<'static>>();
        let _ = core::any::type_name::<install_response::Reader<'static>>();
        let _ = core::any::type_name::<query_request::Reader<'static>>();
        let _ = core::any::type_name::<query_response::Reader<'static>>();
    }
}
