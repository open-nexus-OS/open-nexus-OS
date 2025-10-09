//! Host-only remote end-to-end harness exercising the DSoftBus-lite stack.
//!
//! The helpers defined here spin up a pair of service nodes (identityd,
//! samgrd, bundlemgrd, and dsoftbusd equivalents) entirely in-process. The
//! nodes communicate using the `userspace/dsoftbus` host backend and forward
//! Cap'n Proto frames to the existing daemons, providing a realistic control
//! plane without booting QEMU.

#![forbid(unsafe_code)]

use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use capnp::message::{Builder, ReaderOptions};
use capnp::serialize;
use dsoftbus::{Announcement, Authenticator, FramePayload, HostAuthenticator, HostDiscovery, HostStream};
use identity::{DeviceId, Identity};
use nexus_idl_runtime::bundlemgr_capnp::{install_request, install_response, query_request, query_response};
use nexus_idl_runtime::samgr_capnp::{register_request, register_response, resolve_request, resolve_response};
use nexus_ipc::{LoopbackClient, Wait};
use parking_lot::Mutex;
use rand::Rng;
use samgr::Registry;
use samgrd::serve_with_registry as samgr_serve_with_registry;
use bundlemgrd::{self, run_with_transport as bundle_run_with_transport, ArtifactStore};
use thiserror::Error;

const CHAN_SAMGR: u32 = 1;
const CHAN_BUNDLEMGR: u32 = 2;
const CHAN_ARTIFACT: u32 = 3;

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

/// Represents a running host node exposing DSoftBus-lite services.
pub struct Node {
    authenticator: Arc<HostAuthenticator>,
    discovery: HostDiscovery,
    announcement: Announcement,
    samgr_client: Arc<LoopbackClient>,
    bundle_client: Arc<LoopbackClient>,
    artifact_store: ArtifactStore,
    accept_thread: JoinHandle<()>,
    samgr_thread: JoinHandle<()>,
    bundle_thread: JoinHandle<()>,
    shutdown: Arc<AtomicBool>,
    listen_addr: SocketAddr,
}

impl Node {
    /// Boots a node using randomly generated identity material and binds the
    /// DSoftBus authenticator to `listen_port`.
    pub fn start(listen_port: u16, services: Vec<String>) -> Result<Self> {
        let identity = Identity::generate().context("generate identity")?;
        let listen_addr = SocketAddr::from(([127, 0, 0, 1], listen_port));
        let authenticator = HostAuthenticator::bind(listen_addr, identity.clone())
            .context("bind host authenticator")?;
        let discovery = HostDiscovery::new();
        let announcement = Announcement::new(
            identity.device_id().clone(),
            services,
            listen_port,
            authenticator.local_noise_public(),
        );
        discovery
            .announce(announcement.clone())
            .context("announce local node")?;

        // samgrd loopback transport and server thread
        let (samgr_client, mut samgr_server) = samgrd::loopback_transport();
        let registry = Registry::new();
        let samgr_thread = thread::spawn(move || {
            let mut transport = samgr_server;
            if let Err(err) = samgr_serve_with_registry(&mut transport, registry) {
                eprintln!("samgrd loop terminated: {err}");
            }
        });
        let samgr_client = Arc::new(samgr_client);

        // bundlemgrd loopback transport and server thread
        let (bundle_client, mut bundle_server) = bundlemgrd::loopback_transport();
        let artifacts = ArtifactStore::new();
        let artifact_clone = artifacts.clone();
        let bundle_thread = thread::spawn(move || {
            let mut transport = bundle_server;
            if let Err(err) = bundle_run_with_transport(&mut transport, artifact_clone) {
                eprintln!("bundlemgrd loop terminated: {err}");
            }
        });
        let bundle_client = Arc::new(bundle_client);

        let shutdown = Arc::new(AtomicBool::new(false));
        let acceptor = authenticator.clone();
        let samgr_bridge = Arc::clone(&samgr_client);
        let bundle_bridge = Arc::clone(&bundle_client);
        let store_bridge = artifacts.clone();
        let stop_flag = Arc::clone(&shutdown);
        let accept_thread = thread::spawn(move || {
            while !stop_flag.load(Ordering::SeqCst) {
                match acceptor.accept() {
                    Ok(session) => {
                        if let Ok(stream) = session.into_stream() {
                            let samgr_client = Arc::clone(&samgr_bridge);
                            let bundle_client = Arc::clone(&bundle_bridge);
                            let store = store_bridge.clone();
                            thread::spawn(move || {
                                if let Err(err) = handle_session(stream, samgr_client, bundle_client, store) {
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
            authenticator: Arc::new(authenticator),
            discovery,
            announcement,
            samgr_client,
            bundle_client,
            artifact_store: artifacts,
            accept_thread,
            samgr_thread,
            bundle_thread,
            shutdown,
            listen_addr,
        })
    }

    /// Returns the device identifier assigned to this node.
    pub fn device_id(&self) -> DeviceId {
        self.authenticator.identity().device_id().clone()
    }

    /// Returns a clone of the local announcement payload.
    pub fn announcement(&self) -> Announcement {
        self.announcement.clone()
    }

    /// Returns a discovery iterator seeded with the current registry state.
    pub fn watch(&self) -> Result<impl Iterator<Item = Announcement>> {
        self.discovery.watch().map_err(|err| anyhow!(err.to_string()))
    }

    /// Attempts to retrieve an announcement for `device` from the registry.
    pub fn get_announcement(&self, device: &DeviceId) -> Result<Option<Announcement>> {
        self.discovery
            .get(device)
            .map_err(|err| anyhow!(err.to_string()))
    }

    /// Registers a local service name with the SAMGR daemon.
    pub fn register_service(&self, name: &str, endpoint: u32) -> Result<()> {
        let frame = build_samgr_register(name, endpoint)?;
        let response = forward_ipc(&self.samgr_client, frame)
            .map_err(|err| anyhow!(err.to_string()))?;
        if !parse_samgr_register(&response)? {
            return Err(anyhow!("samgr register rejected"));
        }
        Ok(())
    }

    /// Connects to `peer` and returns a handle used for remote operations.
    pub fn connect(&self, peer: &Announcement) -> Result<RemoteConnection> {
        let session = self
            .authenticator
            .connect(peer)
            .context("connect to remote peer")?;
        let stream = session.into_stream().context("stream negotiation")?;
        Ok(RemoteConnection::new(stream))
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // Wake the blocking accept call by connecting once to the listener.
        let _ = TcpStream::connect(self.listen_addr);
        let _ = self.accept_thread.join();
        let _ = self.samgr_thread.join();
        let _ = self.bundle_thread.join();
    }
}

fn handle_session(
    mut stream: HostStream,
    samgr: Arc<LoopbackClient>,
    bundle: Arc<LoopbackClient>,
    artifacts: ArtifactStore,
) -> Result<(), HarnessError> {
    loop {
        match stream.recv() {
            Ok(Some(FramePayload { channel, bytes })) => match channel {
                CHAN_SAMGR => {
                    let response = forward_ipc(&samgr, bytes).map_err(|err| HarnessError::Forward(err.to_string()))?;
                    stream
                        .send(CHAN_SAMGR, &response)
                        .map_err(|err| HarnessError::Forward(err.to_string()))?;
                }
                CHAN_BUNDLEMGR => {
                    let response = forward_ipc(&bundle, bytes).map_err(|err| HarnessError::Forward(err.to_string()))?;
                    stream
                        .send(CHAN_BUNDLEMGR, &response)
                        .map_err(|err| HarnessError::Forward(err.to_string()))?;
                }
                CHAN_ARTIFACT => {
                    if bytes.len() < 4 {
                        return Err(HarnessError::Protocol("artifact frame too small".into()));
                    }
                    let handle = u32::from_be_bytes(bytes[0..4].try_into().expect("slice length checked"));
                    let payload = bytes[4..].to_vec();
                    artifacts.insert(handle, payload);
                    stream
                        .send(CHAN_ARTIFACT, &[])
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
    client
        .send(&frame, Wait::Blocking)
        .map_err(|err| HarnessError::Forward(err.to_string()))?;
    client
        .recv(Wait::Blocking)
        .map_err(|err| HarnessError::Forward(err.to_string()))
}

fn build_samgr_register(name: &str, endpoint: u32) -> Result<Vec<u8>> {
    let mut message = Builder::new_default();
    {
        let mut request = message.init_root::<register_request::Builder<'_>>();
        request.set_name(name);
        request.set_endpoint(endpoint);
    }
    let words = serialize::write_message_to_words(&message).map_err(|err| anyhow!(err.to_string()))?;
    let mut frame = Vec::with_capacity(1 + words.len() * 8);
    frame.push(OPCODE_REGISTER);
    frame.extend_from_slice(capnp::Word::words_to_bytes(&words));
    Ok(frame)
}

fn build_samgr_resolve(name: &str) -> Result<Vec<u8>> {
    let mut message = Builder::new_default();
    {
        let mut request = message.init_root::<resolve_request::Builder<'_>>();
        request.set_name(name);
    }
    let words = serialize::write_message_to_words(&message).map_err(|err| anyhow!(err.to_string()))?;
    let mut frame = Vec::with_capacity(1 + words.len() * 8);
    frame.push(OPCODE_RESOLVE);
    frame.extend_from_slice(capnp::Word::words_to_bytes(&words));
    Ok(frame)
}

fn parse_samgr_register(bytes: &[u8]) -> Result<bool> {
    let mut cursor = std::io::Cursor::new(bytes);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new()).map_err(|err| anyhow!(err.to_string()))?;
    let response = message
        .get_root::<register_response::Reader<'_>>()
        .map_err(|err| anyhow!(err.to_string()))?;
    Ok(response.get_ok())
}

fn parse_samgr_resolve(bytes: &[u8]) -> Result<bool> {
    let mut cursor = std::io::Cursor::new(bytes);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new()).map_err(|err| anyhow!(err.to_string()))?;
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
    let words = serialize::write_message_to_words(&message).map_err(|err| anyhow!(err.to_string()))?;
    let mut frame = Vec::with_capacity(1 + words.len() * 8);
    frame.push(OPCODE_INSTALL);
    frame.extend_from_slice(capnp::Word::words_to_bytes(&words));
    Ok(frame)
}

fn parse_bundle_install(bytes: &[u8]) -> Result<bool> {
    let mut cursor = std::io::Cursor::new(bytes);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new()).map_err(|err| anyhow!(err.to_string()))?;
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
    let words = serialize::write_message_to_words(&message).map_err(|err| anyhow!(err.to_string()))?;
    let mut frame = Vec::with_capacity(1 + words.len() * 8);
    frame.push(OPCODE_QUERY);
    frame.extend_from_slice(capnp::Word::words_to_bytes(&words));
    Ok(frame)
}

fn parse_bundle_query(bytes: &[u8]) -> Result<Option<String>> {
    let mut cursor = std::io::Cursor::new(bytes);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new()).map_err(|err| anyhow!(err.to_string()))?;
    let response = message
        .get_root::<query_response::Reader<'_>>()
        .map_err(|err| anyhow!(err.to_string()))?;
    if response.get_installed() {
        Ok(Some(
            response
                .get_version()
                .map_err(|err| anyhow!(err.to_string()))?
                .to_string(),
        ))
    } else {
        Ok(None)
    }
}

/// Represents an established remote connection over DSoftBus.
pub struct RemoteConnection {
    stream: Mutex<HostStream>,
}

impl RemoteConnection {
    fn new(stream: HostStream) -> Self {
        Self {
            stream: Mutex::new(stream),
        }
    }

    /// Resolves `service` on the remote node, returning whether it was found.
    pub fn resolve(&self, service: &str) -> Result<bool> {
        let request = build_samgr_resolve(service)?;
        let mut stream = self.stream.lock();
        stream
            .send(CHAN_SAMGR, &request)
            .map_err(|err| anyhow!(err.to_string()))?;
        let response = stream
            .recv()
            .map_err(|err| anyhow!(err.to_string()))?
            .ok_or_else(|| anyhow!("remote closed stream"))?;
        if response.channel != CHAN_SAMGR {
            return Err(anyhow!("unexpected channel {}", response.channel));
        }
        parse_samgr_resolve(&response.bytes)
    }

    /// Uploads bundle bytes into the remote artifact store under `handle`.
    pub fn push_artifact(&self, handle: u32, bytes: &[u8]) -> Result<()> {
        let mut payload = Vec::with_capacity(4 + bytes.len());
        payload.extend_from_slice(&handle.to_be_bytes());
        payload.extend_from_slice(bytes);
        let mut stream = self.stream.lock();
        stream
            .send(CHAN_ARTIFACT, &payload)
            .map_err(|err| anyhow!(err.to_string()))?;
        let response = stream
            .recv()
            .map_err(|err| anyhow!(err.to_string()))?
            .ok_or_else(|| anyhow!("remote closed stream"))?;
        if response.channel != CHAN_ARTIFACT {
            return Err(anyhow!("artifact ack on unexpected channel"));
        }
        Ok(())
    }

    /// Requests installation of `name` using the uploaded artifact handle.
    pub fn install_bundle(&self, name: &str, handle: u32, expected_len: u32) -> Result<bool> {
        let request = build_bundle_install(name, expected_len, handle)?;
        let mut stream = self.stream.lock();
        stream
            .send(CHAN_BUNDLEMGR, &request)
            .map_err(|err| anyhow!(err.to_string()))?;
        let response = stream
            .recv()
            .map_err(|err| anyhow!(err.to_string()))?
            .ok_or_else(|| anyhow!("remote closed stream"))?;
        if response.channel != CHAN_BUNDLEMGR {
            return Err(anyhow!("install response on unexpected channel"));
        }
        parse_bundle_install(&response.bytes)
    }

    /// Queries bundle metadata on the remote node.
    pub fn query_bundle(&self, name: &str) -> Result<Option<String>> {
        let request = build_bundle_query(name)?;
        let mut stream = self.stream.lock();
        stream
            .send(CHAN_BUNDLEMGR, &request)
            .map_err(|err| anyhow!(err.to_string()))?;
        let response = stream
            .recv()
            .map_err(|err| anyhow!(err.to_string()))?
            .ok_or_else(|| anyhow!("remote closed stream"))?;
        if response.channel != CHAN_BUNDLEMGR {
            return Err(anyhow!("query response on unexpected channel"));
        }
        parse_bundle_query(&response.bytes)
    }
}

/// Generates a random high port in the dynamic range for host tests.
pub fn random_port() -> u16 {
    const BASE: u16 = 30_000;
    const RANGE: u16 = 10_000;
    let mut rng = rand::thread_rng();
    BASE + rng.gen_range(0..RANGE)
}
