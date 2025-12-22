// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]
//! CONTEXT: BundleMgr daemon – bundle install/query/payload via Cap'n Proto IPC
//! OWNERS: @services-team
//! PUBLIC API: service_main_loop(), run_with_transport(), loopback_transport()
//! DEPENDS_ON: nexus_ipc, nexus_idl_runtime (capnp), keystored client, packagefs client
//! INVARIANTS: Separate from SAMgr/Keystore roles; stable readiness prints
//! ADR: docs/adr/0017-service-architecture.md

use std::collections::HashMap;
use std::fmt;
use std::io::Cursor;
use std::sync::{Arc, Mutex, OnceLock};

#[cfg(feature = "idl-capnp")]
use bundlemgr::{
    service::InstallRequest as DomainInstallRequest, Error as ManifestError, Manifest, Service,
    ServiceError,
};
use nexus_ipc::{self, Wait};

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
    get_payload_request, get_payload_response, install_request, install_response, query_request,
    query_response, InstallError,
};
#[cfg(feature = "idl-capnp")]
use nexus_idl_runtime::keystored_capnp::{verify_request, verify_response};
#[cfg(feature = "idl-capnp")]
use nexus_packagefs::{
    BundleEntry as PackageFsEntry, PackageFsClient, PublishRequest as PackageFsPublish,
};

const OPCODE_INSTALL: u8 = 1;
const OPCODE_QUERY: u8 = 2;
const OPCODE_GET_PAYLOAD: u8 = 3;
#[cfg(feature = "idl-capnp")]
const KEYSTORE_OPCODE_VERIFY: u8 = 2;

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
            nexus_ipc::IpcError::NoSpace => Self::Other("ipc ran out of resources".to_string()),
            nexus_ipc::IpcError::Kernel(inner) => {
                Self::Other(format!("kernel ipc error: {inner:?}"))
            }
            _ => Self::Other(format!("ipc error: {err:?}")),
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
    staged_payloads: Arc<Mutex<HashMap<u32, Vec<u8>>>>,
    payloads: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    staged_assets: Arc<Mutex<HashMap<u32, Vec<StagedAsset>>>>,
}

static GLOBAL_ARTIFACTS: OnceLock<Mutex<Option<ArtifactStore>>> = OnceLock::new();

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

    /// Stages payload bytes under `handle` until installation completes.
    pub fn stage_payload(&self, handle: u32, bytes: Vec<u8>) {
        match self.staged_payloads.lock() {
            Ok(mut guard) => {
                guard.insert(handle, bytes);
            }
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.insert(handle, bytes);
            }
        }
    }

    /// Takes staged payload bytes associated with `handle`, if present.
    pub fn take_staged_payload(&self, handle: u32) -> Option<Vec<u8>> {
        match self.staged_payloads.lock() {
            Ok(mut guard) => guard.remove(&handle),
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.remove(&handle)
            }
        }
    }

    /// Stores payload bytes for the bundle named `name` after installation succeeds.
    pub fn install_payload(&self, name: &str, bytes: Vec<u8>) {
        match self.payloads.lock() {
            Ok(mut guard) => {
                guard.insert(name.to_string(), bytes);
            }
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.insert(name.to_string(), bytes);
            }
        }
    }

    /// Returns a clone of the payload bytes for `name` if available.
    pub fn payload(&self, name: &str) -> Option<Vec<u8>> {
        match self.payloads.lock() {
            Ok(guard) => guard.get(name).cloned(),
            Err(poisoned) => {
                let guard = poisoned.into_inner();
                guard.get(name).cloned()
            }
        }
    }

    /// Stages an asset file to be published alongside the bundle payload.
    pub fn stage_asset(&self, handle: u32, path: &str, bytes: Vec<u8>) {
        let asset = StagedAsset {
            path: path.to_string(),
            bytes,
        };
        match self.staged_assets.lock() {
            Ok(mut guard) => guard.entry(handle).or_default().push(asset),
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.entry(handle).or_default().push(asset);
            }
        }
    }

    /// Takes staged assets associated with `handle` if any were recorded.
    pub fn take_staged_assets(&self, handle: u32) -> Vec<StagedAsset> {
        match self.staged_assets.lock() {
            Ok(mut guard) => guard.remove(&handle).unwrap_or_default(),
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.remove(&handle).unwrap_or_default()
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct StagedAsset {
    pub path: String,
    pub bytes: Vec<u8>,
}

fn global_artifacts() -> &'static Mutex<Option<ArtifactStore>> {
    GLOBAL_ARTIFACTS.get_or_init(|| Mutex::new(None))
}

/// Registers the provided artifact store as the globally accessible instance.
pub fn register_artifact_store(store: &ArtifactStore) {
    if let Ok(mut slot) = global_artifacts().lock() {
        *slot = Some(store.clone());
    }
}

/// Returns a clone of the globally registered artifact store if available.
pub fn artifact_store() -> Option<ArtifactStore> {
    global_artifacts()
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().cloned())
}

#[cfg(feature = "idl-capnp")]
pub(crate) trait KeystoreClient: Send + 'static {
    fn verify(
        &self,
        anchor_id: &str,
        payload: &[u8],
        signature: &[u8],
    ) -> Result<bool, KeystoreClientError>;
}

#[cfg(feature = "idl-capnp")]
struct KeystoreIpc<C> {
    client: C,
}

#[cfg(feature = "idl-capnp")]
impl<C> KeystoreIpc<C> {
    fn new(client: C) -> Self {
        Self { client }
    }
}

#[cfg(feature = "idl-capnp")]
impl<C> KeystoreClient for KeystoreIpc<C>
where
    C: nexus_ipc::Client + Send + 'static,
{
    fn verify(
        &self,
        anchor_id: &str,
        payload: &[u8],
        signature: &[u8],
    ) -> Result<bool, KeystoreClientError> {
        let mut message = Builder::new_default();
        {
            let mut request = message.init_root::<verify_request::Builder<'_>>();
            request.set_anchor_id(anchor_id);
            request.set_payload(payload);
            request.set_signature(signature);
        }
        let mut buf = Vec::new();
        serialize::write_message(&mut buf, &message).map_err(KeystoreClientError::Encode)?;
        let mut frame = Vec::with_capacity(1 + buf.len());
        frame.push(KEYSTORE_OPCODE_VERIFY);
        frame.extend_from_slice(&buf);
        self.client
            .send(&frame, Wait::Blocking)
            .map_err(|err| KeystoreClientError::Transport(err.into()))?;
        let response = self
            .client
            .recv(Wait::Blocking)
            .map_err(|err| KeystoreClientError::Transport(err.into()))?;
        let (opcode, payload) = response
            .split_first()
            .ok_or_else(|| KeystoreClientError::Protocol("empty frame".into()))?;
        if *opcode != KEYSTORE_OPCODE_VERIFY {
            return Err(KeystoreClientError::Protocol(format!(
                "unexpected opcode {opcode}"
            )));
        }
        let mut cursor = Cursor::new(payload);
        let message = serialize::read_message(&mut cursor, ReaderOptions::new())
            .map_err(|err| KeystoreClientError::Decode(err.to_string()))?;
        let response = message
            .get_root::<verify_response::Reader<'_>>()
            .map_err(|err| KeystoreClientError::Decode(err.to_string()))?;
        Ok(response.get_ok())
    }
}

#[cfg(feature = "idl-capnp")]
#[derive(Debug)]
enum KeystoreClientError {
    Transport(TransportError),
    Encode(capnp::Error),
    Decode(String),
    Protocol(String),
}

#[cfg(feature = "idl-capnp")]
impl fmt::Display for KeystoreClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(err) => write!(f, "transport error: {err}"),
            Self::Encode(err) => write!(f, "encode error: {err}"),
            Self::Decode(msg) => write!(f, "decode error: {msg}"),
            Self::Protocol(msg) => write!(f, "protocol error: {msg}"),
        }
    }
}

#[cfg(feature = "idl-capnp")]
impl std::error::Error for KeystoreClientError {}

#[cfg(feature = "idl-capnp")]
pub struct KeystoreHandle(Box<dyn KeystoreClient>);

#[cfg(feature = "idl-capnp")]
impl KeystoreHandle {
    #[cfg(nexus_env = "host")]
    pub fn from_loopback(client: nexus_ipc::LoopbackClient) -> Self {
        Self(Box::new(KeystoreIpc::new(client)))
    }

    #[cfg(nexus_env = "os")]
    pub fn from_kernel(client: nexus_ipc::KernelClient) -> Self {
        Self(Box::new(KeystoreIpc::new(client)))
    }

    fn into_client(self) -> Box<dyn KeystoreClient> {
        self.0
    }
}

#[cfg(feature = "idl-capnp")]
pub struct PackageFsHandle(Arc<PackageFsClient>);

#[cfg(feature = "idl-capnp")]
impl PackageFsHandle {
    #[cfg(nexus_env = "host")]
    pub fn from_loopback(client: nexus_ipc::LoopbackClient) -> Self {
        Self(Arc::new(PackageFsClient::from_loopback(client)))
    }

    #[cfg(nexus_env = "os")]
    pub fn from_kernel() -> Result<Self, nexus_packagefs::Error> {
        PackageFsClient::new().map(Arc::new).map(Self)
    }

    pub fn from_client(client: Arc<PackageFsClient>) -> Self {
        Self(client)
    }

    fn into_client(self) -> Arc<PackageFsClient> {
        self.0
    }
}

struct Server {
    service: Service,
    artifacts: ArtifactStore,
    #[cfg(feature = "idl-capnp")]
    keystore: Option<Box<dyn KeystoreClient>>,
    #[cfg(feature = "idl-capnp")]
    packagefs: Option<Arc<PackageFsClient>>,
}

impl Server {
    fn new(
        service: Service,
        artifacts: ArtifactStore,
        keystore: Option<Box<dyn KeystoreClient>>,
        packagefs: Option<Arc<PackageFsClient>>,
    ) -> Self {
        Self {
            service,
            artifacts,
            keystore,
            packagefs,
        }
    }

    #[cfg(feature = "idl-capnp")]
    fn handle_frame(&mut self, opcode: u8, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        match opcode {
            OPCODE_INSTALL => self.handle_install(payload),
            OPCODE_QUERY => self.handle_query(payload),
            OPCODE_GET_PAYLOAD => self.handle_get_payload(payload),
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

        let manifest_bytes = match self.artifacts.take(handle) {
            Some(bytes) => bytes,
            None => {
                builder.set_ok(false);
                builder.set_err(InstallError::Enoent);
                return Self::encode_response(OPCODE_INSTALL, &response);
            }
        };

        let payload_bytes = match self.artifacts.take_staged_payload(handle) {
            Some(bytes) => bytes,
            None => {
                builder.set_ok(false);
                builder.set_err(InstallError::Enoent);
                // Put manifest bytes back so the caller can retry after staging payload.
                self.artifacts.insert(handle, manifest_bytes);
                return Self::encode_response(OPCODE_INSTALL, &response);
            }
        };

        if expected_len != 0 && manifest_bytes.len() != expected_len {
            builder.set_ok(false);
            builder.set_err(InstallError::Einval);
            self.artifacts.insert(handle, manifest_bytes.clone());
            self.artifacts.stage_payload(handle, payload_bytes);
            return Self::encode_response(OPCODE_INSTALL, &response);
        }

        let manifest_str = match std::str::from_utf8(&manifest_bytes) {
            Ok(value) => value,
            Err(_) => {
                builder.set_ok(false);
                builder.set_err(InstallError::Einval);
                self.artifacts.insert(handle, manifest_bytes.clone());
                self.artifacts.stage_payload(handle, payload_bytes);
                return Self::encode_response(OPCODE_INSTALL, &response);
            }
        };

        let manifest = match Manifest::parse_str(manifest_str) {
            Ok(manifest) => manifest,
            Err(err) => {
                builder.set_ok(false);
                builder.set_err(map_manifest_error(&err));
                self.artifacts.insert(handle, manifest_bytes.clone());
                self.artifacts.stage_payload(handle, payload_bytes);
                return Self::encode_response(OPCODE_INSTALL, &response);
            }
        };

        // Use manifest payload with the signature line stripped for verification
        let abilities_list = manifest
            .abilities
            .iter()
            .map(|s| format!("\"{}\"", s))
            .collect::<Vec<_>>()
            .join(", ");
        let caps_list = manifest
            .capabilities
            .iter()
            .map(|s| format!("\"{}\"", s))
            .collect::<Vec<_>>()
            .join(", ");
        let canonical = format!(
            "name = \"{}\"\nversion = \"{}\"\nabilities = [{}]\ncaps = [{}]\nmin_sdk = \"{}\"\npublisher = \"{}\"\n",
            name,
            manifest.version,
            abilities_list,
            caps_list,
            manifest.min_sdk,
            manifest.publisher,
        );
        let signed_bytes = canonical.as_bytes();

        eprintln!(
            "bundlemgrd: verify begin publisher={} payload_len={} sig_len={}",
            manifest.publisher,
            signed_bytes.len(),
            manifest.signature.len()
        );
        if self.keystore.is_none() {
            eprintln!(
                "bundlemgrd: keystore unavailable; skipping signature verification for {name}"
            );
        } else {
            match self.verify_bundle_signature(
                &manifest.publisher,
                signed_bytes,
                &manifest.signature,
            ) {
                Ok(true) => {}
                Ok(false) => {
                    // Debug: show a short digest of payload and signature for mismatch analysis
                    let payload_digest = {
                        use sha2::{Digest, Sha256};
                        let mut hasher = Sha256::new();
                        hasher.update(signed_bytes);
                        let out = hasher.finalize();
                        hex::encode(&out[..8])
                    };
                    eprintln!(
                    "bundlemgrd: verify mismatch publisher={} payload_sha256_64={} first_bytes={:02x?}",
                    manifest.publisher,
                    payload_digest,
                    &signed_bytes.get(0..std::cmp::min(4, signed_bytes.len())).unwrap_or(&[])
                );
                    // Fallback to signing payload derived from raw manifest bytes (strip sig line)
                    let alt_signed = signing_payload_from_manifest_bytes(manifest_str.as_bytes());
                    eprintln!(
                        "bundlemgrd: verify fallback publisher={} payload_len={} sig_len={}",
                        manifest.publisher,
                        alt_signed.len(),
                        manifest.signature.len()
                    );
                    match self.verify_bundle_signature(
                        &manifest.publisher,
                        alt_signed,
                        &manifest.signature,
                    ) {
                        Ok(true) => {}
                        _ => {
                            eprintln!("bundlemgrd: invalid signature for {name}");
                            builder.set_ok(false);
                            builder.set_err(InstallError::InvalidSig);
                            return Self::encode_response(OPCODE_INSTALL, &response);
                        }
                    }
                }
                Err(err) => {
                    eprintln!("bundlemgrd: signature verify error: {err}");
                    builder.set_ok(false);
                    builder.set_err(InstallError::InvalidSig);
                    return Self::encode_response(OPCODE_INSTALL, &response);
                }
            }
        }

        let assets = self.artifacts.take_staged_assets(handle);

        match self.service.install(DomainInstallRequest {
            name: &name,
            manifest: manifest_str,
        }) {
            Ok(bundle) => {
                self.publish_package_to_fs(&bundle, &manifest_bytes, &payload_bytes, &assets);
                self.artifacts.install_payload(&name, payload_bytes);
                builder.set_ok(true);
                builder.set_err(InstallError::None);
            }
            Err(err) => {
                self.artifacts.insert(handle, manifest_bytes.clone());
                self.artifacts.stage_payload(handle, payload_bytes);
                self.restage_assets(handle, &assets);
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
                    let mut caps = builder
                        .reborrow()
                        .init_required_caps(bundle.capabilities.len() as u32);
                    for (idx, cap) in bundle.capabilities.iter().enumerate() {
                        caps.set(idx as u32, cap);
                    }
                }
                None => {
                    builder.set_installed(false);
                    builder.set_version("");
                    builder.reborrow().init_required_caps(0);
                }
            }
        }
        Self::encode_response(OPCODE_QUERY, &response)
    }

    #[cfg(feature = "idl-capnp")]
    fn handle_get_payload(&mut self, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        let name = Self::decode_get_payload_request(payload)?;
        match self.service.query(&name).map_err(ServerError::from)? {
            Some(_) => match self.artifacts.payload(&name) {
                Some(bytes) => {
                    println!("bundlemgrd: get_payload {name} len={}", bytes.len());
                    Self::encode_get_payload_response(true, &bytes)
                }
                None => {
                    println!("bundlemgrd: get_payload enoent {name}");
                    Self::encode_get_payload_response(false, &[])
                }
            },
            None => {
                println!("bundlemgrd: get_payload enoent {name}");
                Self::encode_get_payload_response(false, &[])
            }
        }
    }

    #[cfg(feature = "idl-capnp")]
    fn decode_get_payload_request(payload: &[u8]) -> Result<String, ServerError> {
        let mut cursor = Cursor::new(payload);
        let message = serialize::read_message(&mut cursor, ReaderOptions::new())
            .map_err(|err| ServerError::Decode(format!("get_payload read: {err}")))?;
        let request = message
            .get_root::<get_payload_request::Reader<'_>>()
            .map_err(|err| ServerError::Decode(format!("get_payload root: {err}")))?;
        let name = request
            .get_name()
            .map_err(|err| ServerError::Decode(format!("get_payload name: {err}")))?;
        let utf8 = name
            .to_str()
            .map_err(|err| ServerError::Decode(format!("get_payload name utf8: {err}")))?;
        Ok(utf8.to_string())
    }

    #[cfg(feature = "idl-capnp")]
    fn encode_get_payload_response(ok: bool, bytes: &[u8]) -> Result<Vec<u8>, ServerError> {
        let mut response = Builder::new_default();
        {
            let mut builder = response.init_root::<get_payload_response::Builder<'_>>();
            builder.set_ok(ok);
            if ok {
                builder.set_bytes(bytes);
            } else {
                builder.reborrow().init_bytes(0);
            }
        }
        Self::encode_response(OPCODE_GET_PAYLOAD, &response)
    }

    #[cfg(feature = "idl-capnp")]
    fn verify_bundle_signature(
        &self,
        publisher: &str,
        payload: &[u8],
        signature: &[u8],
    ) -> Result<bool, KeystoreClientError> {
        match &self.keystore {
            Some(client) => client.verify(publisher, payload, signature),
            None => Err(KeystoreClientError::Protocol("keystore unavailable".into())),
        }
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

impl Server {
    fn restage_assets(&self, handle: u32, assets: &[StagedAsset]) {
        for asset in assets {
            self.artifacts
                .stage_asset(handle, &asset.path, asset.bytes.clone());
        }
    }

    fn publish_package_to_fs(
        &self,
        bundle: &bundlemgr::service::InstalledBundle,
        manifest_bytes: &[u8],
        payload_bytes: &[u8],
        assets: &[StagedAsset],
    ) {
        let Some(client) = &self.packagefs else {
            return;
        };
        let version = bundle.version.to_string();
        let mut entries = Vec::with_capacity(2 + assets.len());
        entries.push(PackageFsEntry::new("manifest.toml", 0, manifest_bytes));
        entries.push(PackageFsEntry::new("payload.elf", 0, payload_bytes));
        for asset in assets {
            entries.push(PackageFsEntry::new(&asset.path, 0, &asset.bytes));
        }
        let request = PackageFsPublish {
            name: &bundle.name,
            version: &version,
            root_vmo: 0,
            entries: &entries,
        };
        if let Err(err) = client.publish_bundle(request) {
            eprintln!(
                "bundlemgrd: packagefs publish {}@{} failed: {err}",
                bundle.name, version
            );
        } else {
            println!(
                "bundlemgrd: published {}@{} to packagefs",
                bundle.name, version
            );
        }
    }
}

/// Returns the input TOML string without a trailing `sig = "..."` line if present.
#[allow(dead_code)]
fn strip_sig_line(manifest: &str) -> String {
    let mut out = String::with_capacity(manifest.len());
    for line in manifest.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("sig = ") {
            // Skip the signature line for signing payload
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Builds signing payload bytes by removing any trailing `sig = "..."` line.
fn signing_payload_from_manifest_bytes(bytes: &[u8]) -> &[u8] {
    let needle = b"\nsig = \"";
    if let Some(pos) = memchr::memmem::find(bytes, needle) {
        &bytes[..pos + 1] // keep the trailing newline before sig
    } else {
        bytes
    }
}

/// Runs the server with the provided transport and artifact store.
#[cfg(feature = "idl-capnp")]
pub fn run_with_transport<T: Transport>(
    transport: &mut T,
    artifacts: ArtifactStore,
    keystore: Option<KeystoreHandle>,
    packagefs: Option<PackageFsHandle>,
) -> Result<(), ServerError> {
    let service = Service::new();
    let client = keystore.map(KeystoreHandle::into_client);
    let packagefs_client = packagefs.map(PackageFsHandle::into_client);
    register_artifact_store(&artifacts);
    serve_with_components(transport, service, artifacts, client, packagefs_client)
}

/// Serves requests using injected service and artifact store.
#[cfg(feature = "idl-capnp")]
pub(crate) fn serve_with_components<T: Transport>(
    transport: &mut T,
    service: Service,
    artifacts: ArtifactStore,
    keystore: Option<Box<dyn KeystoreClient>>,
    packagefs: Option<Arc<PackageFsClient>>,
) -> Result<(), ServerError> {
    let mut server = Server::new(service, artifacts, keystore, packagefs);
    while let Some(frame) = transport
        .recv()
        .map_err(|err| ServerError::Transport(err.into()))?
    {
        if frame.is_empty() {
            continue;
        }
        let (opcode, payload) = frame
            .split_first()
            .ok_or_else(|| ServerError::Decode("empty frame".into()))?;
        let response = server.handle_frame(*opcode, payload)?;
        transport
            .send(&response)
            .map_err(|err| ServerError::Transport(err.into()))?;
    }
    Ok(())
}

#[cfg(feature = "idl-capnp")]
fn map_manifest_error(error: &ManifestError) -> InstallError {
    match error {
        ManifestError::Toml(_) | ManifestError::InvalidRoot => InstallError::Einval,
        ManifestError::MissingField(field) => match *field {
            "publisher" | "sig" => InstallError::Unsigned,
            _ => InstallError::Einval,
        },
        ManifestError::InvalidField { field, .. } => match *field {
            "sig" => InstallError::InvalidSig,
            "publisher" => InstallError::Einval,
            _ => InstallError::Einval,
        },
    }
}

#[cfg(feature = "idl-capnp")]
fn map_install_error(error: &ServiceError) -> InstallError {
    match error {
        ServiceError::AlreadyInstalled => InstallError::Ebusy,
        ServiceError::InvalidSignature => InstallError::InvalidSig,
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
        let (client_bundle, server_bundle) = nexus_ipc::loopback_channel();
        let (client_keystore, server_keystore) = nexus_ipc::loopback_channel();

        // Spawn keystored on host loopback and keep client handle alive
        // Keep bundle client alive for the loopback server lifetime
        let _bundle_guard = client_bundle;
        std::thread::spawn(move || {
            let mut ks_transport = keystored::IpcTransport::new(server_keystore);
            let _ = keystored::run_with_transport_default_anchors(&mut ks_transport);
        });

        let mut transport = IpcTransport::new(server_bundle);
        notifier.notify();
        println!("bundlemgrd: ready");
        let keystore = Some(KeystoreHandle::from_loopback(client_keystore));
        run_with_transport(&mut transport, artifacts, keystore, None)
    }

    #[cfg(nexus_env = "os")]
    {
        nexus_ipc::set_default_target("bundlemgrd");
        let server = nexus_ipc::KernelServer::new()
            .map_err(|err| ServerError::Transport(TransportError::from(err)))?;
        let mut transport = IpcTransport::new(server);
        notifier.notify();
        println!("bundlemgrd: ready");
        // TODO: Wire kernel keystore client once IPC is available
        #[cfg(feature = "idl-capnp")]
        let packagefs = match PackageFsHandle::from_kernel() {
            Ok(handle) => Some(handle),
            Err(err) => {
                eprintln!("bundlemgrd: packagefs client init failed: {err:?}");
                None
            }
        };

        #[cfg(not(feature = "idl-capnp"))]
        let packagefs = None;

        run_with_transport(&mut transport, artifacts, None, packagefs)
    }
}

/// Creates a loopback transport pair for host-side tests.
#[cfg(nexus_env = "host")]
pub fn loopback_transport() -> (
    nexus_ipc::LoopbackClient,
    IpcTransport<nexus_ipc::LoopbackServer>,
) {
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
        let _ = core::any::type_name::<get_payload_request::Reader<'static>>();
        let _ = core::any::type_name::<get_payload_response::Reader<'static>>();
    }
}
