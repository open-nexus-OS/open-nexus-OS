// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]
//! CONTEXT: BundleMgr daemon – bundle install/query/payload via Cap'n Proto IPC
//! OWNERS: @services-team
//! PUBLIC API: service_main_loop(), run_with_transport(), loopback_transport()
//! DEPENDS_ON: nexus_ipc, nexus_idl_runtime (capnp), keystored client, packagefs client
//! INVARIANTS: Separate from SAMgr/Keystore roles; stable readiness prints
//! ADR: docs/adr/0017-service-architecture.md
//! TEST_COVERAGE: 11 unit tests + shared E2E coverage
//!   - Unit: supply_chain_install_ok + 10 reject/tamper/oversize checks in this module
//!   - E2E: tests/e2e/tests/bundlemgrd_roundtrip.rs

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
use nexus_idl_runtime::keystored_capnp::{
    is_key_allowed_request, is_key_allowed_response, verify_request, verify_response,
};
#[cfg(feature = "idl-capnp")]
use nexus_idl_runtime::manifest_capnp::bundle_manifest;
#[cfg(feature = "idl-capnp")]
use nexus_packagefs::{
    BundleEntry as PackageFsEntry, PackageFsClient, PublishRequest as PackageFsPublish,
};
use sha2::Digest;

const OPCODE_INSTALL: u8 = 1;
const OPCODE_QUERY: u8 = 2;
const OPCODE_GET_PAYLOAD: u8 = 3;
#[cfg(feature = "idl-capnp")]
const KEYSTORE_OPCODE_VERIFY: u8 = 2;
#[cfg(feature = "idl-capnp")]
const KEYSTORE_OPCODE_IS_KEY_ALLOWED: u8 = 4;
const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const MAX_SBOM_BYTES: usize = 512 * 1024;
const MAX_REPRO_BYTES: usize = 64 * 1024;

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
        let asset = StagedAsset { path: path.to_string(), bytes };
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
    global_artifacts().lock().ok().and_then(|slot| slot.as_ref().cloned())
}

#[cfg(feature = "idl-capnp")]
pub(crate) trait KeystoreClient: Send + 'static {
    fn verify(
        &self,
        anchor_id: &str,
        payload: &[u8],
        signature: &[u8],
    ) -> Result<bool, KeystoreClientError>;

    fn is_key_allowed(
        &self,
        publisher: &str,
        alg: &str,
        pubkey: &[u8],
    ) -> Result<KeyAllowDecision, KeystoreClientError>;
}

#[cfg(feature = "idl-capnp")]
#[derive(Debug, Clone)]
pub(crate) struct KeyAllowDecision {
    pub allowed: bool,
    pub reason: String,
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
            return Err(KeystoreClientError::Protocol(format!("unexpected opcode {opcode}")));
        }
        let mut cursor = Cursor::new(payload);
        let message = serialize::read_message(&mut cursor, ReaderOptions::new())
            .map_err(|err| KeystoreClientError::Decode(err.to_string()))?;
        let response = message
            .get_root::<verify_response::Reader<'_>>()
            .map_err(|err| KeystoreClientError::Decode(err.to_string()))?;
        Ok(response.get_ok())
    }

    fn is_key_allowed(
        &self,
        publisher: &str,
        alg: &str,
        pubkey: &[u8],
    ) -> Result<KeyAllowDecision, KeystoreClientError> {
        let mut message = Builder::new_default();
        {
            let mut request = message.init_root::<is_key_allowed_request::Builder<'_>>();
            request.set_publisher(publisher);
            request.set_alg(alg);
            request.set_pubkey(pubkey);
        }
        let mut buf = Vec::new();
        serialize::write_message(&mut buf, &message).map_err(KeystoreClientError::Encode)?;
        let mut frame = Vec::with_capacity(1 + buf.len());
        frame.push(KEYSTORE_OPCODE_IS_KEY_ALLOWED);
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
        if *opcode != KEYSTORE_OPCODE_IS_KEY_ALLOWED {
            return Err(KeystoreClientError::Protocol(format!("unexpected opcode {opcode}")));
        }
        let mut cursor = Cursor::new(payload);
        let message = serialize::read_message(&mut cursor, ReaderOptions::new())
            .map_err(|err| KeystoreClientError::Decode(err.to_string()))?;
        let response = message
            .get_root::<is_key_allowed_response::Reader<'_>>()
            .map_err(|err| KeystoreClientError::Decode(err.to_string()))?;
        let reason = response
            .get_reason()
            .map_err(|err| KeystoreClientError::Decode(err.to_string()))?
            .to_str()
            .map_err(|err| KeystoreClientError::Decode(err.to_string()))?
            .to_string();
        Ok(KeyAllowDecision { allowed: response.get_allowed(), reason })
    }
}

#[cfg(feature = "idl-capnp")]
#[derive(Debug)]
pub(crate) enum KeystoreClientError {
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
        Self { service, artifacts, keystore, packagefs }
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

        let assets = self.artifacts.take_staged_assets(handle);

        if manifest_bytes.len() > MAX_MANIFEST_BYTES || payload_bytes.len() > MAX_PAYLOAD_BYTES {
            builder.set_ok(false);
            builder.set_err(InstallError::Einval);
            self.artifacts.insert(handle, manifest_bytes.clone());
            self.artifacts.stage_payload(handle, payload_bytes);
            self.restage_assets(handle, &assets);
            return Self::encode_response(OPCODE_INSTALL, &response);
        }

        if expected_len != 0 && manifest_bytes.len() != expected_len {
            builder.set_ok(false);
            builder.set_err(InstallError::Einval);
            self.artifacts.insert(handle, manifest_bytes.clone());
            self.artifacts.stage_payload(handle, payload_bytes);
            self.restage_assets(handle, &assets);
            return Self::encode_response(OPCODE_INSTALL, &response);
        }

        let manifest = match Manifest::parse_nxb(&manifest_bytes) {
            Ok(manifest) => manifest,
            Err(err) => {
                builder.set_ok(false);
                builder.set_err(map_manifest_error(&err));
                self.artifacts.insert(handle, manifest_bytes.clone());
                self.artifacts.stage_payload(handle, payload_bytes);
                self.restage_assets(handle, &assets);
                return Self::encode_response(OPCODE_INSTALL, &response);
            }
        };

        eprintln!(
            "bundlemgrd: verify begin publisher={} payload_len={} sig_len={}",
            manifest.publisher,
            payload_bytes.len(),
            manifest.signature.len()
        );
        if self.keystore.is_none() {
            eprintln!(
                "bundlemgrd: keystore unavailable; skipping signature verification for {name}"
            );
        } else {
            // v1 contract: signature covers payload bytes (ELF).
            match self.verify_bundle_signature(
                &manifest.publisher,
                &payload_bytes,
                &manifest.signature,
            ) {
                Ok(true) => {}
                Ok(false) => {
                    eprintln!("bundlemgrd: invalid signature for {name}");
                    builder.set_ok(false);
                    builder.set_err(
                        if self.emit_supply_chain_audit("policy.signature_invalid", false) {
                            InstallError::InvalidSig
                        } else {
                            InstallError::Eacces
                        },
                    );
                    self.artifacts.insert(handle, manifest_bytes.clone());
                    self.artifacts.stage_payload(handle, payload_bytes);
                    self.restage_assets(handle, &assets);
                    return Self::encode_response(OPCODE_INSTALL, &response);
                }
                Err(err) => {
                    eprintln!("bundlemgrd: signature verify error: {err}");
                    builder.set_ok(false);
                    builder.set_err(
                        if self.emit_supply_chain_audit("policy.signature_invalid", false) {
                            InstallError::InvalidSig
                        } else {
                            InstallError::Eacces
                        },
                    );
                    self.artifacts.insert(handle, manifest_bytes.clone());
                    self.artifacts.stage_payload(handle, payload_bytes);
                    self.restage_assets(handle, &assets);
                    return Self::encode_response(OPCODE_INSTALL, &response);
                }
            }
        }

        let publisher_pubkey = hex::decode(&manifest.publisher)
            .unwrap_or_else(|_| manifest.publisher.as_bytes().to_vec());
        let policy_decision = policyd::supply_chain::decide_from_authority(
            &manifest.publisher,
            "ed25519",
            &publisher_pubkey,
            |publisher, alg, pubkey| {
                self.is_key_allowed(publisher, alg, pubkey)
                    .map(|decision| (decision.allowed, decision.reason))
            },
        );
        match policy_decision {
            Ok(policyd::supply_chain::SignPolicyDecision::Allow) => {}
            Ok(policyd::supply_chain::SignPolicyDecision::Deny { label }) => {
                builder.set_ok(false);
                let _ = self.emit_supply_chain_audit(label, false);
                builder.set_err(InstallError::Eacces);
                self.artifacts.insert(handle, manifest_bytes.clone());
                self.artifacts.stage_payload(handle, payload_bytes);
                self.restage_assets(handle, &assets);
                return Self::encode_response(OPCODE_INSTALL, &response);
            }
            Err(policyd::supply_chain::SignPolicyError::QueryFailed) => {
                eprintln!("bundlemgrd: allowlist query error: policy backend unavailable");
                builder.set_ok(false);
                let _ = self.emit_supply_chain_audit("policy.query_failed", false);
                builder.set_err(InstallError::Eacces);
                self.artifacts.insert(handle, manifest_bytes.clone());
                self.artifacts.stage_payload(handle, payload_bytes);
                self.restage_assets(handle, &assets);
                return Self::encode_response(OPCODE_INSTALL, &response);
            }
        }
        let payload_sha256 = sha256_hex(&payload_bytes);
        if manifest.payload_digest.as_deref() != Some(payload_sha256.as_str()) {
            builder.set_ok(false);
            builder.set_err(
                if self.emit_supply_chain_audit("integrity.payload_digest_mismatch", false) {
                    InstallError::Einval
                } else {
                    InstallError::Eacces
                },
            );
            self.artifacts.insert(handle, manifest_bytes.clone());
            self.artifacts.stage_payload(handle, payload_bytes);
            self.restage_assets(handle, &assets);
            return Self::encode_response(OPCODE_INSTALL, &response);
        }
        let manifest_binding_sha256 = manifest_binding_sha256(&manifest_bytes)?;

        let generated_sbom = if find_asset_bytes(&assets, "meta/sbom.json").is_none() {
            Some(
                sbom::generate_bundle_sbom_json(&sbom::BundleSbomInput {
                    bundle_name: manifest.name.clone(),
                    bundle_version: manifest.version.to_string(),
                    publisher_hex: manifest.publisher.clone(),
                    payload_sha256: payload_sha256.clone(),
                    payload_size: payload_bytes.len() as u64,
                    manifest_sha256: manifest_binding_sha256.clone(),
                    source_date_epoch: sbom::source_date_epoch_from_env().unwrap_or(0),
                    components: Vec::new(),
                })
                .map_err(|err| ServerError::Decode(format!("sbom generate: {err}")))?,
            )
        } else {
            None
        };
        let sbom_bytes = find_asset_bytes(&assets, "meta/sbom.json")
            .or(generated_sbom.as_deref())
            .ok_or_else(|| ServerError::Decode("missing sbom".to_string()))?;
        if sbom_bytes.len() > MAX_SBOM_BYTES {
            builder.set_ok(false);
            builder.set_err(if self.emit_supply_chain_audit("integrity.sbom_oversize", false) {
                InstallError::Einval
            } else {
                InstallError::Eacces
            });
            self.artifacts.insert(handle, manifest_bytes.clone());
            self.artifacts.stage_payload(handle, payload_bytes);
            self.restage_assets(handle, &assets);
            return Self::encode_response(OPCODE_INSTALL, &response);
        }
        let generated_repro = if find_asset_bytes(&assets, "meta/repro.env.json").is_none() {
            Some(
                repro::capture_bundle_repro_json_with_manifest_digest(
                    &manifest_binding_sha256,
                    &payload_bytes,
                    sbom_bytes,
                )
                .map_err(|err| ServerError::Decode(format!("repro generate: {err}")))?,
            )
        } else {
            None
        };
        let repro_bytes = find_asset_bytes(&assets, "meta/repro.env.json")
            .or(generated_repro.as_deref())
            .ok_or_else(|| ServerError::Decode("missing repro".to_string()))?;
        if repro_bytes.len() > MAX_REPRO_BYTES {
            builder.set_ok(false);
            builder.set_err(if self.emit_supply_chain_audit("integrity.repro_oversize", false) {
                InstallError::Einval
            } else {
                InstallError::Eacces
            });
            self.artifacts.insert(handle, manifest_bytes.clone());
            self.artifacts.stage_payload(handle, payload_bytes);
            self.restage_assets(handle, &assets);
            return Self::encode_response(OPCODE_INSTALL, &response);
        }
        let sbom_sha256 = sha256_hex(sbom_bytes);
        if manifest.sbom_digest.as_deref() != Some(sbom_sha256.as_str()) {
            builder.set_ok(false);
            builder.set_err(
                if self.emit_supply_chain_audit("integrity.sbom_digest_mismatch", false) {
                    InstallError::Einval
                } else {
                    InstallError::Eacces
                },
            );
            self.artifacts.insert(handle, manifest_bytes.clone());
            self.artifacts.stage_payload(handle, payload_bytes);
            self.restage_assets(handle, &assets);
            return Self::encode_response(OPCODE_INSTALL, &response);
        }
        let repro_sha256 = sha256_hex(repro_bytes);
        if manifest.repro_digest.as_deref() != Some(repro_sha256.as_str()) {
            builder.set_ok(false);
            builder.set_err(
                if self.emit_supply_chain_audit("integrity.repro_digest_mismatch", false) {
                    InstallError::Einval
                } else {
                    InstallError::Eacces
                },
            );
            self.artifacts.insert(handle, manifest_bytes.clone());
            self.artifacts.stage_payload(handle, payload_bytes);
            self.restage_assets(handle, &assets);
            return Self::encode_response(OPCODE_INSTALL, &response);
        }

        let repro_expect = repro::ReproVerifyInput {
            payload_sha256,
            manifest_sha256: manifest_binding_sha256,
            sbom_sha256,
        };
        if let Err(err) = repro::verify_repro_json(repro_bytes, &repro_expect) {
            let label = match err {
                repro::ReproError::SchemaInvalid(_) => "pack.repro_schema_invalid",
                repro::ReproError::DigestMismatch { field: "payload_sha256" } => {
                    "integrity.payload_digest_mismatch"
                }
                repro::ReproError::DigestMismatch { field: "sbom_sha256" } => {
                    "integrity.sbom_digest_mismatch"
                }
                repro::ReproError::DigestMismatch { field: "manifest_sha256" } => {
                    "integrity.repro_digest_mismatch"
                }
                _ => "integrity.repro_invalid",
            };
            builder.set_ok(false);
            builder.set_err(if self.emit_supply_chain_audit(label, false) {
                InstallError::Einval
            } else {
                InstallError::Eacces
            });
            self.artifacts.insert(handle, manifest_bytes.clone());
            self.artifacts.stage_payload(handle, payload_bytes);
            self.restage_assets(handle, &assets);
            return Self::encode_response(OPCODE_INSTALL, &response);
        }

        if !self.emit_supply_chain_audit("bundlemgrd.install.allow", true) {
            builder.set_ok(false);
            builder.set_err(InstallError::Eacces);
            self.artifacts.insert(handle, manifest_bytes.clone());
            self.artifacts.stage_payload(handle, payload_bytes);
            self.restage_assets(handle, &assets);
            return Self::encode_response(OPCODE_INSTALL, &response);
        }

        match self.service.install(DomainInstallRequest { name: &name, manifest: &manifest_bytes })
        {
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
                    let mut caps =
                        builder.reborrow().init_required_caps(bundle.capabilities.len() as u32);
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
    fn is_key_allowed(
        &self,
        publisher: &str,
        alg: &str,
        pubkey: &[u8],
    ) -> Result<KeyAllowDecision, KeystoreClientError> {
        match &self.keystore {
            Some(client) => client.is_key_allowed(publisher, alg, pubkey),
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

    #[cfg(feature = "idl-capnp")]
    fn emit_supply_chain_audit(&self, label: &str, allowed: bool) -> bool {
        let _ = allowed;
        if std::env::var("NEXUS_BUNDLEMGRD_AUDIT_FAIL").map(|value| value == "1").unwrap_or(false) {
            return false;
        }
        eprintln!("bundlemgrd: audit {label}");
        true
    }
}

impl Server {
    fn restage_assets(&self, handle: u32, assets: &[StagedAsset]) {
        for asset in assets {
            self.artifacts.stage_asset(handle, &asset.path, asset.bytes.clone());
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
        entries.push(PackageFsEntry::new("manifest.nxb", 0, manifest_bytes));
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
            eprintln!("bundlemgrd: packagefs publish {}@{} failed: {err}", bundle.name, version);
        } else {
            println!("bundlemgrd: published {}@{} to packagefs", bundle.name, version);
        }
    }
}

// NOTE: legacy TOML signing-payload helpers removed as part of manifest.nxb unification.

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

fn find_asset_bytes<'a>(assets: &'a [StagedAsset], path: &str) -> Option<&'a [u8]> {
    assets.iter().find(|asset| asset.path == path).map(|asset| asset.bytes.as_slice())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(feature = "idl-capnp")]
fn manifest_binding_sha256(manifest_bytes: &[u8]) -> Result<String, ServerError> {
    let mut cursor = Cursor::new(manifest_bytes);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|err| ServerError::Decode(format!("manifest binding read: {err}")))?;
    let src = message
        .get_root::<bundle_manifest::Reader<'_>>()
        .map_err(|err| ServerError::Decode(format!("manifest binding root: {err}")))?;

    let mut builder = Builder::new_default();
    {
        let mut dst = builder.init_root::<bundle_manifest::Builder<'_>>();
        dst.set_schema_version(src.get_schema_version());
        dst.set_name(
            src.get_name()
                .map_err(|err| ServerError::Decode(format!("manifest binding name: {err}")))?
                .to_str()
                .map_err(|err| ServerError::Decode(format!("manifest binding name utf8: {err}")))?,
        );
        dst.set_semver(
            src.get_semver()
                .map_err(|err| ServerError::Decode(format!("manifest binding semver: {err}")))?
                .to_str()
                .map_err(|err| {
                    ServerError::Decode(format!("manifest binding semver utf8: {err}"))
                })?,
        );
        dst.set_min_sdk(
            src.get_min_sdk()
                .map_err(|err| ServerError::Decode(format!("manifest binding minSdk: {err}")))?
                .to_str()
                .map_err(|err| {
                    ServerError::Decode(format!("manifest binding minSdk utf8: {err}"))
                })?,
        );
        dst.set_publisher(
            src.get_publisher()
                .map_err(|err| ServerError::Decode(format!("manifest binding publisher: {err}")))?,
        );
        dst.set_signature(
            src.get_signature()
                .map_err(|err| ServerError::Decode(format!("manifest binding signature: {err}")))?,
        );
        dst.set_payload_digest(src.get_payload_digest().map_err(|err| {
            ServerError::Decode(format!("manifest binding payloadDigest: {err}"))
        })?);
        dst.set_payload_size(src.get_payload_size());
        // Critical: exclude self-referential meta digest fields from binding hash.
        dst.set_sbom_digest(&[]);
        dst.set_repro_digest(&[]);

        let src_abilities = src
            .get_abilities()
            .map_err(|err| ServerError::Decode(format!("manifest binding abilities: {err}")))?;
        let mut dst_abilities = dst.reborrow().init_abilities(src_abilities.len());
        for idx in 0..src_abilities.len() {
            dst_abilities.set(
                idx,
                src_abilities
                    .get(idx)
                    .map_err(|err| {
                        ServerError::Decode(format!("manifest binding ability entry {idx}: {err}"))
                    })?
                    .to_str()
                    .map_err(|err| {
                        ServerError::Decode(format!(
                            "manifest binding ability entry {idx} utf8: {err}"
                        ))
                    })?,
            );
        }

        let src_caps = src
            .get_capabilities()
            .map_err(|err| ServerError::Decode(format!("manifest binding capabilities: {err}")))?;
        let mut dst_caps = dst.reborrow().init_capabilities(src_caps.len());
        for idx in 0..src_caps.len() {
            dst_caps.set(
                idx,
                src_caps
                    .get(idx)
                    .map_err(|err| {
                        ServerError::Decode(format!("manifest binding cap entry {idx}: {err}"))
                    })?
                    .to_str()
                    .map_err(|err| {
                        ServerError::Decode(format!("manifest binding cap entry {idx} utf8: {err}"))
                    })?,
            );
        }
    }

    let mut out = Vec::new();
    serialize::write_message(&mut out, &builder)
        .map_err(|err| ServerError::Decode(format!("manifest binding write: {err}")))?;
    Ok(sha256_hex(&out))
}

#[cfg(feature = "idl-capnp")]
fn map_manifest_error(error: &ManifestError) -> InstallError {
    match error {
        ManifestError::MissingField(field) => match *field {
            "publisher" | "signature" => InstallError::Unsigned,
            _ => InstallError::Einval,
        },
        ManifestError::Decode(_) => InstallError::Einval,
        ManifestError::InvalidField { field, .. } => match *field {
            "signature" => InstallError::InvalidSig,
            "publisher" => InstallError::Unsigned,
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
        println!("bundlemgrd: ready");
        notifier.notify();
        let keystore = Some(KeystoreHandle::from_loopback(client_keystore));
        run_with_transport(&mut transport, artifacts, keystore, None)
    }

    #[cfg(nexus_env = "os")]
    {
        nexus_ipc::set_default_target("bundlemgrd");
        let server = nexus_ipc::KernelServer::new()
            .map_err(|err| ServerError::Transport(TransportError::from(err)))?;
        let mut transport = IpcTransport::new(server);
        println!("bundlemgrd: ready");
        notifier.notify();
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
pub fn loopback_transport() -> (nexus_ipc::LoopbackClient, IpcTransport<nexus_ipc::LoopbackServer>)
{
    let (client, server) = nexus_ipc::loopback_channel();
    (client, IpcTransport::new(server))
}

#[cfg(all(test, feature = "idl-capnp", nexus_env = "host"))]
mod tests {
    use super::*;
    use capnp::message::Builder;
    use nexus_idl_runtime::bundlemgr_capnp::{install_request, install_response};
    use nexus_idl_runtime::manifest_capnp::bundle_manifest;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    struct FakeKeystore {
        verify_ok: bool,
        allowed: bool,
        reason: &'static str,
    }

    impl KeystoreClient for FakeKeystore {
        fn verify(
            &self,
            _anchor_id: &str,
            _payload: &[u8],
            _signature: &[u8],
        ) -> Result<bool, KeystoreClientError> {
            Ok(self.verify_ok)
        }

        fn is_key_allowed(
            &self,
            _publisher: &str,
            _alg: &str,
            _pubkey: &[u8],
        ) -> Result<KeyAllowDecision, KeystoreClientError> {
            Ok(KeyAllowDecision { allowed: self.allowed, reason: self.reason.to_string() })
        }
    }

    fn build_install_payload(name: &str, handle: u32, bytes_len: u32) -> Vec<u8> {
        let mut message = Builder::new_default();
        {
            let mut req = message.init_root::<install_request::Builder<'_>>();
            req.set_name(name);
            req.set_vmo_handle(handle);
            req.set_bytes_len(bytes_len);
        }
        let mut payload = Vec::new();
        capnp::serialize::write_message(&mut payload, &message).expect("serialize install payload");
        payload
    }

    fn parse_install_response(frame: &[u8]) -> (bool, InstallError) {
        let mut cursor = Cursor::new(frame);
        let message = capnp::serialize::read_message(&mut cursor, ReaderOptions::new())
            .expect("read install response");
        let response =
            message.get_root::<install_response::Reader<'_>>().expect("install response root");
        (response.get_ok(), response.get_err().unwrap_or(InstallError::Einval))
    }

    fn build_manifest(publisher: [u8; 16], signature: &[u8]) -> Vec<u8> {
        let mut message = Builder::new_default();
        {
            let mut m = message.init_root::<bundle_manifest::Builder<'_>>();
            m.set_schema_version(1);
            m.set_name("demo.bundle");
            m.set_semver("1.0.0");
            m.set_min_sdk("0.1.0");
            let mut abilities = m.reborrow().init_abilities(1);
            abilities.set(0, "demo");
            let mut caps = m.reborrow().init_capabilities(0);
            let _ = &mut caps;
            m.set_publisher(&publisher);
            m.set_signature(signature);
            m.set_payload_digest(&[]);
            m.set_payload_size(0);
            m.set_sbom_digest(&[]);
            m.set_repro_digest(&[]);
        }
        let mut bytes = Vec::new();
        capnp::serialize::write_message(&mut bytes, &message).expect("serialize manifest");
        bytes
    }

    fn manifest_with_digests(
        manifest: &[u8],
        payload: &[u8],
        sbom: Option<&[u8]>,
        repro_json: Option<&[u8]>,
    ) -> Vec<u8> {
        let mut cursor = Cursor::new(manifest);
        let message = capnp::serialize::read_message(&mut cursor, ReaderOptions::new())
            .expect("read manifest");
        let src = message.get_root::<bundle_manifest::Reader<'_>>().expect("manifest root");
        let mut out_builder = Builder::new_default();
        {
            let mut dst = out_builder.init_root::<bundle_manifest::Builder<'_>>();
            dst.set_schema_version(src.get_schema_version());
            dst.set_name(src.get_name().expect("name").to_str().expect("name utf8"));
            dst.set_semver(src.get_semver().expect("semver").to_str().expect("semver utf8"));
            dst.set_min_sdk(src.get_min_sdk().expect("minSdk").to_str().expect("minSdk utf8"));
            dst.set_publisher(src.get_publisher().expect("publisher"));
            dst.set_signature(src.get_signature().expect("signature"));
            dst.set_payload_digest(&hex::decode(sha256_hex(payload)).expect("payload digest"));
            dst.set_payload_size(payload.len() as u64);
            if let Some(sbom) = sbom {
                dst.set_sbom_digest(&hex::decode(sha256_hex(sbom)).expect("sbom digest"));
            } else {
                dst.set_sbom_digest(&[]);
            }
            if let Some(repro_json) = repro_json {
                dst.set_repro_digest(&hex::decode(sha256_hex(repro_json)).expect("repro digest"));
            } else {
                dst.set_repro_digest(&[]);
            }
            let src_abilities = src.get_abilities().expect("abilities");
            let mut dst_abilities = dst.reborrow().init_abilities(src_abilities.len());
            for idx in 0..src_abilities.len() {
                dst_abilities.set(
                    idx,
                    src_abilities.get(idx).expect("ability entry").to_str().expect("ability utf8"),
                );
            }
            let src_caps = src.get_capabilities().expect("caps");
            let mut dst_caps = dst.reborrow().init_capabilities(src_caps.len());
            for idx in 0..src_caps.len() {
                dst_caps
                    .set(idx, src_caps.get(idx).expect("cap entry").to_str().expect("cap utf8"));
            }
        }
        let mut out = Vec::new();
        capnp::serialize::write_message(&mut out, &out_builder).expect("serialize manifest");
        out
    }

    fn sbom_and_repro(
        manifest: &[u8],
        payload: &[u8],
        publisher_hex: &str,
    ) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let manifest_for_binding = manifest_with_digests(manifest, payload, None, None);
        let manifest_binding_sha = sha256_hex(&manifest_for_binding);
        let sbom = sbom::generate_bundle_sbom_json(&sbom::BundleSbomInput {
            bundle_name: "demo.bundle".to_string(),
            bundle_version: "1.0.0".to_string(),
            publisher_hex: publisher_hex.to_string(),
            payload_sha256: sha256_hex(payload),
            payload_size: payload.len() as u64,
            manifest_sha256: manifest_binding_sha.clone(),
            source_date_epoch: 0,
            components: Vec::new(),
        })
        .expect("generate sbom");
        let repro = repro::capture_bundle_repro_json_with_manifest_digest(
            &manifest_binding_sha,
            payload,
            &sbom,
        )
        .expect("generate repro");
        let final_manifest = manifest_with_digests(manifest, payload, Some(&sbom), Some(&repro));
        (final_manifest, sbom, repro)
    }

    fn run_install(
        keystore: FakeKeystore,
        manifest: Vec<u8>,
        payload: Vec<u8>,
        sbom: Vec<u8>,
        repro_json: Vec<u8>,
    ) -> (bool, InstallError) {
        run_install_with_audit_fail(keystore, manifest, payload, sbom, repro_json, false)
    }

    fn run_install_with_audit_fail(
        keystore: FakeKeystore,
        manifest: Vec<u8>,
        payload: Vec<u8>,
        sbom: Vec<u8>,
        repro_json: Vec<u8>,
        audit_fail: bool,
    ) -> (bool, InstallError) {
        let _guard = TEST_LOCK.lock().expect("test lock");
        let previous_audit_flag = std::env::var("NEXUS_BUNDLEMGRD_AUDIT_FAIL").ok();
        if audit_fail {
            std::env::set_var("NEXUS_BUNDLEMGRD_AUDIT_FAIL", "1");
        } else {
            std::env::remove_var("NEXUS_BUNDLEMGRD_AUDIT_FAIL");
        }
        let artifacts = ArtifactStore::new();
        artifacts.insert(7, manifest.clone());
        artifacts.stage_payload(7, payload.clone());
        artifacts.stage_asset(7, "meta/sbom.json", sbom);
        artifacts.stage_asset(7, "meta/repro.env.json", repro_json);

        let mut server = Server::new(Service::new(), artifacts, Some(Box::new(keystore)), None);
        let install_payload = build_install_payload("demo.bundle", 7, manifest.len() as u32);
        let frame = server.handle_install(&install_payload).expect("handle install");
        let result = parse_install_response(&frame[1..]);
        match previous_audit_flag {
            Some(value) => std::env::set_var("NEXUS_BUNDLEMGRD_AUDIT_FAIL", value),
            None => std::env::remove_var("NEXUS_BUNDLEMGRD_AUDIT_FAIL"),
        }
        result
    }

    #[test]
    fn supply_chain_install_ok() {
        let publisher = [0u8; 16];
        let publisher_hex = hex::encode(publisher);
        let manifest = build_manifest(publisher, &[0x11; 64]);
        let payload = vec![0xde, 0xad, 0xbe, 0xef];
        let (manifest, sbom, repro_json) = sbom_and_repro(&manifest, &payload, &publisher_hex);
        let (ok, err) = run_install(
            FakeKeystore { verify_ok: true, allowed: true, reason: "allow" },
            manifest,
            payload,
            sbom,
            repro_json,
        );
        assert!(ok);
        assert_eq!(err, InstallError::None);
    }

    #[test]
    fn test_reject_unknown_publisher() {
        let publisher = [0u8; 16];
        let publisher_hex = hex::encode(publisher);
        let manifest = build_manifest(publisher, &[0x11; 64]);
        let payload = vec![1, 2, 3, 4];
        let (manifest, sbom, repro_json) = sbom_and_repro(&manifest, &payload, &publisher_hex);
        let (ok, err) = run_install(
            FakeKeystore { verify_ok: true, allowed: false, reason: "publisher_unknown" },
            manifest,
            payload,
            sbom,
            repro_json,
        );
        assert!(!ok);
        assert_eq!(err, InstallError::Eacces);
    }

    #[test]
    fn test_reject_unknown_key() {
        let publisher = [0u8; 16];
        let publisher_hex = hex::encode(publisher);
        let manifest = build_manifest(publisher, &[0x11; 64]);
        let payload = vec![1, 2, 3, 4];
        let (manifest, sbom, repro_json) = sbom_and_repro(&manifest, &payload, &publisher_hex);
        let (ok, err) = run_install(
            FakeKeystore { verify_ok: true, allowed: false, reason: "key_unknown" },
            manifest,
            payload,
            sbom,
            repro_json,
        );
        assert!(!ok);
        assert_eq!(err, InstallError::Eacces);
    }

    #[test]
    fn test_reject_unsupported_alg() {
        let publisher = [0u8; 16];
        let publisher_hex = hex::encode(publisher);
        let manifest = build_manifest(publisher, &[0x11; 64]);
        let payload = vec![1, 2, 3, 4];
        let (manifest, sbom, repro_json) = sbom_and_repro(&manifest, &payload, &publisher_hex);
        let (ok, err) = run_install(
            FakeKeystore { verify_ok: true, allowed: false, reason: "alg_unsupported" },
            manifest,
            payload,
            sbom,
            repro_json,
        );
        assert!(!ok);
        assert_eq!(err, InstallError::Eacces);
    }

    #[test]
    fn test_reject_payload_digest_mismatch() {
        let publisher = [0u8; 16];
        let publisher_hex = hex::encode(publisher);
        let manifest = build_manifest(publisher, &[0x11; 64]);
        let payload = vec![1, 2, 3, 4];
        let (manifest, sbom, mut repro_json) = sbom_and_repro(&manifest, &payload, &publisher_hex);
        repro_json[20] ^= 0x01;
        let (ok, err) = run_install(
            FakeKeystore { verify_ok: true, allowed: true, reason: "allow" },
            manifest,
            payload,
            sbom,
            repro_json,
        );
        assert!(!ok);
        assert_eq!(err, InstallError::Einval);
    }

    #[test]
    fn test_reject_repro_schema_invalid() {
        let publisher = [0u8; 16];
        let publisher_hex = hex::encode(publisher);
        let manifest = build_manifest(publisher, &[0x11; 64]);
        let payload = vec![1, 2, 3, 4];
        let (manifest, sbom, _repro_json) = sbom_and_repro(&manifest, &payload, &publisher_hex);
        let invalid_repro = br#"{"schema_version":1}"#.to_vec();
        let (ok, err) = run_install(
            FakeKeystore { verify_ok: true, allowed: true, reason: "allow" },
            manifest,
            payload,
            sbom,
            invalid_repro,
        );
        assert!(!ok);
        assert_eq!(err, InstallError::Einval);
    }

    #[test]
    fn test_reject_audit_unreachable() {
        let publisher = [0u8; 16];
        let publisher_hex = hex::encode(publisher);
        let manifest = build_manifest(publisher, &[0x11; 64]);
        let payload = vec![1, 2, 3, 4];
        let (manifest, sbom, repro_json) = sbom_and_repro(&manifest, &payload, &publisher_hex);
        let (ok, err) = run_install_with_audit_fail(
            FakeKeystore { verify_ok: true, allowed: true, reason: "allow" },
            manifest,
            payload,
            sbom,
            repro_json,
            true,
        );
        assert!(!ok);
        assert_eq!(err, InstallError::Eacces);
    }

    #[test]
    fn test_reject_sbom_oversize() {
        let publisher = [0u8; 16];
        let publisher_hex = hex::encode(publisher);
        let manifest = build_manifest(publisher, &[0x11; 64]);
        let payload = vec![1, 2, 3, 4];
        let (manifest, _sbom, repro_json) = sbom_and_repro(&manifest, &payload, &publisher_hex);
        let oversized_sbom = vec![b'a'; MAX_SBOM_BYTES + 1];
        let (ok, err) = run_install(
            FakeKeystore { verify_ok: true, allowed: true, reason: "allow" },
            manifest,
            payload,
            oversized_sbom,
            repro_json,
        );
        assert!(!ok);
        assert_eq!(err, InstallError::Einval);
    }

    #[test]
    fn test_reject_sbom_digest_mismatch() {
        let publisher = [0u8; 16];
        let publisher_hex = hex::encode(publisher);
        let manifest = build_manifest(publisher, &[0x11; 64]);
        let payload = vec![1, 2, 3, 4];
        let (manifest, mut sbom, repro_json) = sbom_and_repro(&manifest, &payload, &publisher_hex);
        sbom.push(b' ');
        let (ok, err) = run_install(
            FakeKeystore { verify_ok: true, allowed: true, reason: "allow" },
            manifest,
            payload,
            sbom,
            repro_json,
        );
        assert!(!ok);
        assert_eq!(err, InstallError::Einval);
    }

    #[test]
    fn test_reject_repro_digest_mismatch() {
        let publisher = [0u8; 16];
        let publisher_hex = hex::encode(publisher);
        let manifest = build_manifest(publisher, &[0x11; 64]);
        let payload = vec![1, 2, 3, 4];
        let (manifest, sbom, mut repro_json) = sbom_and_repro(&manifest, &payload, &publisher_hex);
        repro_json.push(b' ');
        let (ok, err) = run_install(
            FakeKeystore { verify_ok: true, allowed: true, reason: "allow" },
            manifest,
            payload,
            sbom,
            repro_json,
        );
        assert!(!ok);
        assert_eq!(err, InstallError::Einval);
    }

    #[test]
    fn test_reject_sbom_secret_leak() {
        let input = sbom::BundleSbomInput {
            bundle_name: "demo.bundle".to_string(),
            bundle_version: "1.0.0".to_string(),
            publisher_hex: "00000000000000000000000000000000".to_string(),
            payload_sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_string(),
            payload_size: 1,
            manifest_sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .to_string(),
            source_date_epoch: 0,
            components: vec![sbom::SbomComponentInput {
                name: "BEGIN PRIVATE KEY".to_string(),
                version: "1.0.0".to_string(),
                purl: None,
                sha256: None,
            }],
        };
        let err = sbom::generate_bundle_sbom_json(&input).expect_err("secret leak must fail");
        assert!(matches!(err, sbom::SbomError::SecretLeak { .. }));
    }
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
