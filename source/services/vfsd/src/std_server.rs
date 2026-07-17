// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Standard (non-lite) vfsd server with Cap'n Proto IDL dispatch, mount table
//! abstraction, pluggable FsProvider backends, and capability-based file handles with
//! sandbox enforcement. Supports open, read, close, stat, and mount operations.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 test
//! ADR: docs/adr/0004-idl-runtime-architecture.md

use std::collections::HashMap;
use std::sync::Arc;

use capnp::message::ReaderOptions;
use capnp::serialize;
use log::{error, info};
use parking_lot::Mutex;
use thiserror::Error;

use nexus_idl_runtime::vfs_capnp::{
    close_request, close_response, mount_request, mount_response, open_request, open_response,
    read_dir_request, read_dir_response, read_request, read_response, stat_request, stat_response,
};
use nexus_ipc::{IpcError, Wait};
use nexus_packagefs::PackageFsClient;
use nexus_vfs_types::{ReadDirPage, VfsError, CODE_OK, MAX_ENTRIES_PER_PAGE};

use crate::{CapFdToken, NamespaceView, ReplayGuard, SandboxError, RIGHT_READ, RIGHT_WRITE};

const OPCODE_OPEN: u8 = 1;
const OPCODE_READ: u8 = 2;
const OPCODE_CLOSE: u8 = 3;
const OPCODE_STAT: u8 = 4;
const OPCODE_MOUNT: u8 = 5;
const OPCODE_READDIR: u8 = 6;

const KIND_DIRECTORY: u16 = 1;

/// The provider-relative path addressing a mount's root listing.
const DIR_ROOT_REL: &str = ".";

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
    /// Stable storage error passed through from a provider (RFC-0072).
    #[error("vfs error: {0}")]
    Vfs(VfsError),
    /// Underlying provider failed.
    #[error("provider error: {0}")]
    Provider(String),
}

impl ServiceError {
    /// Maps to the stable RFC-0072 wire code.
    fn code(&self) -> u16 {
        match self {
            Self::NotFound => VfsError::NotFound.code(),
            Self::InvalidPath => VfsError::Invalid.code(),
            Self::BadHandle => VfsError::Invalid.code(),
            Self::Vfs(err) => err.code(),
            Self::Provider(_) => VfsError::Io.code(),
        }
    }
}

/// Read-only file metadata stored for active handles.
#[derive(Clone)]
struct HandleEntry {
    bytes: Vec<u8>,
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
    /// Lists direct children of `rel_path` (`"."` = mount root), paginated.
    fn read_dir(
        &self,
        rel_path: &str,
        cursor: u32,
        limit: u16,
    ) -> core::result::Result<ReadDirPage, ServiceError>;
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
        Ok(ProviderEntry { bytes: entry.bytes().to_vec(), size: entry.size(), kind: entry.kind() })
    }

    fn stat(&self, rel_path: &str) -> core::result::Result<(u64, u16), ServiceError> {
        let entry = self.client.resolve(rel_path).map_err(map_packagefs_error)?;
        Ok((entry.size(), entry.kind()))
    }

    fn read_dir(
        &self,
        rel_path: &str,
        cursor: u32,
        limit: u16,
    ) -> core::result::Result<ReadDirPage, ServiceError> {
        self.client.list(rel_path, cursor, limit).map_err(ServiceError::Vfs)
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
        Self { mounts: HashMap::new() }
    }

    fn mount(&mut self, path: &str, provider: Box<dyn FsProvider>) {
        self.mounts.insert(path.to_string(), provider);
    }

    fn resolve<'a>(
        &'a self,
        path: &str,
    ) -> core::result::Result<(&'a dyn FsProvider, String), ServiceError> {
        let path = sanitize_dispatch_path(path)?;
        if let Some(rest) = path.strip_prefix("pkg:/") {
            let provider = self.mounts.get("/packages").ok_or(ServiceError::InvalidPath)?;
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
                if best.is_none_or(|(_, best_len)| len > best_len) {
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

    /// Like [`Self::resolve`], but an empty relative path is valid and maps to
    /// the mount root (`"."`) — directories are listable at every level.
    fn resolve_dir<'a>(
        &'a self,
        path: &str,
    ) -> core::result::Result<(&'a dyn FsProvider, String), ServiceError> {
        if path == "pkg:/" {
            let provider = self.mounts.get("/packages").ok_or(ServiceError::NotFound)?;
            return Ok((&**provider, DIR_ROOT_REL.to_string()));
        }
        match self.resolve(path) {
            Ok(resolved) => Ok(resolved),
            Err(ServiceError::InvalidPath) => {
                // Retry as a mount-root listing: "/packages" has an empty rel.
                let trimmed = path.trim_start_matches('/').trim_end_matches('/');
                if trimmed.is_empty() {
                    return Err(ServiceError::InvalidPath);
                }
                for (mount, provider) in &self.mounts {
                    if mount.trim_start_matches('/').trim_end_matches('/') == trimmed {
                        return Ok((&**provider, DIR_ROOT_REL.to_string()));
                    }
                }
                Err(ServiceError::InvalidPath)
            }
            Err(err) => Err(err),
        }
    }
}

fn sanitize_dispatch_path(path: &str) -> core::result::Result<String, ServiceError> {
    if path.contains(":/") {
        let ns = NamespaceView::new(vec!["pkg:/".to_string()]);
        return ns.assert_allowed(path).map_err(map_sandbox_path_error);
    }
    let mut out = String::new();
    out.push('/');
    let mut wrote_segment = false;
    for seg in path.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            return Err(ServiceError::InvalidPath);
        }
        if wrote_segment {
            out.push('/');
        }
        out.push_str(seg);
        wrote_segment = true;
    }
    if !wrote_segment {
        return Err(ServiceError::InvalidPath);
    }
    Ok(out)
}

fn map_sandbox_path_error(err: SandboxError) -> ServiceError {
    match err {
        SandboxError::Traversal | SandboxError::OutOfNamespace | SandboxError::InvalidPath => {
            ServiceError::InvalidPath
        }
        _ => ServiceError::Provider("sandbox path validation failed".to_string()),
    }
}

fn map_sandbox_cap_error(err: SandboxError) -> ServerError {
    match err {
        SandboxError::Integrity
        | SandboxError::Replay
        | SandboxError::Rights
        | SandboxError::Subject
        | SandboxError::Expired => ServiceError::BadHandle.into(),
        SandboxError::InvalidPath | SandboxError::Traversal | SandboxError::OutOfNamespace => {
            ServiceError::InvalidPath.into()
        }
    }
}

/// Shared dispatcher state.
struct Dispatcher {
    mounts: Mutex<MountTable>,
    handles: Mutex<HashMap<u32, HandleEntry>>,
    cap_tokens: Mutex<HashMap<u32, CapFdToken>>,
    replay_guard: Mutex<ReplayGuard>,
    mac_key: [u8; 32],
    next_handle: Mutex<u32>,
    next_nonce: Mutex<u64>,
    packagefs: Arc<PackageFsClient>,
}

impl Dispatcher {
    fn new(packagefs: Arc<PackageFsClient>) -> Self {
        let mut mounts = MountTable::new();
        mounts.mount("/packages", Box::new(PackageFsProvider::new(packagefs.clone())));
        info!("vfsd: mounted /packages -> packagefs");
        Self {
            mounts: Mutex::new(mounts),
            handles: Mutex::new(HashMap::new()),
            cap_tokens: Mutex::new(HashMap::new()),
            replay_guard: Mutex::new(ReplayGuard::default()),
            mac_key: [0xA5; 32],
            next_handle: Mutex::new(1),
            next_nonce: Mutex::new(1),
            packagefs,
        }
    }

    fn mount(&self, mount_point: &str, fs_id: &str) -> Result<()> {
        let mut mounts = self.mounts.lock();
        match fs_id {
            "packagefs" => {
                mounts.mount(mount_point, Box::new(PackageFsProvider::new(self.packagefs.clone())));
                Ok(())
            }
            other => {
                Err(ServerError::Service(ServiceError::Provider(format!("unknown fs id {other}"))))
            }
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
        let mut cap_tokens = self.cap_tokens.lock();
        let mut next = self.next_handle.lock();
        let mut next_nonce = self.next_nonce.lock();
        let handle = *next;
        *next = next.saturating_add(1).max(1);
        handles.insert(handle, HandleEntry { bytes: entry.bytes.clone() });
        cap_tokens.insert(
            handle,
            CapFdToken::mint(&self.mac_key, 0, path.to_string(), RIGHT_READ, *next_nonce, u64::MAX),
        );
        *next_nonce = next_nonce.saturating_add(1).max(1);
        Ok((handle, entry))
    }

    fn read(&self, handle: u32, off: u64, len: u32) -> Result<Vec<u8>> {
        let handles = self.handles.lock();
        let entry = handles.get(&handle).ok_or(ServiceError::BadHandle)?;
        let mut cap_tokens = self.cap_tokens.lock();
        let token = cap_tokens.get(&handle).cloned().ok_or(ServiceError::BadHandle)?;
        if (token.rights & RIGHT_WRITE) != 0 {
            return Err(ServiceError::BadHandle.into());
        }
        {
            let mut replay = self.replay_guard.lock();
            replay
                .verify(&self.mac_key, &token, 0, RIGHT_READ, 0)
                .map_err(map_sandbox_cap_error)?;
        }
        let slice = entry.read(off, len);
        let mut next_nonce = self.next_nonce.lock();
        cap_tokens.insert(
            handle,
            CapFdToken::mint(
                &self.mac_key,
                token.subject_id,
                token.canonical_path,
                token.rights,
                *next_nonce,
                token.expires_at,
            ),
        );
        *next_nonce = next_nonce.saturating_add(1).max(1);
        Ok(slice.to_vec())
    }

    fn close(&self, handle: u32) -> Result<()> {
        let mut handles = self.handles.lock();
        let mut cap_tokens = self.cap_tokens.lock();
        if handles.remove(&handle).is_some() {
            cap_tokens.remove(&handle);
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

    fn read_dir(&self, path: &str, cursor: u32, limit: u16) -> Result<ReadDirPage> {
        let limit = limit.clamp(1, MAX_ENTRIES_PER_PAGE);
        let mounts = self.mounts.lock();
        let (provider, rel) = mounts.resolve_dir(path)?;
        provider.read_dir(&rel, cursor, limit).map_err(ServerError::from)
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
            ServerError::Service(ServiceError::Provider(format!("packagefs client init: {err}")))
        })?;
        nexus_ipc::set_default_target("vfsd");
        let dispatcher = Arc::new(Dispatcher::new(packagefs));
        notifier.notify();
        run_loop(&mut transport, dispatcher)
    }
}

/// Runs the service with an injected transport and packagefs client.
pub fn run_with_transport<T>(transport: &mut T, packagefs: Arc<PackageFsClient>) -> Result<()>
where
    T: Transport,
{
    let dispatcher = Arc::new(Dispatcher::new(packagefs));
    run_loop(transport, dispatcher)
}

/// Creates a loopback transport pair for host tests.
#[cfg(nexus_env = "host")]
pub fn loopback_transport() -> (nexus_ipc::LoopbackClient, IpcTransport<nexus_ipc::LoopbackServer>)
{
    let (client, server) = nexus_ipc::loopback_channel();
    (client, IpcTransport::new(server))
}

fn run_loop<T>(transport: &mut T, dispatcher: Arc<Dispatcher>) -> Result<()>
where
    T: Transport,
{
    println!("vfsd: ready");
    while let Some(frame) = transport.recv().map_err(|err| ServerError::Transport(err.into()))? {
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
    let (opcode, payload) =
        frame.split_first().ok_or_else(|| ServerError::Decode("empty frame".into()))?;
    let opcode = *opcode;
    let response = match opcode {
        OPCODE_OPEN => handle_open(dispatcher, payload)?,
        OPCODE_READ => handle_read(dispatcher, payload)?,
        OPCODE_CLOSE => handle_close(dispatcher, payload)?,
        OPCODE_STAT => handle_stat(dispatcher, payload)?,
        OPCODE_MOUNT => handle_mount(dispatcher, payload)?,
        OPCODE_READDIR => handle_readdir(dispatcher, payload)?,
        other => {
            error!("vfsd: unknown opcode {other}");
            return Ok(());
        }
    };
    transport.send(&response).map_err(|err| ServerError::Transport(err.into()))
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
        Ok((handle, entry)) => encode_open_response(true, handle, entry.size, entry.kind, CODE_OK),
        Err(ServerError::Service(err)) => encode_open_response(false, 0, 0, 0, err.code()),
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
        Ok(bytes) => encode_read_response(true, &bytes, CODE_OK),
        Err(ServerError::Service(err)) => encode_read_response(false, &[], err.code()),
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
        Ok(()) => encode_close_response(true, CODE_OK),
        Err(ServerError::Service(err)) => encode_close_response(false, err.code()),
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
        Ok((size, kind)) => encode_stat_response(true, size, kind, CODE_OK),
        Err(ServerError::Service(err)) => encode_stat_response(false, 0, 0, err.code()),
        Err(err) => Err(err),
    }
}

fn handle_readdir(dispatcher: &Dispatcher, payload: &[u8]) -> Result<Vec<u8>> {
    let mut cursor = std::io::Cursor::new(payload);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|err| ServerError::Decode(format!("readdir read: {err}")))?;
    let request = message
        .get_root::<read_dir_request::Reader<'_>>()
        .map_err(|err| ServerError::Decode(format!("readdir root: {err}")))?;
    let path = request
        .get_path()
        .map_err(|err| ServerError::Decode(format!("readdir path: {err}")))?
        .to_str()
        .map_err(|err| ServerError::Decode(format!("readdir path utf8: {err}")))?
        .to_string();
    match dispatcher.read_dir(&path, request.get_cursor(), request.get_limit()) {
        Ok(page) => {
            info!("vfsd: readdir ok (path={path} entries={})", page.entries.len());
            encode_readdir_response(Ok(&page))
        }
        Err(ServerError::Service(err)) => encode_readdir_response(Err(err.code())),
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

fn encode_open_response(
    success: bool,
    handle: u32,
    size: u64,
    kind: u16,
    err: u16,
) -> Result<Vec<u8>> {
    encode_frame(OPCODE_OPEN, |message| {
        let mut resp = message.init_root::<open_response::Builder<'_>>();
        resp.set_ok(success);
        resp.set_fh(handle);
        resp.set_size(size);
        resp.set_kind(kind);
        resp.set_err(err);
        Ok(())
    })
}

fn encode_read_response(success: bool, bytes: &[u8], err: u16) -> Result<Vec<u8>> {
    encode_frame(OPCODE_READ, |message| {
        let mut resp = message.init_root::<read_response::Builder<'_>>();
        resp.set_ok(success);
        resp.set_bytes(bytes);
        resp.set_err(err);
        Ok(())
    })
}

fn encode_close_response(success: bool, err: u16) -> Result<Vec<u8>> {
    encode_frame(OPCODE_CLOSE, |message| {
        let mut resp = message.init_root::<close_response::Builder<'_>>();
        resp.set_ok(success);
        resp.set_err(err);
        Ok(())
    })
}

fn encode_stat_response(success: bool, size: u64, kind: u16, err: u16) -> Result<Vec<u8>> {
    encode_frame(OPCODE_STAT, |message| {
        let mut resp = message.init_root::<stat_response::Builder<'_>>();
        resp.set_ok(success);
        resp.set_size(size);
        resp.set_kind(kind);
        resp.set_err(err);
        Ok(())
    })
}

fn encode_mount_response(success: bool, msg: String) -> Result<Vec<u8>> {
    encode_frame(OPCODE_MOUNT, |message| {
        let mut resp = message.init_root::<mount_response::Builder<'_>>();
        let _ = msg; // suppress unused until mount response gains fields
        resp.set_ok(success);
        resp.set_err(if success { CODE_OK } else { VfsError::Io.code() });
        Ok(())
    })
}

fn encode_readdir_response(outcome: core::result::Result<&ReadDirPage, u16>) -> Result<Vec<u8>> {
    encode_frame(OPCODE_READDIR, |message| {
        let mut resp = message.init_root::<read_dir_response::Builder<'_>>();
        match outcome {
            Ok(page) => {
                resp.set_ok(true);
                resp.set_err(CODE_OK);
                resp.set_next_cursor(page.next_cursor);
                resp.set_eof(page.eof);
                let mut list = resp.init_entries(page.entries.len() as u32);
                for (idx, entry) in page.entries.iter().enumerate() {
                    let mut slot = list.reborrow().get(idx as u32);
                    slot.set_name(entry.name.as_str());
                    slot.set_kind(entry.kind as u16);
                    slot.set_size(entry.size);
                }
            }
            Err(code) => {
                resp.set_ok(false);
                resp.set_err(code);
                resp.set_next_cursor(0);
                resp.set_eof(true);
                resp.init_entries(0);
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_packagefs::PackageFsClient;

    struct StaticProvider {
        bytes: Vec<u8>,
    }

    impl FsProvider for StaticProvider {
        fn resolve(&self, _rel_path: &str) -> core::result::Result<ProviderEntry, ServiceError> {
            Ok(ProviderEntry { bytes: self.bytes.clone(), size: self.bytes.len() as u64, kind: 0 })
        }

        fn stat(&self, _rel_path: &str) -> core::result::Result<(u64, u16), ServiceError> {
            Ok((self.bytes.len() as u64, 0))
        }

        fn read_dir(
            &self,
            rel_path: &str,
            cursor: u32,
            _limit: u16,
        ) -> core::result::Result<ReadDirPage, ServiceError> {
            if rel_path != DIR_ROOT_REL {
                return Err(ServiceError::Vfs(nexus_vfs_types::VfsError::NotFound));
            }
            let entries = alloc_entries();
            let start = cursor as usize;
            let page: Vec<_> = entries.iter().skip(start).cloned().collect();
            Ok(ReadDirPage { next_cursor: (start + page.len()) as u32, eof: true, entries: page })
        }
    }

    fn alloc_entries() -> Vec<nexus_vfs_types::DirEntry> {
        vec![
            nexus_vfs_types::DirEntry {
                name: "build.prop".to_string(),
                kind: nexus_vfs_types::FileKind::File,
                size: 19,
            },
            nexus_vfs_types::DirEntry {
                name: "system".to_string(),
                kind: nexus_vfs_types::FileKind::Dir,
                size: 0,
            },
        ]
    }

    fn test_dispatcher() -> Dispatcher {
        let (pkg_client, _pkg_server) = nexus_ipc::loopback_channel();
        let packagefs = Arc::new(PackageFsClient::from_loopback(pkg_client));
        let mut mounts = MountTable::new();
        mounts.mount(
            "/packages",
            Box::new(StaticProvider { bytes: b"ro.nexus.build=dev\n".to_vec() }),
        );
        Dispatcher {
            mounts: Mutex::new(mounts),
            handles: Mutex::new(HashMap::new()),
            cap_tokens: Mutex::new(HashMap::new()),
            replay_guard: Mutex::new(ReplayGuard::default()),
            mac_key: [0xA5; 32],
            next_handle: Mutex::new(1),
            next_nonce: Mutex::new(1),
            packagefs,
        }
    }

    #[test]
    fn readdir_lists_mount_root_and_maps_errors() {
        let dispatcher = test_dispatcher();
        // Mount-root listing via both spellings.
        for path in ["/packages", "pkg:/"] {
            let page = dispatcher.read_dir(path, 0, 64).expect("root listing");
            let names: Vec<&str> = page.entries.iter().map(|e| e.name.as_str()).collect();
            assert_eq!(names, ["build.prop", "system"], "path {path}");
            assert!(page.eof);
        }
        // Unknown mount → stable InvalidPath (EINVAL on the wire).
        let err = dispatcher.read_dir("/nope", 0, 64).expect_err("unknown mount");
        match err {
            ServerError::Service(service) => {
                assert_eq!(service.code(), nexus_vfs_types::VfsError::Invalid.code());
            }
            other => panic!("unexpected error {other}"),
        }
        // Provider-side NotFound passes through with its stable code.
        let err = dispatcher.read_dir("pkg:/system/missing", 0, 64).expect_err("not found");
        match err {
            ServerError::Service(service) => {
                assert_eq!(service.code(), nexus_vfs_types::VfsError::NotFound.code());
            }
            other => panic!("unexpected error {other}"),
        }
    }

    #[test]
    fn test_reject_forged_capfd_service_path() {
        let dispatcher = test_dispatcher();
        let (handle, _) = dispatcher.open("pkg:/system/build.prop").expect("open must succeed");

        {
            let mut tokens = dispatcher.cap_tokens.lock();
            let token = tokens.get_mut(&handle).expect("token must exist");
            token.mac[0] ^= 0x01;
        }

        let err = dispatcher.read(handle, 0, 8).expect_err("forged capfd must be rejected");
        assert!(matches!(err, ServerError::Service(ServiceError::BadHandle)));
    }
}
