// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Keystored daemon – loads anchor keys and verifies signatures via Cap'n Proto IPC.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 unit test
//! PUBLIC API: service_main_loop(), daemon_main(), loopback_transport()
//! DEPENDS_ON: nexus_ipc, nexus_idl_runtime (capnp), keystore lib
//! INVARIANTS: Separate from SAMgr/BundleMgr roles; stable readiness prints
//! ADR: docs/adr/0017-service-architecture.md

use std::collections::HashMap;
use std::env;
use std::fmt;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use keystore::{self, PublicKey};
use nexus_ipc::{self, Wait};
use thiserror::Error;

#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!(
    "nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '--cfg nexus_env=\"os\"'."
);

#[cfg(not(feature = "idl-capnp"))]
compile_error!("Enable the `idl-capnp` feature to build keystored handlers.");

#[cfg(feature = "idl-capnp")]
use capnp::message::{Builder, HeapAllocator, ReaderOptions};
#[cfg(feature = "idl-capnp")]
use capnp::serialize;
#[cfg(feature = "idl-capnp")]
use capnp::serialize::OwnedSegments;
#[cfg(feature = "idl-capnp")]
use nexus_idl_runtime::keystored_capnp::{
    device_id_request, device_id_response, get_anchors_request, get_anchors_response,
    is_key_allowed_request, is_key_allowed_response, verify_request, verify_response,
};

const OPCODE_GET_ANCHORS: u8 = 1;
const OPCODE_VERIFY: u8 = 2;
const OPCODE_DEVICE_ID: u8 = 3;
const OPCODE_IS_KEY_ALLOWED: u8 = 4;

/// Trait implemented by transports capable of delivering request frames.
pub trait Transport {
    /// Error type surfaced by the transport implementation.
    type Error: Into<TransportError>;

    /// Receives the next request frame if one is available.
    fn recv(&mut self) -> Result<Option<Vec<u8>>, Self::Error>;

    /// Sends a response frame back to the caller.
    fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error>;
}

/// Errors emitted by transports when interacting with the daemon.
#[derive(Debug)]
pub enum TransportError {
    /// Transport has been closed by the peer.
    Closed,
    /// I/O error while reading from or writing to the transport.
    Io(std::io::Error),
    /// Current platform lacks an implementation for the transport.
    Unsupported,
    /// Any other error described by a string message.
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
            nexus_ipc::IpcError::NoSpace => Self::Other("ipc ran out of resources".to_string()),
            nexus_ipc::IpcError::Kernel(inner) => {
                Self::Other(format!("kernel ipc error: {inner:?}"))
            }
            _ => Self::Other(format!("ipc error: {err:?}")),
        }
    }
}

/// Notifies init that the daemon has completed its startup sequence.
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    /// Creates a notifier from a closure.
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

/// IPC transport backed by the [`nexus-ipc`] runtime.
pub struct IpcTransport<T> {
    server: T,
}

impl<T> IpcTransport<T> {
    /// Wraps the provided server implementation.
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

/// Errors surfaced by the keystore server.
#[derive(Debug, Error)]
pub enum ServerError {
    /// Transport level issue.
    #[error("transport error: {0}")]
    Transport(TransportError),
    /// Failed to decode an incoming request frame.
    #[error("decode error: {0}")]
    Decode(String),
    /// Failed to encode an outgoing response frame.
    #[error("encode error: {0}")]
    Encode(#[from] capnp::Error),
    /// Failed to initialize anchors from disk.
    #[error("init error: {0}")]
    Init(String),
}

impl From<TransportError> for ServerError {
    fn from(err: TransportError) -> Self {
        Self::Transport(err)
    }
}

pub(crate) struct AnchorStore {
    ordered: Vec<String>,
    by_id: HashMap<String, PublicKey>,
}

impl AnchorStore {
    fn new(keys: Vec<PublicKey>) -> Self {
        let mut ordered = Vec::with_capacity(keys.len());
        let mut by_id = HashMap::with_capacity(keys.len());
        for key in keys {
            let id = keystore::device_id(&key);
            ordered.push(id.clone());
            by_id.insert(id, key);
        }
        Self { ordered, by_id }
    }

    fn len(&self) -> usize {
        self.ordered.len()
    }

    fn ids(&self) -> &[String] {
        &self.ordered
    }

    fn get(&self, id: &str) -> Option<&PublicKey> {
        self.by_id.get(id)
    }

    fn primary_id(&self) -> Option<&str> {
        self.ordered.first().map(|id| id.as_str())
    }
}

#[derive(Default)]
pub(crate) struct AllowlistStore {
    entries: HashMap<String, PublisherAllowEntry>,
}

impl AllowlistStore {
    fn decision(&self, publisher: &str, alg: &str, pubkey: &[u8]) -> (bool, &'static str) {
        let Some(entry) = self.entries.get(publisher) else {
            return (false, "publisher_unknown");
        };
        if !entry.enabled {
            return (false, "disabled");
        }
        if !entry.allowed_algs.iter().any(|candidate| candidate == alg) {
            return (false, "alg_unsupported");
        }
        if !entry.keys.iter().any(|candidate| candidate.as_slice() == pubkey) {
            return (false, "key_unknown");
        }
        (true, "allow")
    }
}

pub(crate) struct PublisherAllowEntry {
    enabled: bool,
    allowed_algs: Vec<String>,
    keys: Vec<Vec<u8>>,
}

pub(crate) struct KeystoreService {
    anchors: AnchorStore,
    allowlist: AllowlistStore,
}

impl KeystoreService {
    fn new(anchors: AnchorStore, allowlist: AllowlistStore) -> Self {
        Self { anchors, allowlist }
    }

    fn anchors(&self) -> &AnchorStore {
        &self.anchors
    }

    fn allowlist(&self) -> &AllowlistStore {
        &self.allowlist
    }
}

struct Server {
    service: KeystoreService,
}

impl Server {
    fn new(service: KeystoreService) -> Self {
        Self { service }
    }

    fn handle_frame(&self, opcode: u8, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        match opcode {
            OPCODE_GET_ANCHORS => self.handle_get_anchors(payload),
            OPCODE_VERIFY => self.handle_verify(payload),
            OPCODE_DEVICE_ID => self.handle_device_id(payload),
            OPCODE_IS_KEY_ALLOWED => self.handle_is_key_allowed(payload),
            _ => Err(ServerError::Decode(format!("unknown opcode {opcode}"))),
        }
    }

    #[cfg(feature = "idl-capnp")]
    fn handle_get_anchors(&self, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        let message = read_message(payload)?;
        let _request: get_anchors_request::Reader<'_> =
            message.get_root().map_err(|err| ServerError::Decode(err.to_string()))?;
        let mut message = Builder::new_default();
        {
            let response = message.init_root::<get_anchors_response::Builder<'_>>();
            let mut list = response.init_anchors(self.service.anchors().len() as u32);
            for (idx, id) in self.service.anchors().ids().iter().enumerate() {
                list.set(idx as u32, id);
            }
        }
        encode_response(OPCODE_GET_ANCHORS, &message)
    }

    #[cfg(feature = "idl-capnp")]
    fn handle_verify(&self, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        let message = read_message(payload)?;
        let request: verify_request::Reader<'_> =
            message.get_root().map_err(|err| ServerError::Decode(err.to_string()))?;
        let anchor_id = request
            .get_anchor_id()
            .map_err(|err| ServerError::Decode(err.to_string()))?
            .to_str()
            .map_err(|err| ServerError::Decode(err.to_string()))?;
        let payload_reader =
            request.get_payload().map_err(|err| ServerError::Decode(err.to_string()))?;
        let signature_reader =
            request.get_signature().map_err(|err| ServerError::Decode(err.to_string()))?;
        let payload_bytes: Vec<u8> = payload_reader.to_vec();
        let signature_bytes: Vec<u8> = signature_reader.to_vec();

        let anchor_opt = self.service.anchors().get(anchor_id);
        eprintln!(
            "keystored: verify publisher={} anchor_present={} payload_len={} sig_len={}",
            anchor_id,
            anchor_opt.is_some(),
            payload_bytes.len(),
            signature_bytes.len()
        );
        let verified = anchor_opt
            .map(|key| keystore::verify_detached(key, &payload_bytes, &signature_bytes).is_ok())
            .unwrap_or(false);

        let mut message = Builder::new_default();
        {
            let mut response = message.init_root::<verify_response::Builder<'_>>();
            response.set_ok(verified);
        }
        encode_response(OPCODE_VERIFY, &message)
    }

    #[cfg(feature = "idl-capnp")]
    fn handle_device_id(&self, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        let message = read_message(payload)?;
        let _request: device_id_request::Reader<'_> =
            message.get_root().map_err(|err| ServerError::Decode(err.to_string()))?;
        let id = self.service.anchors().primary_id().unwrap_or("");

        let mut message = Builder::new_default();
        {
            let mut response = message.init_root::<device_id_response::Builder<'_>>();
            response.set_id(id);
        }
        encode_response(OPCODE_DEVICE_ID, &message)
    }

    #[cfg(feature = "idl-capnp")]
    fn handle_is_key_allowed(&self, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        let message = read_message(payload)?;
        let request: is_key_allowed_request::Reader<'_> =
            message.get_root().map_err(|err| ServerError::Decode(err.to_string()))?;
        let publisher = request
            .get_publisher()
            .map_err(|err| ServerError::Decode(err.to_string()))?
            .to_str()
            .map_err(|err| ServerError::Decode(err.to_string()))?
            .to_string();
        let alg = request
            .get_alg()
            .map_err(|err| ServerError::Decode(err.to_string()))?
            .to_str()
            .map_err(|err| ServerError::Decode(err.to_string()))?
            .to_string();
        let pubkey = request.get_pubkey().map_err(|err| ServerError::Decode(err.to_string()))?;

        let (allowed, reason) = self.service.allowlist().decision(&publisher, &alg, pubkey);

        let mut response = Builder::new_default();
        {
            let mut builder = response.init_root::<is_key_allowed_response::Builder<'_>>();
            builder.set_allowed(allowed);
            builder.set_reason(reason);
        }
        encode_response(OPCODE_IS_KEY_ALLOWED, &response)
    }
}

#[cfg(feature = "idl-capnp")]
fn read_message(payload: &[u8]) -> Result<capnp::message::Reader<OwnedSegments>, ServerError> {
    let mut cursor = Cursor::new(payload);
    serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|err| ServerError::Decode(err.to_string()))
}

#[cfg(feature = "idl-capnp")]
fn encode_response(opcode: u8, message: &Builder<HeapAllocator>) -> Result<Vec<u8>, ServerError> {
    let mut payload = Vec::new();
    serialize::write_message(&mut payload, message).map_err(ServerError::Encode)?;
    let mut frame = Vec::with_capacity(1 + payload.len());
    frame.push(opcode);
    frame.extend_from_slice(&payload);
    Ok(frame)
}

fn anchors_dir() -> PathBuf {
    if let Some(dir) = env::var_os("NEXUS_ANCHORS_DIR") {
        let path = PathBuf::from(dir);
        if path.is_dir() {
            return path;
        }
    }
    Path::new("recipes/keys").to_path_buf()
}

fn allowlist_path() -> PathBuf {
    if let Some(path) = env::var_os("NEXUS_SIGNING_ALLOWLIST") {
        return PathBuf::from(path);
    }
    Path::new("recipes/signing/publishers.toml").to_path_buf()
}

fn load_anchor_store() -> Result<AnchorStore, ServerError> {
    let dir = anchors_dir();
    let keys = keystore::load_anchors(&dir)
        .map_err(|err| ServerError::Init(format!("load anchors from {}: {err}", dir.display())))?;
    Ok(AnchorStore::new(keys))
}

fn load_allowlist_store() -> Result<AllowlistStore, ServerError> {
    let path = allowlist_path();
    if !path.exists() {
        return Ok(AllowlistStore::default());
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|err| ServerError::Init(format!("read allowlist {}: {err}", path.display())))?;
    let value: toml::Value = toml::from_str(&content)
        .map_err(|err| ServerError::Init(format!("parse allowlist {}: {err}", path.display())))?;
    let mut entries = HashMap::new();
    let Some(publishers) = value.get("publishers").and_then(toml::Value::as_array) else {
        return Ok(AllowlistStore { entries });
    };
    for publisher in publishers {
        let Some(table) = publisher.as_table() else {
            continue;
        };
        let Some(id) = table.get("id").and_then(toml::Value::as_str) else {
            continue;
        };
        let enabled = table.get("enabled").and_then(toml::Value::as_bool).unwrap_or(true);
        let allowed_algs = table
            .get("allowed_algs")
            .and_then(toml::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(toml::Value::as_str)
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let keys = table
            .get("keys")
            .and_then(toml::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(toml::Value::as_str)
                    .filter_map(|value| hex::decode(value).ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        entries
            .entry(id.to_string())
            .and_modify(|entry: &mut PublisherAllowEntry| {
                entry.allowed_algs.extend(allowed_algs.clone());
                entry.keys.extend(keys.clone());
                entry.enabled = enabled;
            })
            .or_insert_with(|| PublisherAllowEntry {
                enabled,
                allowed_algs: allowed_algs.clone(),
                keys: keys.clone(),
            });
    }
    Ok(AllowlistStore { entries })
}

/// Runs the server with the provided transport.
#[cfg(feature = "idl-capnp")]
pub(crate) fn run_with_transport<T: Transport>(
    transport: &mut T,
    anchors: AnchorStore,
    allowlist: AllowlistStore,
) -> Result<(), ServerError> {
    let service = KeystoreService::new(anchors, allowlist);
    serve_with_components(transport, service)
}

/// Runs the server with the provided transport, loading anchors from the default directory.
#[cfg(feature = "idl-capnp")]
pub fn run_with_transport_default_anchors<T: Transport>(
    transport: &mut T,
) -> Result<(), ServerError> {
    let anchors = load_anchor_store()?;
    let allowlist = load_allowlist_store()?;
    let service = KeystoreService::new(anchors, allowlist);
    serve_with_components(transport, service)
}

/// Serves requests using injected service components.
#[cfg(feature = "idl-capnp")]
pub(crate) fn serve_with_components<T: Transport>(
    transport: &mut T,
    service: KeystoreService,
) -> Result<(), ServerError> {
    let server = Server::new(service);
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

/// Touches Cap'n Proto schemas to ensure generated code is retained.
pub fn touch_schemas() {
    #[cfg(feature = "idl-capnp")]
    {
        let _ = core::any::type_name::<get_anchors_request::Reader<'static>>();
        let _ = core::any::type_name::<get_anchors_response::Reader<'static>>();
        let _ = core::any::type_name::<verify_request::Reader<'static>>();
        let _ = core::any::type_name::<verify_response::Reader<'static>>();
        let _ = core::any::type_name::<device_id_request::Reader<'static>>();
        let _ = core::any::type_name::<device_id_response::Reader<'static>>();
        let _ = core::any::type_name::<is_key_allowed_request::Reader<'static>>();
        let _ = core::any::type_name::<is_key_allowed_response::Reader<'static>>();
    }
}

/// Executes the server using the default transport for the current platform.
pub fn service_main_loop(notifier: ReadyNotifier) -> Result<(), ServerError> {
    #[cfg(nexus_env = "host")]
    {
        let anchors = load_anchor_store()?;
        let allowlist = load_allowlist_store()?;
        let count = anchors.len();
        println!("keystored: anchors={count}");
        let (client, server) = nexus_ipc::loopback_channel();
        let _client_guard = client;
        let mut transport = IpcTransport::new(server);
        notifier.notify();
        println!("keystored: ready");
        run_with_transport(&mut transport, anchors, allowlist)
    }

    #[cfg(nexus_env = "os")]
    {
        let anchors = load_anchor_store()?;
        let allowlist = load_allowlist_store()?;
        let count = anchors.len();
        println!("keystored: anchors={count}");
        let server = nexus_ipc::KernelServer::new()
            .map_err(|err| ServerError::Transport(TransportError::from(err)))?;
        let mut transport = IpcTransport::new(server);
        notifier.notify();
        println!("keystored: ready");
        run_with_transport(&mut transport, anchors, allowlist)
    }
}

/// Runs the daemon entry point until termination.
pub fn daemon_main<R: FnOnce() + Send + 'static>(notify: R) -> ! {
    touch_schemas();
    if let Err(err) = service_main_loop(ReadyNotifier::new(notify)) {
        eprintln!("keystored: {err}");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_store_tracks_order() {
        let mut store = AnchorStore::new(Vec::new());
        assert_eq!(store.len(), 0);

        let key = PublicKey::from_bytes(&[0u8; 32]).unwrap();
        store = AnchorStore::new(vec![key]);
        assert_eq!(store.len(), 1);
        let expected = keystore::device_id(&key);
        assert_eq!(store.primary_id(), Some(expected.as_str()));
    }

    #[test]
    fn is_key_allowed_decision_paths() {
        let mut allowlist = AllowlistStore::default();
        allowlist.entries.insert(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            PublisherAllowEntry {
                enabled: true,
                allowed_algs: vec!["ed25519".to_string()],
                keys: vec![vec![0xAA; 16], vec![0xBB; 16]],
            },
        );

        assert_eq!(
            allowlist.decision("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "ed25519", &[0xAA; 16]),
            (true, "allow")
        );
        assert_eq!(
            allowlist.decision("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "ed25519", &[0xAA; 16]),
            (false, "publisher_unknown")
        );
        assert_eq!(
            allowlist.decision("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "rsa", &[0xAA; 16]),
            (false, "alg_unsupported")
        );
        assert_eq!(
            allowlist.decision("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "ed25519", &[0xCC; 16]),
            (false, "key_unknown")
        );
    }
}
