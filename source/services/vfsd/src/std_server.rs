use std::collections::HashMap;
use std::sync::Arc;

use capnp::message::ReaderOptions;
use capnp::serialize;
use log::{error, info};
use parking_lot::Mutex;
use thiserror::Error;

use nexus_idl_runtime::vfs_capnp::{
    close_request, close_response, mount_request, mount_response, open_request, open_response,
    read_request, read_response, stat_request, stat_response,
};
use nexus_ipc::{IpcError, Wait};
use nexus_packagefs::PackageFsClient;

const OPCODE_OPEN: u8 = 1;
const OPCODE_READ: u8 = 2;
const OPCODE_CLOSE: u8 = 3;
const OPCODE_STAT: u8 = 4;
const OPCODE_MOUNT: u8 = 5;

const KIND_DIRECTORY: u16 = 1;

/// Result alias used by the service.
pub type Result<T> = core::result::Result<T, ServerError>;

/// Errors surfaced while serving requests.
#[derive(Debug, Error)]
pub enum ServerError {
    /// Transport level failure.
    #[error("transport error: {0}")]
    Transport(TransportError),
    /// Incoming frame could not be decoded.
    #[error("decode error: {0}")]
    Decode(String),
    /// Failed to encode a Cap'n Proto response.
    #[error("encode error: {0}")]
    Encode(capnp::Error),
    /// Backend specific failure.
    #[error("service error: {0}")]
    Service(ServiceError),
}

impl From<TransportError> for ServerError {
    fn from(value: TransportError) -> Self {
        Self::Transport(value)
    }
}

impl From<ServiceError> for ServerError {
    fn from(value: ServiceError) -> Self {
        Self::Service(value)
    }
}

/// Transport abstraction used by vfsd.
pub trait Transport {
    /// Error surfaced by the transport implementation.
    type Error: Into<TransportError>;

    /// Receives the next frame if available.
    fn recv(&mut self) -> core::result::Result<Option<Vec<u8>>, Self::Error>;

    /// Sends a response frame back to the caller.
    fn send(&mut self, frame: &[u8]) -> core::result::Result<(), Self::Error>;
}

/// Transport backed by [`nexus_ipc`].
pub struct IpcTransport<T> {
    server: T,
}

impl<T> IpcTransport<T> {
    /// Wraps the provided server handle.
    pub fn new(server: T) -> Self {
        Self { server }
    }
}

impl<T> Transport for IpcTransport<T>
where
    T: nexus_ipc::Server + Send,
{
    type Error = nexus_ipc::IpcError;

    fn recv(&mut self) -> core::result::Result<Option<Vec<u8>>, Self::Error> {
        match self.server.recv(Wait::Blocking) {
            Ok(frame) => Ok(Some(frame)),
            Err(IpcError::Disconnected) => Ok(None),
            Err(IpcError::WouldBlock | IpcError::Timeout) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn send(&mut self, frame: &[u8]) -> core::result::Result<(), Self::Error> {
        self.server.send(frame, Wait::Blocking)
    }
}

/// Transport level failures surfaced by [`Transport`].
#[derive(Debug, Error)]
pub enum TransportError {
    /// Connection closed by the peer.
    #[error("transport closed")]
    Closed,
    /// I/O failure.
    #[error("io error: {0}")]
    Io(String),
    /// Transport unsupported on this platform.
    #[error("transport unsupported")]
    Unsupported,
    /// Any other failure category.
    #[error("transport error: {0}")]
    Other(String),
}

impl From<IpcError> for TransportError {
    fn from(value: IpcError) -> Self {
        match value {
            IpcError::Disconnected => Self::Closed,
            IpcError::Unsupported => Self::Unsupported,
            IpcError::WouldBlock | IpcError::Timeout => {
                Self::Other("operation timed out".to_string())
            }
            IpcError::NoSpace => Self::Other("ipc ran out of resources".to_string()),
            IpcError::Kernel(err) => Self::Other(format!("kernel error: {err:?}")),
            _ => Self::Other(format!("ipc error: {value:?}")),
        }
    }
}

impl From<std::io::Error> for TransportError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
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

/// Error types produced by the VFS dispatcher.
#[derive(Debug, Error)]
pub enum ServiceError {
    /// Requested path was not found.
    #[error("not found")]
    NotFound,
    /// Caller supplied an invalid path.
    #[error("invalid path")]
    InvalidPath,
    /// File handle is invalid or has been closed.
    #[error("bad file handle")]
    BadHandle,
    /// Underlying provider failed.
    #[error("provider error: {0}")]
    Provider(String),
}

/// Read-only file metadata stored for active handles.
#[derive(Clone)]
struct HandleEntry {
    bytes: Vec<u8>,
    size: u64,
    kind: u16,
}

impl HandleEntry {
    fn read(&self, off: u64, len: u32) -> &[u8] {
        let start = core::cmp::min(off as usize, self.bytes.len());
        let end = core::cmp::min(start.saturating_add(len as usize), self.bytes.len());
        &self.bytes[start..end]
    }
}

/// File description returned by filesystem providers.
#[derive(Debug, Clone)]
struct ProviderEntry {
    bytes: Vec<u8>,
    size: u64,
    kind: u16,
}

/// Trait implemented by filesystem backends registered in the mount table.
trait FsProvider: Send + Sync {
    fn resolve(&self, rel_path: &str) -> core::result::Result<ProviderEntry, ServiceError>;
    fn stat(&self, rel_path: &str) -> core::result::Result<(u64, u16), ServiceError>;
}

/// Package file system provider backed by the packagefs service.
struct PackageFsProvider {
    client: Arc<PackageFsClient>,
}

impl PackageFsProvider {
    fn new(client: Arc<PackageFsClient>) -> Self {
        Self { client }
    }
}

impl FsProvider for PackageFsProvider {
    fn resolve(&self, rel_path: &str) -> core::result::Result<ProviderEntry, ServiceError> {
        let entry = self.client.resolve(rel_path).map_err(map_packagefs_error)?;
        if entry.kind() == KIND_DIRECTORY {
            return Err(ServiceError::InvalidPath);
        }
        Ok(ProviderEntry {
            bytes: entry.bytes().to_vec(),
            size: entry.size(),
            kind: entry.kind(),
        })
    }

    fn stat(&self, rel_path: &str) -> core::result::Result<(u64, u16), ServiceError> {
        let entry = self.client.resolve(rel_path).map_err(map_packagefs_error)?;
        Ok((entry.size(), entry.kind()))
    }
}

fn map_packagefs_error(err: nexus_packagefs::Error) -> ServiceError {
    match err {
        nexus_packagefs::Error::NotFound => ServiceError::NotFound,
        nexus_packagefs::Error::InvalidPath => ServiceError::InvalidPath,
        nexus_packagefs::Error::Unsupported => {
            ServiceError::Provider("packagefs unsupported".into())
        }
        nexus_packagefs::Error::Encode | nexus_packagefs::Error::Decode => {
            ServiceError::Provider("packagefs protocol error".into())
        }
        nexus_packagefs::Error::Rejected => {
            ServiceError::Provider("packagefs publish rejected".into())
        }
        nexus_packagefs::Error::Ipc(msg) => ServiceError::Provider(format!("packagefs ipc: {msg}")),
    }
}

/// In-memory mount table shared by the dispatcher.
struct MountTable {
    mounts: HashMap<String, Box<dyn FsProvider>>,
}

impl MountTable {
    fn new() -> Self {
        Self {
            mounts: HashMap::new(),
        }
    }

    fn mount(&mut self, path: &str, provider: Box<dyn FsProvider>) {
        self.mounts.insert(path.to_string(), provider);
    }

    fn resolve<'a>(
        &'a self,
        path: &str,
    ) -> core::result::Result<(&'a dyn FsProvider, String), ServiceError> {
        if let Some(rest) = path.strip_prefix("pkg:/") {
            let provider = self
                .mounts
                .get("/packages")
                .ok_or(ServiceError::InvalidPath)?;
            let rel = rest.trim_start_matches('/');
            if rel.is_empty() {
                return Err(ServiceError::InvalidPath);
            }
            return Ok((&**provider, rel.to_string()));
        }
        let path = path.trim_start_matches('/');
        let mut best: Option<(&Box<dyn FsProvider>, usize)> = None;
        for (mount, provider) in &self.mounts {
            let trimmed = mount.trim_start_matches('/').trim_end_matches('/');
            if trimmed.is_empty() {
                continue;
            }
            if path.starts_with(trimmed) {
                let len = trimmed.len();
                if best.map_or(true, |(_, best_len)| len > best_len) {
                    best = Some((provider, len));
                }
            }
        }
        if let Some((provider, len)) = best {
            let rel = path[len..].trim_start_matches('/').to_string();
            if rel.is_empty() {
                return Err(ServiceError::InvalidPath);
            }
            Ok((&**provider, rel))
        } else {
            Err(ServiceError::InvalidPath)
        }
    }
}

/// Shared dispatcher state.
struct Dispatcher {
    mounts: Mutex<MountTable>,
    handles: Mutex<HashMap<u32, HandleEntry>>,
    next_handle: Mutex<u32>,
    packagefs: Arc<PackageFsClient>,
}

impl Dispatcher {
    fn new(packagefs: Arc<PackageFsClient>) -> Self {
        let mut mounts = MountTable::new();
        mounts.mount(
            "/packages",
            Box::new(PackageFsProvider::new(packagefs.clone())),
        );
        info!("vfsd: mounted /packages -> packagefs");
        Self {
            mounts: Mutex::new(mounts),
            handles: Mutex::new(HashMap::new()),
            next_handle: Mutex::new(1),
            packagefs,
        }
    }

    fn mount(&self, mount_point: &str, fs_id: &str) -> Result<()> {
        let mut mounts = self.mounts.lock();
        match fs_id {
            "packagefs" => {
                mounts.mount(
                    mount_point,
                    Box::new(PackageFsProvider::new(self.packagefs.clone())),
                );
                Ok(())
            }
            other => Err(ServerError::Service(ServiceError::Provider(format!(
                "unknown fs id {other}"
            )))),
        }
    }

    fn open(&self, path: &str) -> Result<(u32, ProviderEntry)> {
        let mounts = self.mounts.lock();
        let (provider, rel) = mounts.resolve(path)?;
        let entry = provider.resolve(&rel)?;
        drop(mounts);

        if entry.kind == KIND_DIRECTORY {
            return Err(ServiceError::InvalidPath.into());
        }

        let mut handles = self.handles.lock();
        let mut next = self.next_handle.lock();
        let handle = *next;
        *next = next.saturating_add(1).max(1);
        handles.insert(
            handle,
            HandleEntry {
                bytes: entry.bytes.clone(),
                size: entry.size,
                kind: entry.kind,
            },
        );
        Ok((handle, entry))
    }

    fn read(&self, handle: u32, off: u64, len: u32) -> Result<Vec<u8>> {
        let handles = self.handles.lock();
        let entry = handles.get(&handle).ok_or(ServiceError::BadHandle)?;
        let slice = entry.read(off, len);
        Ok(slice.to_vec())
    }

    fn close(&self, handle: u32) -> Result<()> {
        let mut handles = self.handles.lock();
        if handles.remove(&handle).is_some() {
            Ok(())
        } else {
            Err(ServiceError::BadHandle.into())
        }
    }

    fn stat(&self, path: &str) -> Result<(u64, u16)> {
        let mounts = self.mounts.lock();
        let (provider, rel) = mounts.resolve(path)?;
        provider.stat(&rel).map_err(ServerError::from)
    }
}

/// Notifies init when the service is ready.
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    /// Creates a notifier from `func`.
    pub fn new<F>(func: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self(Box::new(func))
    }

    /// Emits the ready marker.
    pub fn notify(self) {
        (self.0)();
    }
}

/// Runs the service using the default transport.
pub fn service_main_loop(notifier: ReadyNotifier) -> Result<()> {
    #[cfg(nexus_env = "host")]
    {
        notifier.notify();
        Err(ServerError::Service(ServiceError::Provider(
            "packagefs client unsupported on host".into(),
        )))
    }

    #[cfg(nexus_env = "os")]
    {
        // Ensure default target is set for this service
        nexus_ipc::set_default_target("vfsd");
        let server = nexus_ipc::KernelServer::new().map_err(TransportError::from)?;
        let mut transport = IpcTransport::new(server);
        // Packagefs client targets the packagefsd service
        let _ = nexus_ipc::set_default_target("packagefsd");
        let packagefs = PackageFsClient::new().map(Arc::new).map_err(|err| {
            ServerError::Service(ServiceError::Provider(format!(
                "packagefs client init: {err}"
            )))
        })?;
        nexus_ipc::set_default_target("vfsd");
        let dispatcher = Arc::new(Dispatcher::new(packagefs));
        notifier.notify();
        run_loop(&mut transport, dispatcher)
    }
}

/// Runs the service with an injected transport and packagefs client.
pub fn run_with_transport<T: Transport>(
    transport: &mut T,
    packagefs: Arc<PackageFsClient>,
) -> Result<()>
where
    T: Transport,
{
    let dispatcher = Arc::new(Dispatcher::new(packagefs));
    run_loop(transport, dispatcher)
}

/// Creates a loopback transport pair for host tests.
#[cfg(nexus_env = "host")]
pub fn loopback_transport() -> (
    nexus_ipc::LoopbackClient,
    IpcTransport<nexus_ipc::LoopbackServer>,
) {
    let (client, server) = nexus_ipc::loopback_channel();
    (client, IpcTransport::new(server))
}

fn run_loop<T>(transport: &mut T, dispatcher: Arc<Dispatcher>) -> Result<()>
where
    T: Transport,
{
    println!("vfsd: ready");
    while let Some(frame) = transport
        .recv()
        .map_err(|err| ServerError::Transport(err.into()))?
    {
        if frame.is_empty() {
            continue;
        }
        if let Err(err) = handle_frame(&dispatcher, transport, &frame) {
            error!("vfsd: handle error: {err}");
        }
    }
    Ok(())
}

fn handle_frame<T>(dispatcher: &Dispatcher, transport: &mut T, frame: &[u8]) -> Result<()>
where
    T: Transport,
{
    let (opcode, payload) = frame
        .split_first()
        .ok_or_else(|| ServerError::Decode("empty frame".into()))?;
    let opcode = *opcode;
    let response = match opcode {
        OPCODE_OPEN => handle_open(dispatcher, payload)?,
        OPCODE_READ => handle_read(dispatcher, payload)?,
        OPCODE_CLOSE => handle_close(dispatcher, payload)?,
        OPCODE_STAT => handle_stat(dispatcher, payload)?,
        OPCODE_MOUNT => handle_mount(dispatcher, payload)?,
        other => {
            error!("vfsd: unknown opcode {other}");
            return Ok(());
        }
    };
    transport
        .send(&response)
        .map_err(|err| ServerError::Transport(err.into()))
}

fn handle_open(dispatcher: &Dispatcher, payload: &[u8]) -> Result<Vec<u8>> {
    let mut cursor = std::io::Cursor::new(payload);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|err| ServerError::Decode(format!("open read: {err}")))?;
    let request = message
        .get_root::<open_request::Reader<'_>>()
        .map_err(|err| ServerError::Decode(format!("open root: {err}")))?;
    let path = request
        .get_path()
        .map_err(|err| ServerError::Decode(format!("open path: {err}")))?
        .to_str()
        .map_err(|err| ServerError::Decode(format!("open path utf8: {err}")))?
        .to_string();
    match dispatcher.open(&path) {
        Ok((handle, entry)) => encode_open_response(true, handle, entry.size, entry.kind),
        Err(ServerError::Service(ServiceError::NotFound)) => encode_open_response(false, 0, 0, 0),
        Err(ServerError::Service(ServiceError::InvalidPath)) => {
            encode_open_response(false, 0, 0, 0)
        }
        Err(err) => Err(err),
    }
}

fn handle_read(dispatcher: &Dispatcher, payload: &[u8]) -> Result<Vec<u8>> {
    let mut cursor = std::io::Cursor::new(payload);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|err| ServerError::Decode(format!("read read: {err}")))?;
    let request = message
        .get_root::<read_request::Reader<'_>>()
        .map_err(|err| ServerError::Decode(format!("read root: {err}")))?;
    let handle = request.get_fh();
    let off = request.get_off();
    let len = request.get_len();
    match dispatcher.read(handle, off, len) {
        Ok(bytes) => encode_read_response(true, &bytes),
        Err(ServerError::Service(ServiceError::BadHandle)) => encode_read_response(false, &[]),
        Err(err) => Err(err),
    }
}

fn handle_close(dispatcher: &Dispatcher, payload: &[u8]) -> Result<Vec<u8>> {
    let mut cursor = std::io::Cursor::new(payload);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|err| ServerError::Decode(format!("close read: {err}")))?;
    let request = message
        .get_root::<close_request::Reader<'_>>()
        .map_err(|err| ServerError::Decode(format!("close root: {err}")))?;
    let handle = request.get_fh();
    match dispatcher.close(handle) {
        Ok(()) => encode_close_response(true),
        Err(ServerError::Service(ServiceError::BadHandle)) => encode_close_response(false),
        Err(err) => Err(err),
    }
}

fn handle_stat(dispatcher: &Dispatcher, payload: &[u8]) -> Result<Vec<u8>> {
    let mut cursor = std::io::Cursor::new(payload);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|err| ServerError::Decode(format!("stat read: {err}")))?;
    let request = message
        .get_root::<stat_request::Reader<'_>>()
        .map_err(|err| ServerError::Decode(format!("stat root: {err}")))?;
    let path = request
        .get_path()
        .map_err(|err| ServerError::Decode(format!("stat path: {err}")))?
        .to_str()
        .map_err(|err| ServerError::Decode(format!("stat path utf8: {err}")))?
        .to_string();
    match dispatcher.stat(&path) {
        Ok((size, kind)) => encode_stat_response(true, size, kind),
        Err(ServerError::Service(ServiceError::NotFound)) => encode_stat_response(false, 0, 0),
        Err(ServerError::Service(ServiceError::InvalidPath)) => encode_stat_response(false, 0, 0),
        Err(err) => Err(err),
    }
}

fn handle_mount(dispatcher: &Dispatcher, payload: &[u8]) -> Result<Vec<u8>> {
    let mut cursor = std::io::Cursor::new(payload);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|err| ServerError::Decode(format!("mount read: {err}")))?;
    let request = message
        .get_root::<mount_request::Reader<'_>>()
        .map_err(|err| ServerError::Decode(format!("mount root: {err}")))?;
    let mount_point = request
        .get_mount_point()
        .map_err(|err| ServerError::Decode(format!("mount mount_point: {err}")))?
        .to_str()
        .map_err(|err| ServerError::Decode(format!("mount mount_point utf8: {err}")))?
        .to_string();
    let fs_id = request
        .get_fs_id()
        .map_err(|err| ServerError::Decode(format!("mount fs_id: {err}")))?
        .to_str()
        .map_err(|err| ServerError::Decode(format!("mount fs_id utf8: {err}")))?
        .to_string();
    match dispatcher.mount(&mount_point, &fs_id) {
        Ok(()) => encode_mount_response(true, String::new()),
        Err(ServerError::Service(ServiceError::Provider(msg))) => encode_mount_response(false, msg),
        Err(err) => Err(err),
    }
}

fn encode_frame(
    opcode: u8,
    build: impl FnOnce(
        &mut capnp::message::Builder<capnp::message::HeapAllocator>,
    ) -> core::result::Result<(), capnp::Error>,
) -> Result<Vec<u8>> {
    let mut message = capnp::message::Builder::new_default();
    build(&mut message).map_err(ServerError::Encode)?;
    let mut payload = Vec::new();
    capnp::serialize::write_message(&mut payload, &message).map_err(ServerError::Encode)?;
    let mut frame = Vec::with_capacity(1 + payload.len());
    frame.push(opcode);
    frame.extend_from_slice(&payload);
    Ok(frame)
}

fn encode_open_response(success: bool, handle: u32, size: u64, kind: u16) -> Result<Vec<u8>> {
    encode_frame(OPCODE_OPEN, |message| {
        let mut resp = message.init_root::<open_response::Builder<'_>>();
        resp.set_ok(success);
        resp.set_fh(handle);
        resp.set_size(size);
        resp.set_kind(kind);
        Ok(())
    })
}

fn encode_read_response(success: bool, bytes: &[u8]) -> Result<Vec<u8>> {
    encode_frame(OPCODE_READ, |message| {
        let mut resp = message.init_root::<read_response::Builder<'_>>();
        resp.set_ok(success);
        resp.set_bytes(bytes);
        Ok(())
    })
}

fn encode_close_response(success: bool) -> Result<Vec<u8>> {
    encode_frame(OPCODE_CLOSE, |message| {
        let mut resp = message.init_root::<close_response::Builder<'_>>();
        resp.set_ok(success);
        Ok(())
    })
}

fn encode_stat_response(success: bool, size: u64, kind: u16) -> Result<Vec<u8>> {
    encode_frame(OPCODE_STAT, |message| {
        let mut resp = message.init_root::<stat_response::Builder<'_>>();
        resp.set_ok(success);
        resp.set_size(size);
        resp.set_kind(kind);
        Ok(())
    })
}

fn encode_mount_response(success: bool, msg: String) -> Result<Vec<u8>> {
    encode_frame(OPCODE_MOUNT, |message| {
        let mut resp = message.init_root::<mount_response::Builder<'_>>();
        let _ = msg; // suppress unused until mount response gains fields
        resp.set_ok(success);
        Ok(())
    })
}
