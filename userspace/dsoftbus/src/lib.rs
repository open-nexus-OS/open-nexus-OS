// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: DSoftBus-lite distributed service fabric
//! OWNERS: @runtime
//! STATUS: Functional (host TCP + host QUIC runtime selection), Placeholder (OS backend - pending kernel transport)
//! API_STABILITY: Stable
//! TEST_COVERAGE: integration tests for host/facade transport, discovery robustness, QUIC host/selection contracts (`quic_host_transport_contract`, `quic_selection_contract`), mux v2 requirement suites (`mux_contract_rejects_and_bounds`, `mux_frame_state_keepalive_contract`, `mux_open_accept_data_rst_integration`), and no_std-core reject contracts (`core_contract_rejects`)
//!
//! PUBLIC API:
//!   - Announcement: Service discovery announcement
//!   - Discovery trait: Service discovery interface
//!   - Authenticator trait: Session authentication
//!   - Session/Stream traits: Communication channels
//!
//! DEPENDENCIES:
//!   - identity: Device identity and signing
//!   - curve25519-dalek: Noise key derivation
//!   - ed25519-dalek: Digital signatures
//!   - serde: Message serialization
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#![forbid(unsafe_code)]

#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

// Host is the default environment when no explicit nexus_env cfg is provided.

use std::net::SocketAddr;

use capnp::message::{Builder, ReaderOptions};
use capnp::serialize;
use curve25519_dalek::montgomery::MontgomeryPoint;
use ed25519_dalek::{Signature, VerifyingKey};
use identity::{DeviceId, Identity};
use nexus_idl_runtime::dsoftbus_capnp::{connect_request, connect_response};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Discovery data broadcast by each node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Announcement {
    device_id: DeviceId,
    services: Vec<String>,
    port: u16,
    noise_static: [u8; 32],
}

impl Announcement {
    /// Creates a new announcement for the provided device.
    pub fn new(
        device_id: DeviceId,
        services: Vec<String>,
        port: u16,
        noise_static: [u8; 32],
    ) -> Self {
        Self { device_id, services, port, noise_static }
    }

    /// Returns the announced device id.
    pub fn device_id(&self) -> &DeviceId {
        &self.device_id
    }

    /// Returns the list of published services.
    pub fn services(&self) -> &[String] {
        &self.services
    }

    /// Returns the listening port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Returns the static Noise public key advertised by the node.
    pub fn noise_static(&self) -> &[u8; 32] {
        &self.noise_static
    }
}

/// Announcement payload distributed during handshake authentication.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct HandshakeProof {
    device_id: String,
    verifying_key: Vec<u8>,
    signature: Vec<u8>,
}

impl HandshakeProof {
    fn new(device_id: &DeviceId, verifying_key: &VerifyingKey, signature: &Signature) -> Self {
        Self {
            device_id: device_id.as_str().to_string(),
            verifying_key: verifying_key.to_bytes().to_vec(),
            signature: signature.to_bytes().to_vec(),
        }
    }
}

/// Errors surfaced by discovery backends.
#[derive(Debug, Error)]
pub enum DiscoveryError {
    /// Backend does not support the requested operation in this build.
    #[error("discovery backend unsupported in this environment")]
    Unsupported,
    /// Underlying I/O failure.
    #[error("discovery io error: {0}")]
    Io(#[from] std::io::Error),
    /// Internal registry failure.
    #[error("discovery registry error: {0}")]
    Registry(String),
}

/// Errors produced by the authenticator implementation.
#[derive(Debug, Error)]
pub enum AuthError {
    /// Transport failure during handshake.
    #[error("authenticator io error: {0}")]
    Io(#[from] std::io::Error),
    /// Noise protocol failure.
    #[error("noise handshake failure: {0}")]
    Noise(String),
    /// Identity validation failed.
    #[error("identity validation failed: {0}")]
    Identity(String),
    /// Message parsing failure.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// Feature unavailable for the current build.
    #[error("authenticator unsupported in this environment")]
    Unsupported,
}

/// Errors when materialising a reliable stream from a session.
#[derive(Debug, Error)]
pub enum SessionError {
    /// Underlying transport failure.
    #[error("session io error: {0}")]
    Io(#[from] std::io::Error),
    /// Peer rejected the connection.
    #[error("session rejected: {0}")]
    Rejected(String),
}

/// Errors emitted by reliable stream operations.
#[derive(Debug, Error)]
pub enum StreamError {
    /// I/O failure.
    #[error("stream io error: {0}")]
    Io(#[from] std::io::Error),
    /// Cryptographic failure while encrypting or decrypting frames.
    #[error("stream crypto error: {0}")]
    Crypto(String),
    /// Frame parsing failure.
    #[error("stream protocol error: {0}")]
    Protocol(String),
}

/// Discovery implementations announce the local node and surface peers.
pub trait Discovery {
    type Error;
    type Stream: Iterator<Item = Announcement> + Send + 'static;

    fn announce(&self, announcement: Announcement) -> Result<(), Self::Error>;
    fn get(&self, device: &DeviceId) -> Result<Option<Announcement>, Self::Error>;
    fn watch(&self) -> Result<Self::Stream, Self::Error>;
}

/// Authenticator establishes authenticated sessions with remote peers.
pub trait Authenticator {
    type Session: Session;

    fn bind(addr: SocketAddr, identity: Identity) -> Result<Self, AuthError>
    where
        Self: Sized;

    fn accept(&self) -> Result<Self::Session, AuthError>;
    fn connect(&self, announcement: &Announcement) -> Result<Self::Session, AuthError>;
}

/// A negotiated session bound to a specific remote device.
pub trait Session {
    type Stream: Stream;

    fn remote_device_id(&self) -> &DeviceId;
    fn into_stream(self) -> Result<Self::Stream, SessionError>;
}

/// Reliable framed stream capable of multiplexing logical channels.
pub trait Stream {
    fn send(&mut self, channel: u32, payload: &[u8]) -> Result<(), StreamError>;
    fn recv(&mut self) -> Result<Option<FramePayload>, StreamError>;
}

/// A frame received from a remote peer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FramePayload {
    pub channel: u32,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy)]
enum HandshakeRole {
    Server,
    Client,
}

impl HandshakeRole {
    fn tag(self) -> &'static [u8] {
        match self {
            HandshakeRole::Server => b"server-static",
            HandshakeRole::Client => b"client-static",
        }
    }
}

fn proof_message(role: HandshakeRole, noise_static: &[u8; 32]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(role.tag());
    hasher.update(noise_static);
    hasher.finalize().to_vec()
}

fn derive_noise_keys(identity: &Identity) -> ([u8; 32], [u8; 32]) {
    let secret = identity.secret_key_bytes();
    let hash = Sha256::digest(secret);
    let mut private = [0u8; 32];
    private.copy_from_slice(&hash);
    // Clamp once to ensure deterministic X25519-compatible static key material.
    private[0] &= 248;
    private[31] &= 127;
    private[31] |= 64;
    let public = MontgomeryPoint::mul_base_clamped(private).to_bytes();
    (private, public)
}

fn deserialize_connect_request_plain(bytes: &[u8]) -> Result<String, String> {
    let mut cursor = std::io::Cursor::new(bytes);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|err| err.to_string())?;
    let reader: connect_request::Reader<'_> = message.get_root().map_err(|err| err.to_string())?;
    let txt = reader.get_device_id().map_err(|err| err.to_string())?;
    txt.to_str().map_err(|err| err.to_string()).map(str::to_string)
}

fn serialize_connect_response_plain(ok: bool) -> Result<Vec<u8>, String> {
    let mut message = Builder::new_default();
    {
        let mut response: connect_response::Builder<'_> = message.init_root();
        response.set_ok(ok);
    }
    let mut out = Vec::new();
    serialize::write_message(&mut out, &message).map_err(|err| err.to_string())?;
    Ok(out)
}

#[cfg(any(nexus_env = "host", not(nexus_env = "os")))]
mod host;

#[cfg(any(nexus_env = "host", not(nexus_env = "os")))]
pub use host::{HostAuthenticator, HostDiscovery, HostSession, HostStream};

#[cfg(any(nexus_env = "host", not(nexus_env = "os")))]
mod inproc;

#[cfg(any(nexus_env = "host", not(nexus_env = "os")))]
pub use inproc::{InProcAuthenticator, InProcSession, InProcStream};

#[cfg(any(nexus_env = "host", not(nexus_env = "os")))]
mod net_facade;

#[cfg(any(nexus_env = "host", not(nexus_env = "os")))]
pub use net_facade::{FacadeAuthenticator, FacadeSession, FacadeStream};

#[cfg(any(nexus_env = "host", not(nexus_env = "os")))]
mod facade_discovery;

#[cfg(any(nexus_env = "host", not(nexus_env = "os")))]
pub use facade_discovery::{FacadeAnnouncementStream, FacadeDiscovery};

pub mod discovery_packet;

pub mod remote_proxy_policy;

pub mod mux_v2 {
    pub use dsoftbus_core::mux_v2::*;
}
pub use dsoftbus_core::mux_v2::*;

pub mod core_contract {
    pub use dsoftbus_core::core_contract::*;
}
pub use dsoftbus_core::core_contract::{
    validate_payload_identity_spoof_vs_sender_service_id, validate_record_bounds,
    BorrowedFrameTransport, CoreReject, CorrelationNonce, CorrelationWindow, OwnedRecord,
    PayloadIdentityClaim, SenderServiceId, REJECT_INVALID_STATE_TRANSITION,
    REJECT_NONCE_MISMATCH_OR_STALE_REPLY, REJECT_OVERSIZE_FRAME_OR_RECORD,
    REJECT_PAYLOAD_IDENTITY_SPOOF_VS_SENDER_SERVICE_ID, REJECT_UNAUTHENTICATED_MESSAGE_PATH,
};

pub mod transport_selection;
pub use transport_selection::{
    fallback_marker_budget, quic_attempts_for_mode, select_transport, QuicProbe, TransportKind,
    TransportMode, TransportSelectionError, TransportSelectionOutcome, AUTO_FALLBACK_MARKER_COUNT,
    MARKER_QUIC_OS_DISABLED_FALLBACK_TCP, MARKER_SELFTEST_QUIC_FALLBACK_OK,
    MARKER_TRANSPORT_SELECTED_QUIC, MARKER_TRANSPORT_SELECTED_TCP,
};

#[cfg(any(nexus_env = "host", not(nexus_env = "os")))]
pub mod host_quic;

#[cfg(any(nexus_env = "host", not(nexus_env = "os")))]
pub use host_quic::{
    build_server_config, probe_and_echo_once, select_transport_with_host_quic, HostQuicProbeError,
    HostQuicProbeRequest, HostQuicProbeResult, DSOFTBUS_QUIC_DEFAULT_ALPN,
};

#[cfg(nexus_env = "os")]
mod os;

#[cfg(nexus_env = "os")]
pub use os::{OsAuthenticator, OsDiscovery, OsSession, OsStream};

/// Starts the DSoftBus-lite daemon loop.
///
/// Host builds bind a TCP listener, announce the local node via the in-process
/// registry, then accept authenticated sessions and drain their streams. The
/// OS backend is a placeholder until the kernel transport is available.
pub fn run() {
    #[cfg(any(nexus_env = "host", not(nexus_env = "os")))]
    host_run();

    #[cfg(nexus_env = "os")]
    os_run();
}

#[cfg(any(nexus_env = "host", not(nexus_env = "os")))]
fn host_transport_mode_from_env() -> TransportMode {
    let raw = std::env::var("DSOFTBUS_TRANSPORT").unwrap_or_else(|_| "tcp".to_string());
    match raw.to_ascii_lowercase().as_str() {
        "tcp" => TransportMode::Tcp,
        "quic" => TransportMode::Quic,
        "auto" => TransportMode::Auto,
        other => panic!("invalid DSOFTBUS_TRANSPORT='{other}'; expected tcp|quic|auto"),
    }
}

#[cfg(any(nexus_env = "host", not(nexus_env = "os")))]
fn dsoftbus_port_from_env() -> u16 {
    std::env::var("DSOFTBUS_PORT").ok().and_then(|raw| raw.parse::<u16>().ok()).unwrap_or(34_567)
}

#[cfg(any(nexus_env = "host", not(nexus_env = "os")))]
struct HostQuicRuntimeConfig {
    cert_chain: Vec<rustls::pki_types::CertificateDer<'static>>,
    private_key: rustls::pki_types::PrivateKeyDer<'static>,
}

#[cfg(any(nexus_env = "host", not(nexus_env = "os")))]
fn load_host_quic_runtime_config_from_env() -> Result<HostQuicRuntimeConfig, String> {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

    let cert_path = std::env::var("DSOFTBUS_QUIC_SERVER_CERT_DER_PATH")
        .map_err(|_| "DSOFTBUS_QUIC_SERVER_CERT_DER_PATH missing".to_string())?;
    let key_path = std::env::var("DSOFTBUS_QUIC_SERVER_KEY_DER_PATH")
        .map_err(|_| "DSOFTBUS_QUIC_SERVER_KEY_DER_PATH missing".to_string())?;

    let cert_der = std::fs::read(&cert_path)
        .map_err(|err| format!("read DSOFTBUS_QUIC_SERVER_CERT_DER_PATH failed: {err}"))?;
    let key_der = std::fs::read(&key_path)
        .map_err(|err| format!("read DSOFTBUS_QUIC_SERVER_KEY_DER_PATH failed: {err}"))?;
    let cert_chain = vec![CertificateDer::from(cert_der)];
    let private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der));

    Ok(HostQuicRuntimeConfig { cert_chain, private_key })
}

#[cfg(any(nexus_env = "host", not(nexus_env = "os")))]
fn resolve_host_transport_selection(
    mode: TransportMode,
    quic_runtime_available: bool,
) -> Result<TransportSelectionOutcome, TransportSelectionError> {
    let probe = if quic_runtime_available {
        QuicProbe::Candidate {
            expected_alpn: DSOFTBUS_QUIC_DEFAULT_ALPN,
            offered_alpn: DSOFTBUS_QUIC_DEFAULT_ALPN,
            cert_trusted: true,
        }
    } else {
        QuicProbe::Disabled
    };
    select_transport(mode, probe)
}

#[cfg(any(nexus_env = "host", not(nexus_env = "os")))]
fn host_run_quic(identity: Identity, port: u16, quic_runtime: HostQuicRuntimeConfig) {
    const MAX_AUTH_BYTES: usize = 8 * 1024;
    const MAX_HOST_QUIC_STREAM_BYTES: usize = 64 * 1024;

    let server_config = match build_server_config(
        quic_runtime.cert_chain,
        quic_runtime.private_key,
        DSOFTBUS_QUIC_DEFAULT_ALPN,
    ) {
        Ok(config) => config,
        Err(err) => panic!("build host quic server config failed: {err}"),
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_io()
        .enable_time()
        .build()
    {
        Ok(rt) => rt,
        Err(err) => panic!("build host quic runtime failed: {err}"),
    };

    runtime.block_on(async move {
        let endpoint = match quinn::Endpoint::server(
            server_config,
            SocketAddr::from(([127, 0, 0, 1], port)),
        ) {
            Ok(endpoint) => endpoint,
            Err(err) => panic!("bind host quic endpoint failed: {err}"),
        };
        let local_port = match endpoint.local_addr() {
            Ok(addr) => addr.port(),
            Err(err) => panic!("host quic endpoint local_addr failed: {err}"),
        };

        let discovery = HostDiscovery::new();
        let services = vec!["samgrd".to_string(), "bundlemgrd".to_string()];
        let (_, noise_public) = derive_noise_keys(&identity);
        let announcement =
            Announcement::new(identity.device_id().clone(), services, local_port, noise_public);
        if let Err(err) = discovery.announce(announcement) {
            panic!("announce local node (quic): {err}");
        }

        println!("{}", MARKER_TRANSPORT_SELECTED_QUIC);
        println!("dsoftbusd: ready");

        while let Some(incoming) = endpoint.accept().await {
            tokio::spawn(async move {
                let connection = match incoming.await {
                    Ok(connection) => connection,
                    Err(err) => {
                        eprintln!("[dsoftbus] quic accept failed: {err}");
                        return;
                    }
                };
                let mut session_authenticated = false;
                loop {
                    let (mut send, mut recv) = match connection.accept_bi().await {
                        Ok(streams) => streams,
                        Err(_) => break,
                    };

                    if !session_authenticated {
                        let auth_request = match recv.read_to_end(MAX_AUTH_BYTES).await {
                            Ok(bytes) => bytes,
                            Err(_) => {
                                let _ = send.finish();
                                connection.close(0u32.into(), b"quic auth read failed");
                                break;
                            }
                        };
                        let auth_ok = match deserialize_connect_request_plain(&auth_request) {
                            Ok(device_id) => !device_id.is_empty(),
                            Err(_) => false,
                        };
                        let auth_response = match serialize_connect_response_plain(auth_ok) {
                            Ok(bytes) => bytes,
                            Err(_) => {
                                let _ = send.finish();
                                connection.close(0u32.into(), b"quic auth response encode failed");
                                break;
                            }
                        };
                        if send.write_all(&auth_response).await.is_err() {
                            let _ = send.finish();
                            connection.close(0u32.into(), b"quic auth response send failed");
                            break;
                        }
                        let _ = send.finish();
                        if !auth_ok {
                            connection.close(0u32.into(), b"quic auth rejected");
                            break;
                        }
                        session_authenticated = true;
                        continue;
                    }

                    let _ = recv.read_to_end(MAX_HOST_QUIC_STREAM_BYTES).await;
                }
            });
        }
    });
}

#[cfg(any(nexus_env = "host", not(nexus_env = "os")))]
fn host_run_tcp(identity: Identity, port: u16) {
    use std::thread;

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let authenticator = match HostAuthenticator::bind(addr, identity.clone()) {
        Ok(a) => a,
        Err(e) => panic!("bind host authenticator: {e}"),
    };
    let discovery = HostDiscovery::new();

    // Announce a minimal service set; higher layers may expand this later.
    let services = vec!["samgrd".to_string(), "bundlemgrd".to_string()];
    let announcement = Announcement::new(
        identity.device_id().clone(),
        services,
        port,
        authenticator.local_noise_public(),
    );
    match discovery.announce(announcement) {
        Ok(()) => {}
        Err(e) => panic!("announce local node: {e}"),
    }

    println!("dsoftbusd: ready");

    // Accept authenticated sessions and drain their streams in dedicated threads.
    loop {
        match authenticator.accept() {
            Ok(session) => match session.into_stream() {
                Ok(mut stream) => {
                    thread::spawn(move || {
                        while let Ok(frame) = stream.recv() {
                            if frame.is_none() {
                                break;
                            }
                        }
                    });
                }
                Err(err) => eprintln!("[dsoftbus] stream negotiation failed: {err}"),
            },
            Err(err) => {
                eprintln!("[dsoftbus] accept failed: {err}");
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
}

#[cfg(any(nexus_env = "host", not(nexus_env = "os")))]
fn host_run() {
    let identity = match Identity::generate() {
        Ok(id) => id,
        Err(e) => panic!("identity generation failed: {e}"),
    };
    let mode = host_transport_mode_from_env();
    let quic_runtime = load_host_quic_runtime_config_from_env().ok();
    let transport_selection = match resolve_host_transport_selection(mode, quic_runtime.is_some()) {
        Ok(selection) => selection,
        Err(err) => panic!("dsoftbus host transport selection failed: {err}"),
    };
    eprintln!("[dsoftbus] host transport selected {:?}", transport_selection.transport());

    let port = dsoftbus_port_from_env();
    match transport_selection.transport() {
        TransportKind::Tcp => host_run_tcp(identity, port),
        TransportKind::Quic => {
            let runtime = match quic_runtime {
                Some(cfg) => cfg,
                None => panic!("host quic selected but server runtime material is unavailable"),
            };
            host_run_quic(identity, port, runtime);
        }
    }
}

#[cfg(nexus_env = "os")]
fn os_run() {
    // Placeholder until kernel networking exists. Keep the symbol to satisfy
    // callers while making the limitation explicit at runtime.
    panic!("dsoftbus OS backend not implemented: pending kernel transport");
}
