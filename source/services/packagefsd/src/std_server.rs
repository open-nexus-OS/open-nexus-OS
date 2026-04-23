//! CONTEXT: Host/std packagefs daemon transport + registry + pkgimg v2 mount integration.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Unit tests for publish path and pkgimg v2 mount contract/reject behavior.

use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, OnceLock};

use capnp::message::ReaderOptions;
use capnp::serialize;
use log::{error, info};
use parking_lot::Mutex;
use storage::pkgimg::{parse_pkgimg, PkgImgCaps};
use thiserror::Error;

use nexus_idl_runtime::packagefs_capnp::{
    publish_bundle, publish_response, resolve_path, resolve_response,
};
use nexus_ipc::{IpcError, Wait};

const KIND_FILE: u16 = 0;
const KIND_DIRECTORY: u16 = 1;

const OPCODE_PUBLISH: u8 = 1;
const OPCODE_RESOLVE: u8 = 2;

/// Result alias for operations in this crate.
pub type Result<T> = core::result::Result<T, ServerError>;

/// Notifies init once the daemon is ready.
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    /// Creates a new notifier from `func`.
    pub fn new<F>(func: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self(Box::new(func))
    }

    /// Emits the ready signal.
    pub fn notify(self) {
        (self.0)();
    }
}

/// Errors emitted while serving requests.
#[derive(Debug, Error)]
pub enum ServerError {
    /// Transport level failure.
    #[error("transport error: {0}")]
    Transport(TransportError),
    /// Failed to decode a request frame.
    #[error("decode error: {0}")]
    Decode(String),
    /// Failed to encode a response frame.
    #[error("encode error: {0}")]
    Encode(capnp::Error),
    /// Registry level error.
    #[error("service error: {0}")]
    Service(ServiceError),
    /// Failed to mount pkgimg v2 image.
    #[error("pkgimg mount error: {0}")]
    PkgImg(String),
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

/// Transport abstraction used by the daemon.
pub trait Transport {
    /// Error type returned by the transport.
    type Error: Into<TransportError>;

    /// Receives the next frame.
    fn recv(&mut self) -> core::result::Result<Option<Vec<u8>>, Self::Error>;

    /// Sends a response frame back to the caller.
    fn send(&mut self, frame: &[u8]) -> core::result::Result<(), Self::Error>;
}

/// Transport backed by [`nexus_ipc`].
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

/// Transport level errors.
#[derive(Debug, Error)]
pub enum TransportError {
    /// Connection closed.
    #[error("transport closed")]
    Closed,
    /// I/O failure.
    #[error("io error: {0}")]
    Io(String),
    /// Unsupported configuration.
    #[error("transport unsupported")]
    Unsupported,
    /// Any other failure.
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

/// Errors produced by the registry.
#[derive(Debug, Error)]
pub enum ServiceError {
    /// The requested bundle does not exist.
    #[error("bundle not found")]
    NotFound,
    /// Path is malformed.
    #[error("invalid path")]
    InvalidPath,
}

/// Metadata describing files within a bundle.
#[derive(Clone, Debug)]
pub struct FileEntry {
    path: String,
    kind: u16,
    bytes: Vec<u8>,
}

impl FileEntry {
    /// Constructs a new metadata entry.
    pub fn new(path: &str, kind: u16, bytes: Vec<u8>) -> Self {
        Self { path: path.to_string(), kind, bytes }
    }

    /// Creates a directory entry.
    pub fn directory(path: &str) -> Self {
        Self { path: path.to_string(), kind: KIND_DIRECTORY, bytes: Vec::new() }
    }

    fn size(&self) -> u64 {
        self.bytes.len() as u64
    }
}

#[derive(Clone, Default)]
struct BundleRecord {
    files: HashMap<String, FileEntry>,
}

impl BundleRecord {
    fn replace_entries(
        &mut self,
        entries: Vec<FileEntry>,
    ) -> core::result::Result<(), ServiceError> {
        self.files.clear();
        for entry in entries {
            let FileEntry { path, kind, bytes } = entry;
            let sanitized = sanitize_entry_path(&path)?;
            let stored = FileEntry::new(&sanitized, kind, bytes);
            self.ensure_parent_dirs(&sanitized);
            self.files.insert(sanitized, stored);
        }
        Ok(())
    }

    fn ensure_parent_dirs(&mut self, path: &str) {
        let segments: Vec<&str> = path.split('/').collect();
        if segments.len() <= 1 {
            return;
        }
        let mut prefix = String::new();
        for segment in &segments[..segments.len() - 1] {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(segment);
            let key = prefix.clone();
            self.files.entry(key.clone()).or_insert_with(|| FileEntry::directory(&key));
        }
    }

    fn lookup(&self, rel: &str) -> core::result::Result<ResolvedEntry, ServiceError> {
        self.files
            .get(rel)
            .cloned()
            .map(|entry| ResolvedEntry { size: entry.size(), kind: entry.kind, bytes: entry.bytes })
            .ok_or(ServiceError::NotFound)
    }
}

/// Registry tracking published bundles.
#[derive(Clone, Default)]
pub struct BundleRegistry {
    bundles: Arc<Mutex<HashMap<String, BundleRecord>>>,
    active: Arc<Mutex<HashMap<String, String>>>,
}

impl BundleRegistry {
    /// Returns the global registry instance.
    pub fn global() -> &'static BundleRegistry {
        static REGISTRY: OnceLock<BundleRegistry> = OnceLock::new();
        REGISTRY.get_or_init(BundleRegistry::default)
    }

    /// Publishes bundle entries and marks the version active.
    pub fn publish_bundle(
        &self,
        name: &str,
        version: &str,
        entries: Vec<FileEntry>,
    ) -> core::result::Result<(), ServiceError> {
        let key = format!("{name}@{version}");
        let mut guard = self.bundles.lock();
        let record = guard.entry(key).or_default();
        record.replace_entries(entries)?;
        drop(guard);

        let mut active = self.active.lock();
        active.insert(name.to_string(), version.to_string());
        Ok(())
    }

    fn resolve(&self, rel: &str) -> core::result::Result<ResolvedEntry, ServiceError> {
        let trimmed = rel.trim_start_matches('/');
        let mut parts = trimmed.splitn(2, '/');
        let bundle = parts.next().ok_or(ServiceError::InvalidPath)?;
        let path = parts.next().ok_or(ServiceError::InvalidPath)?;
        let canonical = if bundle.contains('@') {
            bundle.to_string()
        } else {
            let active = self.active.lock();
            let version = active.get(bundle).ok_or(ServiceError::NotFound)?;
            format!("{bundle}@{version}")
        };
        let guard = self.bundles.lock();
        let record = guard.get(&canonical).ok_or(ServiceError::NotFound)?;
        let path = sanitize_entry_path(path)?;
        record.lookup(&path)
    }
}

/// Metadata returned by [`BundleRegistry::resolve`].
#[derive(Debug, Clone)]
pub struct ResolvedEntry {
    /// File size in bytes.
    pub size: u64,
    /// File type identifier.
    pub kind: u16,
    /// File data returned for regular files.
    pub bytes: Vec<u8>,
}

struct ServiceState {
    registry: BundleRegistry,
}

impl ServiceState {
    fn new(registry: BundleRegistry) -> Self {
        Self { registry }
    }
}

/// Runs the service using the default transport.
pub fn service_main_loop(notifier: ReadyNotifier) -> Result<()> {
    #[cfg(nexus_env = "host")]
    {
        let (client, server) = nexus_ipc::loopback_channel();
        let registry = BundleRegistry::global().clone();
        if let Err(err) = try_mount_pkgimg_from_env(&registry) {
            error!("packagefsd: pkgimg mount failed: {err}");
        }
        notifier.notify();
        let _client_guard = client;
        let mut transport = IpcTransport::new(server);
        run_loop(&mut transport, registry)
    }

    #[cfg(nexus_env = "os")]
    {
        nexus_ipc::set_default_target("packagefsd");
        let server = nexus_ipc::KernelServer::new().map_err(TransportError::from)?;
        let mut transport = IpcTransport::new(server);
        let registry = BundleRegistry::global().clone();
        if let Err(err) = try_mount_pkgimg_from_env(&registry) {
            error!("packagefsd: pkgimg mount failed: {err}");
        }
        notifier.notify();
        run_loop(&mut transport, registry)
    }
}

fn try_mount_pkgimg_from_env(registry: &BundleRegistry) -> Result<()> {
    let Some(path) = std::env::var_os("PACKAGEFSD_PKGIMG_PATH") else {
        return Ok(());
    };
    let bytes = fs::read(&path)
        .map_err(|err| ServerError::PkgImg(format!("read {}: {err}", path.to_string_lossy())))?;
    mount_pkgimg_bytes(registry, &bytes)
}

fn mount_pkgimg_bytes(registry: &BundleRegistry, bytes: &[u8]) -> Result<()> {
    let parsed = parse_pkgimg(bytes, PkgImgCaps::default())
        .map_err(|err| ServerError::PkgImg(format!("parse pkgimg: {err}")))?;
    let mut grouped: HashMap<(String, String), Vec<FileEntry>> = HashMap::new();
    for entry in parsed.entries() {
        let payload = parsed
            .read(&entry.bundle, &entry.version, &entry.path)
            .ok_or_else(|| ServerError::PkgImg("entry payload missing".to_string()))?;
        grouped
            .entry((entry.bundle.clone(), entry.version.clone()))
            .or_default()
            .push(FileEntry::new(&entry.path, KIND_FILE, payload.to_vec()));
    }
    for ((bundle, version), entries) in grouped {
        registry.publish_bundle(&bundle, &version, entries)?;
    }
    println!("packagefsd: v2 mounted (pkgimg)");
    Ok(())
}

/// Runs the service with an injected transport and registry instance.
pub fn run_with_transport<T>(transport: &mut T, registry: BundleRegistry) -> Result<()>
where
    T: Transport,
{
    run_loop(transport, registry)
}

/// Creates a loopback transport pair for host tests.
#[cfg(nexus_env = "host")]
pub fn loopback_transport() -> (nexus_ipc::LoopbackClient, IpcTransport<nexus_ipc::LoopbackServer>)
{
    let (client, server) = nexus_ipc::loopback_channel();
    (client, IpcTransport::new(server))
}

fn run_loop<T>(transport: &mut T, registry: BundleRegistry) -> Result<()>
where
    T: Transport,
{
    let mut state = ServiceState::new(registry);
    println!("packagefsd: ready");
    while let Some(frame) = transport.recv().map_err(|err| ServerError::Transport(err.into()))? {
        if frame.is_empty() {
            continue;
        }
        if let Err(err) = handle_frame(&mut state, transport, &frame) {
            error!("packagefsd: handle error: {err}");
        }
    }
    Ok(())
}

fn handle_frame<T>(state: &mut ServiceState, transport: &mut T, frame: &[u8]) -> Result<()>
where
    T: Transport,
{
    let (opcode, payload) =
        frame.split_first().ok_or_else(|| ServerError::Decode("empty frame".into()))?;
    let opcode = *opcode;
    let response = match opcode {
        OPCODE_PUBLISH => handle_publish(state, payload)?,
        OPCODE_RESOLVE => handle_resolve(state, payload)?,
        other => {
            error!("packagefsd: unknown opcode {other}");
            return Ok(());
        }
    };
    transport.send(&response).map_err(|err| ServerError::Transport(err.into()))
}

fn handle_publish(state: &mut ServiceState, payload: &[u8]) -> Result<Vec<u8>> {
    let mut cursor = std::io::Cursor::new(payload);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|err| ServerError::Decode(format!("publish read: {err}")))?;
    let request = message
        .get_root::<publish_bundle::Reader<'_>>()
        .map_err(|err| ServerError::Decode(format!("publish root: {err}")))?;
    let name = request
        .get_name()
        .map_err(|err| ServerError::Decode(format!("publish name: {err}")))?
        .to_str()
        .map_err(|err| ServerError::Decode(format!("publish name utf8: {err}")))?
        .to_string();
    let version = request
        .get_version()
        .map_err(|err| ServerError::Decode(format!("publish version: {err}")))?
        .to_str()
        .map_err(|err| ServerError::Decode(format!("publish version utf8: {err}")))?
        .to_string();
    info!("packagefsd: publish {name}@{version} root={}", request.get_root_vmo());
    let entries_reader = request
        .get_entries()
        .map_err(|err| ServerError::Decode(format!("publish entries: {err}")))?;
    let mut entries = Vec::new();
    for entry in entries_reader.iter() {
        let path = entry
            .get_path()
            .map_err(|err| ServerError::Decode(format!("publish entry path: {err}")))?
            .to_str()
            .map_err(|err| ServerError::Decode(format!("publish entry path utf8: {err}")))?
            .to_string();
        let kind = entry.get_kind();
        if kind != KIND_FILE && kind != KIND_DIRECTORY {
            return Err(ServerError::Decode(format!("publish entry kind invalid: {kind}")));
        }
        let bytes = entry
            .get_bytes()
            .map_err(|err| ServerError::Decode(format!("publish entry bytes: {err}")))?
            .to_vec();
        entries.push(FileEntry::new(&path, kind, bytes));
    }
    match state.registry.publish_bundle(&name, &version, entries) {
        Ok(()) => encode_publish_response(true),
        Err(ServiceError::InvalidPath) => encode_publish_response(false),
        Err(err) => Err(ServerError::Service(err)),
    }
}

fn handle_resolve(state: &mut ServiceState, payload: &[u8]) -> Result<Vec<u8>> {
    let mut cursor = std::io::Cursor::new(payload);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|err| ServerError::Decode(format!("resolve read: {err}")))?;
    let request = message
        .get_root::<resolve_path::Reader<'_>>()
        .map_err(|err| ServerError::Decode(format!("resolve root: {err}")))?;
    let rel = request
        .get_rel()
        .map_err(|err| ServerError::Decode(format!("resolve rel: {err}")))?
        .to_str()
        .map_err(|err| ServerError::Decode(format!("resolve rel utf8: {err}")))?
        .to_string();
    match state.registry.resolve(&rel) {
        Ok(entry) => encode_resolve_response(true, entry.size, entry.kind, &entry.bytes),
        Err(ServiceError::NotFound) => encode_resolve_response(false, 0, 0, &[]),
        Err(err) => Err(ServerError::Service(err)),
    }
}

fn encode_publish_response(ok: bool) -> Result<Vec<u8>> {
    let mut message = capnp::message::Builder::new_default();
    {
        let mut response = message.init_root::<publish_response::Builder<'_>>();
        response.set_ok(ok);
    }
    serialize_response(OPCODE_PUBLISH, message)
}

fn encode_resolve_response(ok: bool, size: u64, kind: u16, bytes: &[u8]) -> Result<Vec<u8>> {
    let mut message = capnp::message::Builder::new_default();
    {
        let mut response = message.init_root::<resolve_response::Builder<'_>>();
        response.set_ok(ok);
        response.set_size(size);
        response.set_kind(kind);
        if ok {
            response.set_bytes(bytes);
        } else {
            response.reborrow().init_bytes(0);
        }
    }
    serialize_response(OPCODE_RESOLVE, message)
}

fn serialize_response(
    opcode: u8,
    message: capnp::message::Builder<capnp::message::HeapAllocator>,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    serialize::write_message(&mut bytes, &message).map_err(ServerError::Encode)?;
    let mut frame = Vec::with_capacity(1 + bytes.len());
    frame.push(opcode);
    frame.extend(bytes);
    Ok(frame)
}

fn sanitize_entry_path(path: &str) -> core::result::Result<String, ServiceError> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return Err(ServiceError::InvalidPath);
    }
    let mut segments = Vec::new();
    for segment in trimmed.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(ServiceError::InvalidPath);
        }
        segments.push(segment);
    }
    Ok(segments.join("/"))
}

/// Ensures Cap'n Proto schemas are referenced.
pub fn touch_schemas() {
    let _ = core::any::type_name::<publish_bundle::Reader<'static>>();
    let _ = core::any::type_name::<resolve_path::Reader<'static>>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use storage::pkgimg::{build_pkgimg, PkgImgFileSpec};

    struct DummyTransport {
        frames: Vec<Vec<u8>>,
        sent: Vec<Vec<u8>>,
    }

    impl DummyTransport {
        fn new(frame: Vec<u8>) -> Self {
            Self { frames: vec![frame], sent: Vec::new() }
        }
    }

    impl Transport for DummyTransport {
        type Error = TransportError;

        fn recv(&mut self) -> core::result::Result<Option<Vec<u8>>, Self::Error> {
            Ok(self.frames.pop())
        }

        fn send(&mut self, frame: &[u8]) -> core::result::Result<(), Self::Error> {
            self.sent.push(frame.to_vec());
            Ok(())
        }
    }

    #[test]
    fn publish_registers_bundle() {
        let registry = BundleRegistry::default();
        let mut state = ServiceState::new(registry.clone());
        let mut message = capnp::message::Builder::new_default();
        {
            let mut request = message.init_root::<publish_bundle::Builder<'_>>();
            request.set_name("demo");
            request.set_version("1.0.0");
            request.set_root_vmo(7);
            let mut entries = request.reborrow().init_entries(1);
            let mut entry = entries.reborrow().get(0);
            entry.set_path("manifest.nxb");
            entry.set_kind(KIND_FILE);
            entry.set_bytes(b"hello");
        }
        let mut bytes = Vec::new();
        serialize::write_message(&mut bytes, &message).unwrap();
        let mut frame = Vec::with_capacity(1 + bytes.len());
        frame.push(OPCODE_PUBLISH);
        frame.extend(bytes);
        let mut transport = DummyTransport::new(frame.clone());
        handle_frame(&mut state, &mut transport, &frame).unwrap();
        assert!(transport.sent.len() == 1);
        let entry = registry.resolve("demo@1.0.0/manifest.nxb").unwrap();
        assert_eq!(entry.size, 5);
    }

    #[test]
    fn mount_pkgimg_registers_bundle_entries_for_resolve() {
        let registry = BundleRegistry::default();
        let specs = vec![
            PkgImgFileSpec::new("system", "1.0.0", "build.prop", b"ro.nexus.build=dev\n"),
            PkgImgFileSpec::new("demo.hello", "1.0.0", "manifest.nxb", b"manifest"),
        ];
        let img = build_pkgimg(&specs, PkgImgCaps::default()).unwrap();
        mount_pkgimg_bytes(&registry, &img).unwrap();
        let system = registry.resolve("system/build.prop").unwrap();
        assert_eq!(system.bytes, b"ro.nexus.build=dev\n");
        let manifest = registry.resolve("demo.hello/manifest.nxb").unwrap();
        assert_eq!(manifest.bytes, b"manifest");
    }

    #[test]
    fn test_reject_pkgimg_bad_magic_or_version_mount() {
        let registry = BundleRegistry::default();
        let specs = vec![PkgImgFileSpec::new("system", "1.0.0", "build.prop", b"ok")];
        let mut img = build_pkgimg(&specs, PkgImgCaps::default()).unwrap();
        img[8..10].copy_from_slice(&3u16.to_le_bytes());
        let err = mount_pkgimg_bytes(&registry, &img).unwrap_err();
        assert!(matches!(err, ServerError::PkgImg(_)));
    }
}
