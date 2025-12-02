//! CONTEXT: DSoftBus-lite distributed service fabric
//! OWNERS: @runtime
//! STATUS: Functional (host backend), Placeholder (OS backend - pending kernel transport)
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 integration test
//!
//! PUBLIC API:
//!   - Announcement: Service discovery announcement
//!   - Discovery trait: Service discovery interface
//!   - Authenticator trait: Session authentication
//!   - Session/Stream traits: Communication channels
//!
//! DEPENDENCIES:
//!   - identity: Device identity and signing
//!   - x25519-dalek: Noise protocol
//!   - ed25519-dalek: Digital signatures
//!   - serde: Message serialization
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#![forbid(unsafe_code)]

#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!("nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '...\"os\"'");

use std::net::SocketAddr;

use ed25519_dalek::{Signature, VerifyingKey};
use identity::{DeviceId, Identity};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use x25519_dalek::{PublicKey as NoisePublicKey, StaticSecret as NoiseSecret};

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
        Self {
            device_id,
            services,
            port,
            noise_static,
        }
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
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&hash);
    let secret = NoiseSecret::from(bytes);
    let public = NoisePublicKey::from(&secret);
    (secret.to_bytes(), public.to_bytes())
}

#[cfg(nexus_env = "host")]
mod host;

#[cfg(nexus_env = "host")]
pub use host::{HostAuthenticator, HostDiscovery, HostSession, HostStream};

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
    #[cfg(nexus_env = "host")]
    host_run();

    #[cfg(nexus_env = "os")]
    os_run();
}

#[cfg(nexus_env = "host")]
fn host_run() {
    use std::env;
    use std::thread;

    let identity = match Identity::generate() {
        Ok(id) => id,
        Err(e) => panic!("identity generation failed: {e}"),
    };

    // Choose a listening port. Allow override via DSOFTBUS_PORT for integration.
    let port: u16 = env::var("DSOFTBUS_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(34_567);
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

    // Print readiness marker once the listener and discovery are active.
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
                // Back off briefly on transient failures.
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
}

#[cfg(nexus_env = "os")]
fn os_run() {
    // Placeholder until kernel networking exists. Keep the symbol to satisfy
    // callers while making the limitation explicit at runtime.
    panic!("dsoftbus OS backend not implemented: pending kernel transport");
}
