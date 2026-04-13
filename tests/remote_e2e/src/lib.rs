//! CONTEXT: Remote end-to-end test harness library
//! INTENT: DSoftBus-lite stack testing with service nodes and remote operations
//! IDL (target): start(port,services), connect(peer), resolve(service), installBundle(name,handle,len)
//! DEPS: dsoftbus, samgrd, bundlemgrd, identity (service integration)
//! READINESS: Host backend ready; multiple nodes with service discovery
//! TESTS: Service discovery, remote resolution, bundle install, authentication
//! Host-only remote end-to-end harness exercising the DSoftBus-lite stack.
//!
//! The helpers defined here spin up a pair of service nodes (identityd,
//! samgrd, bundlemgrd, and dsoftbusd equivalents) entirely in-process. The
//! nodes communicate using `userspace/dsoftbus` host-first transports layered
//! over the `userspace/nexus-net` sockets facade contract (`FakeNet`), including
//! on-wire discovery announce packets (v1) and Noise-authenticated sessions.
//! Cap'n Proto frames are forwarded to the existing daemons, providing a
//! realistic control plane without booting QEMU.

#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use std::{collections::HashMap, convert::TryFrom};

use anyhow::{anyhow, Context, Result};
use bundlemgrd::{self, run_with_transport as bundle_run_with_transport, ArtifactStore};
use capnp::message::{Builder, ReaderOptions};
use capnp::serialize;
use dsoftbus::{
    Announcement, Authenticator, Discovery, FacadeAuthenticator, FacadeDiscovery, FramePayload,
    HostDiscovery, InProcAuthenticator, Session, Stream,
};
use identity::{DeviceId, Identity};
use nexus_idl_runtime::bundlemgr_capnp::{
    install_request, install_response, query_request, query_response,
};
use nexus_idl_runtime::samgr_capnp::{
    register_request, register_response, resolve_request, resolve_response,
};
use nexus_ipc::{Client, LoopbackClient, Wait};
use parking_lot::Mutex;
use rand::Rng;
use samgr::Registry;
use samgrd::serve_with_registry as samgr_serve_with_registry;
use thiserror::Error;

use nexus_net::fake::FakeNet;

const CHAN_SAMGR: u32 = 1;
const CHAN_BUNDLEMGR: u32 = 2;
const CHAN_ARTIFACT: u32 = 3;
const CHAN_PACKAGEFS: u32 = 4;

const PK_MAGIC0: u8 = b'P';
const PK_MAGIC1: u8 = b'K';
const PK_VERSION: u8 = 1;
const PK_OP_STAT: u8 = 1;
const PK_OP_OPEN: u8 = 2;
const PK_OP_READ: u8 = 3;
const PK_OP_CLOSE: u8 = 4;
const PK_STATUS_OK: u8 = 0;
const PK_STATUS_BAD_REQUEST: u8 = 1;
const PK_STATUS_PATH_TRAVERSAL: u8 = 3;
const PK_STATUS_NON_PACKAGEFS_SCHEME: u8 = 4;
const PK_STATUS_NOT_FOUND: u8 = 5;
const PK_STATUS_BADF: u8 = 6;
const PK_STATUS_OVERSIZED: u8 = 7;
const PK_STATUS_LIMIT: u8 = 8;
const PK_MAX_PATH_LEN: usize = 192;
const PK_MAX_READ_LEN: usize = 128;
const PK_MAX_HANDLES: usize = 8;
const PACKAGEFS_KIND_FILE: u16 = 0;
const PACKAGEFS_BUILD_PROP: &[u8] = b"ro.nexus.build=dev\n";

/// Artifact payload kinds supported by the remote harness.
#[derive(Clone, Copy, Debug)]
pub enum ArtifactKind {
    /// Manifest bytes staged prior to installation.
    Manifest = 0,
    /// Payload bytes paired with the manifest.
    Payload = 1,
}

impl ArtifactKind {
    fn as_u8(self) -> u8 {
        self as u8
    }
}

const OPCODE_REGISTER: u8 = 1;
const OPCODE_RESOLVE: u8 = 2;
const OPCODE_INSTALL: u8 = 1;
const OPCODE_QUERY: u8 = 2;

/// Errors produced by the remote harness helpers.
#[derive(Debug, Error)]
pub enum HarnessError {
    /// Failure when forwarding a frame to a local daemon.
    #[error("ipc forwarding failed: {0}")]
    Forward(String),
    /// Received an unexpected or malformed frame.
    #[error("protocol error: {0}")]
    Protocol(String),
}

#[derive(Clone)]
enum AuthBackend {
    InProc(Arc<InProcAuthenticator>),
    Facade(Arc<FacadeAuthenticator<FakeNet>>),
}

enum DiscoveryBackend {
    Host(HostDiscovery),
    Facade(FacadeDiscovery<FakeNet>),
}

/// Represents a running host node exposing DSoftBus-lite services.
pub struct Node {
    authenticator: AuthBackend,
    discovery: DiscoveryBackend,
    announcement: Announcement,
    samgr_client: Arc<LoopbackClient>,
    #[allow(dead_code)]
    bundle_client: Arc<LoopbackClient>,
    #[allow(dead_code)]
    artifact_store: ArtifactStore,
    accept_thread: Option<JoinHandle<()>>,
    samgr_thread: Option<JoinHandle<()>>,
    bundle_thread: Option<JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
}

impl Node {
    /// Boots a node using randomly generated identity material and binds the
    /// DSoftBus authenticator to `listen_port`.
    pub fn start(listen_port: u16, services: Vec<String>) -> Result<Self> {
        let identity = Identity::generate().context("generate identity")?;
        let listen_addr = SocketAddr::from(([127, 0, 0, 1], listen_port));
        let authenticator = InProcAuthenticator::bind(listen_addr, identity.clone())
            .context("bind host authenticator")?;
        let discovery = HostDiscovery::new();
        let published_port = authenticator.local_port();
        let announcement = Announcement::new(
            identity.device_id().clone(),
            services,
            published_port,
            authenticator.local_noise_public(),
        );
        discovery.announce(announcement.clone()).context("announce local node")?;

        // samgrd loopback transport and server thread
        let (samgr_client, samgr_server) = samgrd::loopback_transport();
        let registry = Registry::new();
        let samgr_thread = thread::spawn(move || {
            let mut transport = samgr_server;
            if let Err(err) = samgr_serve_with_registry(&mut transport, registry) {
                eprintln!("samgrd loop terminated: {err}");
            }
        });
        let samgr_client = Arc::new(samgr_client);

        // bundlemgrd loopback transport and server thread
        let (bundle_client, bundle_server) = bundlemgrd::loopback_transport();
        let artifacts = ArtifactStore::new();
        let artifact_clone = artifacts.clone();
        let bundle_thread = thread::spawn(move || {
            let mut transport = bundle_server;
            if let Err(err) = bundle_run_with_transport(&mut transport, artifact_clone, None, None)
            {
                eprintln!("bundlemgrd loop terminated: {err}");
            }
        });
        let bundle_client = Arc::new(bundle_client);

        let shutdown = Arc::new(AtomicBool::new(false));
        let acceptor = Arc::new(authenticator);
        let acceptor_thread = Arc::clone(&acceptor);
        let samgr_bridge = Arc::clone(&samgr_client);
        let bundle_bridge = Arc::clone(&bundle_client);
        let store_bridge = artifacts.clone();
        let stop_flag = Arc::clone(&shutdown);
        let accept_thread = thread::spawn(move || {
            while !stop_flag.load(Ordering::SeqCst) {
                match acceptor_thread.accept() {
                    Ok(session) => {
                        if let Ok(stream) = session.into_stream() {
                            let samgr_client = Arc::clone(&samgr_bridge);
                            let bundle_client = Arc::clone(&bundle_bridge);
                            let store = store_bridge.clone();
                            thread::spawn(move || {
                                if let Err(err) = handle_session(
                                    Box::new(stream),
                                    samgr_client,
                                    bundle_client,
                                    store,
                                ) {
                                    eprintln!("dsoftbus session ended with error: {err}");
                                }
                            });
                        }
                    }
                    Err(err) => {
                        eprintln!("accept error: {err}");
                        thread::sleep(Duration::from_millis(10));
                    }
                }
            }
        });

        Ok(Self {
            authenticator: AuthBackend::InProc(acceptor),
            discovery: DiscoveryBackend::Host(discovery),
            announcement,
            samgr_client,
            bundle_client,
            artifact_store: artifacts,
            accept_thread: Some(accept_thread),
            samgr_thread: Some(samgr_thread),
            bundle_thread: Some(bundle_thread),
            shutdown,
        })
    }

    /// Boots a node that uses DSoftBus over the sockets facade contract (`nexus-net`).
    ///
    /// This is host-first and deterministic when paired with `nexus_net::fake::FakeNet`.
    pub fn start_facade(net: FakeNet, listen_port: u16, services: Vec<String>) -> Result<Self> {
        let identity = Identity::generate().context("generate identity")?;
        let listen_addr = SocketAddr::from(([127, 0, 0, 1], listen_port));

        let net_for_auth = net.clone();
        let net_for_disc = net;

        // Build a facade-backed authenticator and adapt it through the in-proc style surface by
        // using the dsoftbus facade transport directly for sessions.
        //
        // For `remote_e2e` we only need connect/accept + into_stream; the server-side session loop
        // is already transport-agnostic via `Stream`.
        let authenticator = FacadeAuthenticator::new(net_for_auth, listen_addr, identity.clone())
            .context("bind facade authenticator")?;

        // Discovery bus (host-first): all nodes bind the same UDP address and broadcast to it.
        // Deterministic under FakeNet; proves the on-wire announce packet.
        let bus = SocketAddr::from(([127, 0, 0, 1], 37020));
        let discovery =
            FacadeDiscovery::new(net_for_disc, bus, bus).context("bind facade discovery")?;
        let published_port = authenticator.local_port();
        let announcement = Announcement::new(
            identity.device_id().clone(),
            services,
            published_port,
            authenticator.local_noise_public(),
        );
        discovery.announce(announcement.clone()).context("announce local node")?;

        // samgrd loopback transport and server thread
        let (samgr_client, samgr_server) = samgrd::loopback_transport();
        let registry = Registry::new();
        let samgr_thread = thread::spawn(move || {
            let mut transport = samgr_server;
            if let Err(err) = samgr_serve_with_registry(&mut transport, registry) {
                eprintln!("samgrd loop terminated: {err}");
            }
        });
        let samgr_client = Arc::new(samgr_client);

        // bundlemgrd loopback transport and server thread
        let (bundle_client, bundle_server) = bundlemgrd::loopback_transport();
        let artifacts = ArtifactStore::new();
        let artifact_clone = artifacts.clone();
        let bundle_thread = thread::spawn(move || {
            let mut transport = bundle_server;
            if let Err(err) = bundle_run_with_transport(&mut transport, artifact_clone, None, None)
            {
                eprintln!("bundlemgrd loop terminated: {err}");
            }
        });
        let bundle_client = Arc::new(bundle_client);

        let shutdown = Arc::new(AtomicBool::new(false));
        let acceptor = Arc::new(authenticator);
        let acceptor_thread = Arc::clone(&acceptor);
        let samgr_bridge = Arc::clone(&samgr_client);
        let bundle_bridge = Arc::clone(&bundle_client);
        let store_bridge = artifacts.clone();
        let stop_flag = Arc::clone(&shutdown);
        let accept_thread = thread::spawn(move || {
            while !stop_flag.load(Ordering::SeqCst) {
                match acceptor_thread.accept() {
                    Ok(session) => match session.into_stream() {
                        Ok(stream) => {
                            let samgr_client = Arc::clone(&samgr_bridge);
                            let bundle_client = Arc::clone(&bundle_bridge);
                            let store = store_bridge.clone();
                            thread::spawn(move || {
                                if let Err(err) = handle_session(
                                    Box::new(stream),
                                    samgr_client,
                                    bundle_client,
                                    store,
                                ) {
                                    eprintln!("dsoftbus session ended with error: {err}");
                                }
                            });
                        }
                        Err(err) => eprintln!("stream negotiation failed: {err}"),
                    },
                    Err(err) => {
                        eprintln!("accept error: {err}");
                        thread::sleep(Duration::from_millis(10));
                    }
                }
            }
        });

        Ok(Self {
            authenticator: AuthBackend::Facade(acceptor),
            discovery: DiscoveryBackend::Facade(discovery),
            announcement,
            samgr_client,
            bundle_client,
            artifact_store: artifacts,
            accept_thread: Some(accept_thread),
            samgr_thread: Some(samgr_thread),
            bundle_thread: Some(bundle_thread),
            shutdown,
        })
    }

    /// Returns the device identifier assigned to this node.
    pub fn device_id(&self) -> DeviceId {
        match &self.authenticator {
            AuthBackend::InProc(auth) => auth.identity().device_id().clone(),
            AuthBackend::Facade(auth) => auth.identity().device_id().clone(),
        }
    }

    /// Returns a clone of the local announcement payload.
    pub fn announcement(&self) -> Announcement {
        self.announcement.clone()
    }

    /// Returns a discovery iterator seeded with the current registry state.
    pub fn watch(&self) -> Result<Box<dyn Iterator<Item = Announcement>>> {
        match &self.discovery {
            DiscoveryBackend::Host(d) => {
                Ok(Box::new(d.watch().map_err(|err| anyhow!(err.to_string()))?))
            }
            DiscoveryBackend::Facade(d) => {
                Ok(Box::new(d.watch().map_err(|err| anyhow!(err.to_string()))?))
            }
        }
    }

    /// Attempts to retrieve an announcement for `device` from the registry.
    pub fn get_announcement(&self, device: &DeviceId) -> Result<Option<Announcement>> {
        match &self.discovery {
            DiscoveryBackend::Host(d) => d.get(device).map_err(|err| anyhow!(err.to_string())),
            DiscoveryBackend::Facade(d) => d.get(device).map_err(|err| anyhow!(err.to_string())),
        }
    }

    /// Registers a local service name with the SAMGR daemon.
    pub fn register_service(&self, name: &str, endpoint: u32) -> Result<()> {
        let frame = build_samgr_register(name, endpoint)?;
        eprintln!(
            "[remote_e2e] samgr.register sending frame len={} (name={}, ep={})",
            frame.len(),
            name,
            endpoint
        );
        let response =
            forward_ipc(&self.samgr_client, frame).map_err(|err| anyhow!(err.to_string()))?;
        eprintln!("[remote_e2e] samgr.register got response len={}", response.len());
        if !parse_samgr_register(&response)? {
            return Err(anyhow!("samgr register rejected"));
        }
        Ok(())
    }

    /// Connects to `peer` and returns a handle used for remote operations.
    pub fn connect(&self, peer: &Announcement) -> Result<RemoteConnection> {
        let stream: Box<dyn Stream + Send> = match &self.authenticator {
            AuthBackend::InProc(auth) => Box::new(
                auth.connect(peer)
                    .context("connect to remote peer")?
                    .into_stream()
                    .context("stream negotiation")?,
            ),
            AuthBackend::Facade(auth) => Box::new(
                auth.connect(peer)
                    .context("connect to remote peer")?
                    .into_stream()
                    .context("stream negotiation")?,
            ),
        };
        Ok(RemoteConnection::new(stream))
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(h) = self.accept_thread.take() {
            let _ = h.join();
        }
        // Allow daemon threads to terminate on process exit; do not block test teardown.
        let _ = self.samgr_thread.take();
        let _ = self.bundle_thread.take();
    }
}

fn handle_session(
    mut stream: Box<dyn Stream + Send>,
    samgr: Arc<LoopbackClient>,
    bundle: Arc<LoopbackClient>,
    artifacts: ArtifactStore,
) -> Result<(), HarnessError> {
    let mut pkg_handles: HashMap<u32, Vec<u8>> = HashMap::new();
    let mut next_pkg_handle: u32 = 1;
    loop {
        match stream.recv() {
            Ok(Some(FramePayload { channel, bytes })) => match channel {
                CHAN_SAMGR => {
                    eprintln!("[remote_e2e] server: CHAN_SAMGR recv len={}", bytes.len());
                    let response = forward_ipc(&samgr, bytes)
                        .map_err(|err| HarnessError::Forward(err.to_string()))?;
                    eprintln!("[remote_e2e] server: CHAN_SAMGR rsp len={}", response.len());
                    stream
                        .send(CHAN_SAMGR, &response)
                        .map_err(|err| HarnessError::Forward(err.to_string()))?;
                }
                CHAN_BUNDLEMGR => {
                    eprintln!("[remote_e2e] server: CHAN_BUNDLEMGR recv len={}", bytes.len());
                    let response = forward_ipc(&bundle, bytes)
                        .map_err(|err| HarnessError::Forward(err.to_string()))?;
                    eprintln!("[remote_e2e] server: CHAN_BUNDLEMGR rsp len={}", response.len());
                    stream
                        .send(CHAN_BUNDLEMGR, &response)
                        .map_err(|err| HarnessError::Forward(err.to_string()))?;
                }
                CHAN_ARTIFACT => {
                    if bytes.len() < 5 {
                        return Err(HarnessError::Protocol("artifact frame too small".into()));
                    }
                    let handle_bytes: [u8; 4] = match bytes[0..4].try_into() {
                        Ok(b) => b,
                        Err(_) => {
                            return Err(HarnessError::Protocol("artifact handle length".into()))
                        }
                    };
                    let handle = u32::from_be_bytes(handle_bytes);
                    let kind = bytes[4];
                    let payload = bytes[5..].to_vec();
                    eprintln!(
                        "[remote_e2e] server: CHAN_ARTIFACT handle={} kind={} len={}",
                        handle,
                        kind,
                        payload.len()
                    );
                    match kind {
                        x if x == ArtifactKind::Manifest.as_u8() => {
                            artifacts.insert(handle, payload);
                        }
                        x if x == ArtifactKind::Payload.as_u8() => {
                            artifacts.stage_payload(handle, payload);
                        }
                        other => {
                            return Err(HarnessError::Protocol(format!(
                                "unknown artifact kind {other}"
                            )));
                        }
                    }
                    stream
                        .send(CHAN_ARTIFACT, &[])
                        .map_err(|err| HarnessError::Forward(err.to_string()))?;
                }
                CHAN_PACKAGEFS => {
                    let response =
                        handle_packagefs_frame(&bytes, &mut pkg_handles, &mut next_pkg_handle);
                    stream
                        .send(CHAN_PACKAGEFS, &response)
                        .map_err(|err| HarnessError::Forward(err.to_string()))?;
                }
                other => {
                    return Err(HarnessError::Protocol(format!("unknown channel {other}")));
                }
            },
            Ok(None) => break,
            Err(err) => return Err(HarnessError::Forward(err.to_string())),
        }
    }
    Ok(())
}

fn forward_ipc(client: &LoopbackClient, frame: Vec<u8>) -> Result<Vec<u8>, HarnessError> {
    eprintln!("[remote_e2e] forward_ipc tx len={}", frame.len());
    client.send(&frame, Wait::Blocking).map_err(|err| HarnessError::Forward(err.to_string()))?;
    let rsp = client.recv(Wait::Blocking).map_err(|err| HarnessError::Forward(err.to_string()))?;
    eprintln!("[remote_e2e] forward_ipc rx len={}", rsp.len());
    Ok(rsp)
}

fn handle_packagefs_frame(
    frame: &[u8],
    handles: &mut HashMap<u32, Vec<u8>>,
    next_handle: &mut u32,
) -> Vec<u8> {
    if frame.len() < 4 {
        return encode_pkg_status(PK_OP_STAT, PK_STATUS_BAD_REQUEST);
    }
    if frame[0] != PK_MAGIC0 || frame[1] != PK_MAGIC1 || frame[2] != PK_VERSION {
        let op = if frame.len() >= 4 { frame[3] } else { PK_OP_STAT };
        return encode_pkg_status(op, PK_STATUS_BAD_REQUEST);
    }

    match frame[3] {
        PK_OP_STAT => {
            let rel = match parse_pkg_path_request(frame) {
                Ok(v) => v,
                Err(status) => return encode_pkg_stat(status, 0, 0),
            };
            match resolve_pkg_path(&rel) {
                Some(bytes) => {
                    encode_pkg_stat(PK_STATUS_OK, bytes.len() as u64, PACKAGEFS_KIND_FILE)
                }
                None => encode_pkg_stat(PK_STATUS_NOT_FOUND, 0, 0),
            }
        }
        PK_OP_OPEN => {
            let rel = match parse_pkg_path_request(frame) {
                Ok(v) => v,
                Err(status) => return encode_pkg_open(status, 0),
            };
            let Some(bytes) = resolve_pkg_path(&rel) else {
                return encode_pkg_open(PK_STATUS_NOT_FOUND, 0);
            };
            if handles.len() >= PK_MAX_HANDLES {
                return encode_pkg_open(PK_STATUS_LIMIT, 0);
            }
            let Some(handle) = allocate_pkg_handle(handles, next_handle) else {
                return encode_pkg_open(PK_STATUS_LIMIT, 0);
            };
            handles.insert(handle, bytes);
            encode_pkg_open(PK_STATUS_OK, handle)
        }
        PK_OP_READ => {
            if frame.len() != 14 {
                return encode_pkg_read(PK_STATUS_BAD_REQUEST, &[]);
            }
            let handle = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
            let offset = u32::from_le_bytes([frame[8], frame[9], frame[10], frame[11]]) as usize;
            let read_len = u16::from_le_bytes([frame[12], frame[13]]) as usize;
            if read_len == 0 || read_len > PK_MAX_READ_LEN {
                return encode_pkg_read(PK_STATUS_OVERSIZED, &[]);
            }
            let Some(data) = handles.get(&handle) else {
                return encode_pkg_read(PK_STATUS_BADF, &[]);
            };
            let start = core::cmp::min(offset, data.len());
            let end = core::cmp::min(start.saturating_add(read_len), data.len());
            encode_pkg_read(PK_STATUS_OK, &data[start..end])
        }
        PK_OP_CLOSE => {
            if frame.len() != 8 {
                return encode_pkg_status(PK_OP_CLOSE, PK_STATUS_BAD_REQUEST);
            }
            let handle = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
            if handles.remove(&handle).is_some() {
                encode_pkg_status(PK_OP_CLOSE, PK_STATUS_OK)
            } else {
                encode_pkg_status(PK_OP_CLOSE, PK_STATUS_BADF)
            }
        }
        op => encode_pkg_status(op, PK_STATUS_BAD_REQUEST),
    }
}

fn parse_pkg_path_request(frame: &[u8]) -> core::result::Result<String, u8> {
    if frame.len() < 6 {
        return Err(PK_STATUS_BAD_REQUEST);
    }
    let path_len = u16::from_le_bytes([frame[4], frame[5]]) as usize;
    if path_len == 0 || path_len > PK_MAX_PATH_LEN {
        return Err(PK_STATUS_OVERSIZED);
    }
    if frame.len() != 6 + path_len {
        return Err(PK_STATUS_BAD_REQUEST);
    }
    let path = core::str::from_utf8(&frame[6..]).map_err(|_| PK_STATUS_BAD_REQUEST)?;
    normalize_pkg_path(path)
}

fn normalize_pkg_path(path: &str) -> core::result::Result<String, u8> {
    let rel = if let Some(rest) = path.strip_prefix("pkg:/") {
        rest
    } else if let Some(rest) = path.strip_prefix("/packages/") {
        rest
    } else {
        return Err(PK_STATUS_NON_PACKAGEFS_SCHEME);
    };
    if rel.is_empty() || rel.len() > PK_MAX_PATH_LEN {
        return Err(PK_STATUS_OVERSIZED);
    }
    let mut normalized = String::new();
    let mut first = true;
    for seg in rel.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return Err(PK_STATUS_PATH_TRAVERSAL);
        }
        if seg.bytes().any(|b| b == b'\\' || b == 0) {
            return Err(PK_STATUS_PATH_TRAVERSAL);
        }
        if !first {
            normalized.push('/');
        }
        normalized.push_str(seg);
        first = false;
    }
    if normalized.is_empty() {
        return Err(PK_STATUS_PATH_TRAVERSAL);
    }
    Ok(normalized)
}

fn resolve_pkg_path(rel: &str) -> Option<Vec<u8>> {
    match rel {
        "system/build.prop" => Some(PACKAGEFS_BUILD_PROP.to_vec()),
        _ => None,
    }
}

fn allocate_pkg_handle(handles: &HashMap<u32, Vec<u8>>, next_handle: &mut u32) -> Option<u32> {
    let mut candidate = *next_handle;
    for _ in 0..(PK_MAX_HANDLES * 2) {
        if candidate == 0 {
            candidate = 1;
        }
        if !handles.contains_key(&candidate) {
            *next_handle = candidate.wrapping_add(1);
            return Some(candidate);
        }
        candidate = candidate.wrapping_add(1);
    }
    None
}

fn encode_pkg_status(op: u8, status: u8) -> Vec<u8> {
    vec![PK_MAGIC0, PK_MAGIC1, PK_VERSION, op | 0x80, status]
}

fn encode_pkg_stat(status: u8, size: u64, kind: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(15);
    out.extend_from_slice(&[PK_MAGIC0, PK_MAGIC1, PK_VERSION, PK_OP_STAT | 0x80, status]);
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out
}

fn encode_pkg_open(status: u8, handle: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(9);
    out.extend_from_slice(&[PK_MAGIC0, PK_MAGIC1, PK_VERSION, PK_OP_OPEN | 0x80, status]);
    out.extend_from_slice(&handle.to_le_bytes());
    out
}

fn encode_pkg_read(status: u8, data: &[u8]) -> Vec<u8> {
    let n = core::cmp::min(data.len(), PK_MAX_READ_LEN);
    let mut out = Vec::with_capacity(7 + n);
    out.extend_from_slice(&[PK_MAGIC0, PK_MAGIC1, PK_VERSION, PK_OP_READ | 0x80, status]);
    out.extend_from_slice(&(n as u16).to_le_bytes());
    out.extend_from_slice(&data[..n]);
    out
}

fn build_samgr_register(name: &str, endpoint: u32) -> Result<Vec<u8>> {
    let mut message = Builder::new_default();
    {
        let mut request = message.init_root::<register_request::Builder<'_>>();
        request.set_name(name);
        request.set_endpoint(endpoint);
    }
    let mut buf = Vec::new();
    serialize::write_message(&mut buf, &message).map_err(|err| anyhow!(err.to_string()))?;
    let mut frame = Vec::with_capacity(1 + buf.len());
    frame.push(OPCODE_REGISTER);
    frame.extend_from_slice(&buf);
    Ok(frame)
}

fn build_samgr_resolve(name: &str) -> Result<Vec<u8>> {
    let mut message = Builder::new_default();
    {
        let mut request = message.init_root::<resolve_request::Builder<'_>>();
        request.set_name(name);
    }
    let mut buf = Vec::new();
    serialize::write_message(&mut buf, &message).map_err(|err| anyhow!(err.to_string()))?;
    let mut frame = Vec::with_capacity(1 + buf.len());
    frame.push(OPCODE_RESOLVE);
    frame.extend_from_slice(&buf);
    Ok(frame)
}

fn parse_samgr_register(bytes: &[u8]) -> Result<bool> {
    eprintln!("[remote_e2e] parse_samgr_register bytes len={}", bytes.len());
    if bytes.is_empty() {
        return Err(anyhow!("empty register response"));
    }
    let mut cursor = std::io::Cursor::new(&bytes[1..]);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|err| anyhow!(err.to_string()))?;
    let response = message
        .get_root::<register_response::Reader<'_>>()
        .map_err(|err| anyhow!(err.to_string()))?;
    Ok(response.get_ok())
}

fn parse_samgr_resolve(bytes: &[u8]) -> Result<bool> {
    if bytes.is_empty() {
        return Err(anyhow!("empty resolve response"));
    }
    let mut cursor = std::io::Cursor::new(&bytes[1..]);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|err| anyhow!(err.to_string()))?;
    let response = message
        .get_root::<resolve_response::Reader<'_>>()
        .map_err(|err| anyhow!(err.to_string()))?;
    Ok(response.get_found())
}

fn build_bundle_install(name: &str, len: u32, handle: u32) -> Result<Vec<u8>> {
    let mut message = Builder::new_default();
    {
        let mut request = message.init_root::<install_request::Builder<'_>>();
        request.set_name(name);
        request.set_bytes_len(len);
        request.set_vmo_handle(handle);
    }
    let mut buf = Vec::new();
    serialize::write_message(&mut buf, &message).map_err(|err| anyhow!(err.to_string()))?;
    let mut frame = Vec::with_capacity(1 + buf.len());
    frame.push(OPCODE_INSTALL);
    frame.extend_from_slice(&buf);
    Ok(frame)
}

fn parse_bundle_install(bytes: &[u8]) -> Result<bool> {
    if bytes.is_empty() {
        return Err(anyhow!("empty install response"));
    }
    let mut cursor = std::io::Cursor::new(&bytes[1..]);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|err| anyhow!(err.to_string()))?;
    let response = message
        .get_root::<install_response::Reader<'_>>()
        .map_err(|err| anyhow!(err.to_string()))?;
    Ok(response.get_ok())
}

fn build_bundle_query(name: &str) -> Result<Vec<u8>> {
    let mut message = Builder::new_default();
    {
        let mut request = message.init_root::<query_request::Builder<'_>>();
        request.set_name(name);
    }
    let mut buf = Vec::new();
    serialize::write_message(&mut buf, &message).map_err(|err| anyhow!(err.to_string()))?;
    let mut frame = Vec::with_capacity(1 + buf.len());
    frame.push(OPCODE_QUERY);
    frame.extend_from_slice(&buf);
    Ok(frame)
}

fn parse_bundle_query(bytes: &[u8]) -> Result<Option<String>> {
    if bytes.is_empty() {
        return Err(anyhow!("empty query response"));
    }
    let mut cursor = std::io::Cursor::new(&bytes[1..]);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|err| anyhow!(err.to_string()))?;
    let response =
        message.get_root::<query_response::Reader<'_>>().map_err(|err| anyhow!(err.to_string()))?;
    if response.get_installed() {
        let caps = response.get_required_caps().map_err(|err| anyhow!(err.to_string()))?;
        for idx in 0..caps.len() {
            let _ = caps
                .get(idx)
                .map_err(|err| anyhow!(err.to_string()))?
                .to_str()
                .map_err(|err| anyhow!(err.to_string()))?;
        }
        let ver = response
            .get_version()
            .map_err(|err| anyhow!(err.to_string()))?
            .to_str()
            .map_err(|e| anyhow!(e.to_string()))?
            .to_string();
        Ok(Some(ver))
    } else {
        Ok(None)
    }
}

/// Represents an established remote connection over DSoftBus.
pub struct RemoteConnection {
    stream: Mutex<Box<dyn Stream + Send>>,
}

impl RemoteConnection {
    fn new(stream: Box<dyn Stream + Send>) -> Self {
        Self { stream: Mutex::new(stream) }
    }

    /// Resolves `service` on the remote node, returning whether it was found.
    pub fn resolve(&self, service: &str) -> Result<bool> {
        let request = build_samgr_resolve(service)?;
        let mut stream = self.stream.lock();
        eprintln!("[remote_e2e] client: resolve tx len={} service={}", request.len(), service);
        stream.send(CHAN_SAMGR, &request).map_err(|err| anyhow!(err.to_string()))?;
        let response = stream
            .recv()
            .map_err(|err| anyhow!(err.to_string()))?
            .ok_or_else(|| anyhow!("remote closed stream"))?;
        if response.channel != CHAN_SAMGR {
            return Err(anyhow!("unexpected channel {}", response.channel));
        }
        eprintln!("[remote_e2e] client: resolve rx len={}", response.bytes.len());
        parse_samgr_resolve(&response.bytes)
    }

    /// Uploads bundle bytes into the remote artifact store under `handle`.
    pub fn push_artifact(&self, handle: u32, kind: ArtifactKind, bytes: &[u8]) -> Result<()> {
        let mut payload = Vec::with_capacity(5 + bytes.len());
        payload.extend_from_slice(&handle.to_be_bytes());
        payload.push(kind.as_u8());
        payload.extend_from_slice(bytes);
        let mut stream = self.stream.lock();
        eprintln!(
            "[remote_e2e] client: artifact tx handle={} kind={} len={}",
            handle,
            kind.as_u8(),
            bytes.len()
        );
        stream.send(CHAN_ARTIFACT, &payload).map_err(|err| anyhow!(err.to_string()))?;
        let response = stream
            .recv()
            .map_err(|err| anyhow!(err.to_string()))?
            .ok_or_else(|| anyhow!("remote closed stream"))?;
        if response.channel != CHAN_ARTIFACT {
            return Err(anyhow!("artifact ack on unexpected channel"));
        }
        eprintln!("[remote_e2e] client: artifact ack len={}", response.bytes.len());
        Ok(())
    }

    /// Requests installation of `name` using the uploaded artifact handle.
    pub fn install_bundle(&self, name: &str, handle: u32, expected_len: u32) -> Result<bool> {
        let request = build_bundle_install(name, expected_len, handle)?;
        let mut stream = self.stream.lock();
        eprintln!(
            "[remote_e2e] client: install tx len={} name={} handle={} bytes={} ",
            request.len(),
            name,
            handle,
            expected_len
        );
        stream.send(CHAN_BUNDLEMGR, &request).map_err(|err| anyhow!(err.to_string()))?;
        let response = stream
            .recv()
            .map_err(|err| anyhow!(err.to_string()))?
            .ok_or_else(|| anyhow!("remote closed stream"))?;
        if response.channel != CHAN_BUNDLEMGR {
            return Err(anyhow!("install response on unexpected channel"));
        }
        eprintln!("[remote_e2e] client: install rx len={}", response.bytes.len());
        parse_bundle_install(&response.bytes)
    }

    /// Queries bundle metadata on the remote node.
    pub fn query_bundle(&self, name: &str) -> Result<Option<String>> {
        let request = build_bundle_query(name)?;
        let mut stream = self.stream.lock();
        eprintln!("[remote_e2e] client: query tx len={} name={}", request.len(), name);
        stream.send(CHAN_BUNDLEMGR, &request).map_err(|err| anyhow!(err.to_string()))?;
        let response = stream
            .recv()
            .map_err(|err| anyhow!(err.to_string()))?
            .ok_or_else(|| anyhow!("remote closed stream"))?;
        if response.channel != CHAN_BUNDLEMGR {
            return Err(anyhow!("query response on unexpected channel"));
        }
        eprintln!("[remote_e2e] client: query rx len={}", response.bytes.len());
        parse_bundle_query(&response.bytes)
    }

    /// Executes remote packagefs STAT and returns `(status, size, kind)`.
    pub fn remote_pkgfs_stat_status(&self, path: &str) -> Result<(u8, u64, u16)> {
        let request = build_pkgfs_path_req(PK_OP_STAT, path)?;
        let mut stream = self.stream.lock();
        stream.send(CHAN_PACKAGEFS, &request).map_err(|err| anyhow!(err.to_string()))?;
        let response = stream
            .recv()
            .map_err(|err| anyhow!(err.to_string()))?
            .ok_or_else(|| anyhow!("remote closed stream"))?;
        if response.channel != CHAN_PACKAGEFS {
            return Err(anyhow!("pkgfs stat response on unexpected channel"));
        }
        parse_pkgfs_stat_rsp(&response.bytes)
    }

    /// Executes remote packagefs OPEN and returns `(status, handle)`.
    pub fn remote_pkgfs_open_status(&self, path: &str) -> Result<(u8, u32)> {
        let request = build_pkgfs_path_req(PK_OP_OPEN, path)?;
        let mut stream = self.stream.lock();
        stream.send(CHAN_PACKAGEFS, &request).map_err(|err| anyhow!(err.to_string()))?;
        let response = stream
            .recv()
            .map_err(|err| anyhow!(err.to_string()))?
            .ok_or_else(|| anyhow!("remote closed stream"))?;
        if response.channel != CHAN_PACKAGEFS {
            return Err(anyhow!("pkgfs open response on unexpected channel"));
        }
        parse_pkgfs_open_rsp(&response.bytes)
    }

    /// Executes remote packagefs READ and returns `(status, bytes)`.
    pub fn remote_pkgfs_read_status(
        &self,
        handle: u32,
        offset: u32,
        read_len: u16,
    ) -> Result<(u8, Vec<u8>)> {
        let request = build_pkgfs_read_req(handle, offset, read_len);
        let mut stream = self.stream.lock();
        stream.send(CHAN_PACKAGEFS, &request).map_err(|err| anyhow!(err.to_string()))?;
        let response = stream
            .recv()
            .map_err(|err| anyhow!(err.to_string()))?
            .ok_or_else(|| anyhow!("remote closed stream"))?;
        if response.channel != CHAN_PACKAGEFS {
            return Err(anyhow!("pkgfs read response on unexpected channel"));
        }
        parse_pkgfs_read_rsp(&response.bytes)
    }

    /// Executes remote packagefs CLOSE and returns status.
    pub fn remote_pkgfs_close_status(&self, handle: u32) -> Result<u8> {
        let request = build_pkgfs_close_req(handle);
        let mut stream = self.stream.lock();
        stream.send(CHAN_PACKAGEFS, &request).map_err(|err| anyhow!(err.to_string()))?;
        let response = stream
            .recv()
            .map_err(|err| anyhow!(err.to_string()))?
            .ok_or_else(|| anyhow!("remote closed stream"))?;
        if response.channel != CHAN_PACKAGEFS {
            return Err(anyhow!("pkgfs close response on unexpected channel"));
        }
        parse_pkgfs_close_rsp(&response.bytes)
    }

    /// Convenience helper for `STAT -> OPEN -> READ -> CLOSE`.
    pub fn remote_pkgfs_read_once(&self, path: &str, max_len: u16) -> Result<Vec<u8>> {
        let (stat_st, _size, kind) = self.remote_pkgfs_stat_status(path)?;
        if stat_st != PK_STATUS_OK || kind != PACKAGEFS_KIND_FILE {
            return Err(anyhow!("remote pkgfs stat failed status={stat_st} kind={kind}"));
        }
        let (open_st, handle) = self.remote_pkgfs_open_status(path)?;
        if open_st != PK_STATUS_OK {
            return Err(anyhow!("remote pkgfs open failed status={open_st}"));
        }
        let (read_st, bytes) = self.remote_pkgfs_read_status(handle, 0, max_len)?;
        if read_st != PK_STATUS_OK {
            return Err(anyhow!("remote pkgfs read failed status={read_st}"));
        }
        let close_st = self.remote_pkgfs_close_status(handle)?;
        if close_st != PK_STATUS_OK {
            return Err(anyhow!("remote pkgfs close failed status={close_st}"));
        }
        Ok(bytes)
    }
}

fn build_pkgfs_path_req(op: u8, path: &str) -> Result<Vec<u8>> {
    let bytes = path.as_bytes();
    let path_len = u16::try_from(bytes.len()).map_err(|_| anyhow!("path too long"))?;
    let mut out = Vec::with_capacity(6 + bytes.len());
    out.extend_from_slice(&[PK_MAGIC0, PK_MAGIC1, PK_VERSION, op]);
    out.extend_from_slice(&path_len.to_le_bytes());
    out.extend_from_slice(bytes);
    Ok(out)
}

fn build_pkgfs_read_req(handle: u32, offset: u32, read_len: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(14);
    out.extend_from_slice(&[PK_MAGIC0, PK_MAGIC1, PK_VERSION, PK_OP_READ]);
    out.extend_from_slice(&handle.to_le_bytes());
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(&read_len.to_le_bytes());
    out
}

fn build_pkgfs_close_req(handle: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    out.extend_from_slice(&[PK_MAGIC0, PK_MAGIC1, PK_VERSION, PK_OP_CLOSE]);
    out.extend_from_slice(&handle.to_le_bytes());
    out
}

fn parse_pkgfs_stat_rsp(bytes: &[u8]) -> Result<(u8, u64, u16)> {
    if bytes.len() < 15 {
        return Err(anyhow!("pkgfs stat rsp too short"));
    }
    if bytes[0] != PK_MAGIC0
        || bytes[1] != PK_MAGIC1
        || bytes[2] != PK_VERSION
        || bytes[3] != (PK_OP_STAT | 0x80)
    {
        return Err(anyhow!("pkgfs stat rsp header mismatch"));
    }
    let status = bytes[4];
    let size = u64::from_le_bytes([
        bytes[5], bytes[6], bytes[7], bytes[8], bytes[9], bytes[10], bytes[11], bytes[12],
    ]);
    let kind = u16::from_le_bytes([bytes[13], bytes[14]]);
    Ok((status, size, kind))
}

fn parse_pkgfs_open_rsp(bytes: &[u8]) -> Result<(u8, u32)> {
    if bytes.len() < 9 {
        return Err(anyhow!("pkgfs open rsp too short"));
    }
    if bytes[0] != PK_MAGIC0
        || bytes[1] != PK_MAGIC1
        || bytes[2] != PK_VERSION
        || bytes[3] != (PK_OP_OPEN | 0x80)
    {
        return Err(anyhow!("pkgfs open rsp header mismatch"));
    }
    let status = bytes[4];
    let handle = u32::from_le_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]);
    Ok((status, handle))
}

fn parse_pkgfs_read_rsp(bytes: &[u8]) -> Result<(u8, Vec<u8>)> {
    if bytes.len() < 7 {
        return Err(anyhow!("pkgfs read rsp too short"));
    }
    if bytes[0] != PK_MAGIC0
        || bytes[1] != PK_MAGIC1
        || bytes[2] != PK_VERSION
        || bytes[3] != (PK_OP_READ | 0x80)
    {
        return Err(anyhow!("pkgfs read rsp header mismatch"));
    }
    let status = bytes[4];
    let n = u16::from_le_bytes([bytes[5], bytes[6]]) as usize;
    if bytes.len() < 7 + n {
        return Err(anyhow!("pkgfs read rsp len mismatch"));
    }
    Ok((status, bytes[7..7 + n].to_vec()))
}

fn parse_pkgfs_close_rsp(bytes: &[u8]) -> Result<u8> {
    if bytes.len() < 5 {
        return Err(anyhow!("pkgfs close rsp too short"));
    }
    if bytes[0] != PK_MAGIC0
        || bytes[1] != PK_MAGIC1
        || bytes[2] != PK_VERSION
        || bytes[3] != (PK_OP_CLOSE | 0x80)
    {
        return Err(anyhow!("pkgfs close rsp header mismatch"));
    }
    Ok(bytes[4])
}

/// Generates a random high port in the dynamic range for host tests.
pub fn random_port() -> u16 {
    const BASE: u16 = 30_000;
    const RANGE: u16 = 10_000;
    let mut rng = rand::rng();
    BASE + rng.random_range(0..RANGE)
}
