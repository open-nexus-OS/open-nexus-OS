// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]
//! CONTEXT: Identity daemon – device id, sign, verify over Cap'n Proto IPC
//! OWNERS: @services-team
//! PUBLIC API: service_main_loop(), loopback_transport(), touch_schemas()
//! DEPENDS_ON: nexus_ipc, nexus_idl_runtime (capnp), identity lib, ed25519-dalek
//! INVARIANTS: Separate from Keystore; stable readiness prints
//! ADR: docs/adr/0017-service-architecture.md

#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!("nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '...\"os\"'");

#[cfg(not(feature = "idl-capnp"))]
compile_error!("Enable the `idl-capnp` feature to build identityd handlers.");

use core::convert::TryInto;
use std::fmt;
use std::io::Cursor;

use capnp::message::{Builder, HeapAllocator, ReaderOptions};
use capnp::serialize;
use ed25519_dalek::{Signature, VerifyingKey};
use identity::{Identity, IdentityError};
use nexus_idl_runtime::identity_capnp::{
    get_device_id_response, sign_request, sign_response, verify_request, verify_response,
};
use nexus_ipc::{self, Wait};

const OPCODE_GET_DEVICE_ID: u8 = 1;
const OPCODE_SIGN: u8 = 2;
const OPCODE_VERIFY: u8 = 3;

/// Trait implemented by transports capable of delivering request frames to the daemon.
pub trait Transport {
    /// Error type returned by the transport implementation.
    type Error: Into<TransportError>;

    /// Receives the next request frame, returning `Ok(None)` when the peer disconnects.
    fn recv(&mut self) -> Result<Option<Vec<u8>>, Self::Error>;

    /// Sends a response frame back to the caller.
    fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error>;
}

/// Transport layer failures surfaced to the daemon loop.
#[derive(Debug)]
pub enum TransportError {
    /// Remote endpoint has closed the connection.
    Closed,
    /// I/O level failure (e.g. socket error).
    Io(std::io::Error),
    /// Backend not implemented for this configuration.
    Unsupported,
    /// Any other transport issue described by a string message.
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
            nexus_ipc::IpcError::Kernel(inner) => {
                Self::Other(format!("kernel ipc error: {inner:?}"))
            }
        }
    }
}

/// Errors surfaced by the identity daemon.
#[derive(Debug)]
pub enum ServerError {
    /// Underlying transport failed.
    Transport(TransportError),
    /// Failed to decode an incoming frame.
    Decode(String),
    /// Failed to encode the response frame.
    Encode(capnp::Error),
    /// Identity domain failure (e.g. key parsing, signing).
    Identity(IdentityError),
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(err) => write!(f, "transport error: {err}"),
            Self::Decode(msg) => write!(f, "decode error: {msg}"),
            Self::Encode(err) => write!(f, "encode error: {err}"),
            Self::Identity(err) => write!(f, "identity error: {err}"),
        }
    }
}

impl std::error::Error for ServerError {}

impl From<TransportError> for ServerError {
    fn from(err: TransportError) -> Self {
        Self::Transport(err)
    }
}

impl From<IdentityError> for ServerError {
    fn from(err: IdentityError) -> Self {
        Self::Identity(err)
    }
}

struct IdentityService {
    identity: Identity,
}

impl IdentityService {
    fn new(identity: Identity) -> Self {
        Self { identity }
    }

    fn handle_frame(&self, frame: &[u8]) -> Result<Vec<u8>, ServerError> {
        let (opcode, payload) =
            frame.split_first().ok_or_else(|| ServerError::Decode("empty frame".into()))?;
        match opcode {
            &OPCODE_GET_DEVICE_ID => self.handle_get_device_id(),
            &OPCODE_SIGN => self.handle_sign(payload),
            &OPCODE_VERIFY => self.handle_verify(payload),
            other => Err(ServerError::Decode(format!("unknown opcode: {other}"))),
        }
    }

    fn handle_get_device_id(&self) -> Result<Vec<u8>, ServerError> {
        let mut message = Builder::new_default();
        {
            let mut response = message.init_root::<get_device_id_response::Builder<'_>>();
            response.set_device_id(self.identity.device_id().as_str());
        }
        encode_response(OPCODE_GET_DEVICE_ID, &message)
    }

    fn handle_sign(&self, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        let mut cursor = Cursor::new(payload);
        let message = serialize::read_message(&mut cursor, ReaderOptions::new())
            .map_err(|err| ServerError::Decode(err.to_string()))?;
        let request = message
            .get_root::<sign_request::Reader<'_>>()
            .map_err(|err| ServerError::Decode(err.to_string()))?;
        let data = request.get_payload().map_err(|err| ServerError::Decode(err.to_string()))?;
        let signature: Signature = self.identity.sign(data);

        let mut response = Builder::new_default();
        {
            let mut payload = response.init_root::<sign_response::Builder<'_>>();
            payload.set_ok(true);
            let bytes = signature.to_bytes();
            payload.set_signature(&bytes);
        }
        encode_response(OPCODE_SIGN, &response)
    }

    fn handle_verify(&self, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        let mut cursor = Cursor::new(payload);
        let message = serialize::read_message(&mut cursor, ReaderOptions::new())
            .map_err(|err| ServerError::Decode(err.to_string()))?;
        let request = message
            .get_root::<verify_request::Reader<'_>>()
            .map_err(|err| ServerError::Decode(err.to_string()))?;
        let message_bytes =
            request.get_payload().map_err(|err| ServerError::Decode(err.to_string()))?;
        let signature_bytes =
            request.get_signature().map_err(|err| ServerError::Decode(err.to_string()))?;
        let verifying_key_bytes =
            request.get_verifying_key().map_err(|err| ServerError::Decode(err.to_string()))?;

        let signature_bytes: [u8; 64] = signature_bytes.try_into().map_err(|_| {
            ServerError::Identity(IdentityError::Deserialize("invalid signature length".into()))
        })?;
        let signature = Signature::from_bytes(&signature_bytes);
        let verifying_key_slice: [u8; 32] = verifying_key_bytes.try_into().map_err(|_| {
            ServerError::Identity(IdentityError::Deserialize("invalid verifying key length".into()))
        })?;
        let verifying_key = VerifyingKey::from_bytes(&verifying_key_slice)
            .map_err(|err| ServerError::Identity(IdentityError::Crypto(err.to_string())))?;

        let valid = Identity::verify_with_key(&verifying_key, message_bytes, &signature);

        let mut response = Builder::new_default();
        {
            let mut payload = response.init_root::<verify_response::Builder<'_>>();
            payload.set_valid(valid);
        }
        encode_response(OPCODE_VERIFY, &response)
    }
}

fn encode_response(opcode: u8, message: &Builder<HeapAllocator>) -> Result<Vec<u8>, ServerError> {
    let mut body = Vec::new();
    serialize::write_message(&mut body, message).map_err(ServerError::Encode)?;
    let mut frame = Vec::with_capacity(1 + body.len());
    frame.push(opcode);
    frame.extend_from_slice(&body);
    Ok(frame)
}

/// Ready signal helper used by the init system to gate service startup.
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    /// Creates a new notifier from the provided closure.
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

/// IPC transport backed by the [`nexus-ipc`] loopback implementation.
pub struct IpcTransport<T> {
    server: T,
}

impl<T> IpcTransport<T> {
    /// Wraps a loopback server instance.
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

/// Runs the daemon main loop using the default transport backend.
pub fn service_main_loop(notifier: ReadyNotifier) -> Result<(), ServerError> {
    let identity = Identity::generate().map_err(ServerError::Identity)?;

    #[cfg(nexus_env = "host")]
    {
        let (client, server) = nexus_ipc::loopback_channel();
        let _client_guard = client;
        let mut transport = IpcTransport::new(server);
        notifier.notify();
        println!("identityd: ready");
        // Best-effort registration with samgr (no-op on host without a shared client).
        let _ = try_register_with_samgr();
        serve(&mut transport, identity)
    }

    #[cfg(nexus_env = "os")]
    {
        let server = nexus_ipc::KernelServer::new()
            .map_err(|err| ServerError::Transport(TransportError::from(err)))?;
        let mut transport = IpcTransport::new(server);
        notifier.notify();
        println!("identityd: ready");
        // Attempt to register with samgr on OS builds once IPC is wired. Ignore failures.
        let _ = try_register_with_samgr();
        serve(&mut transport, identity)
    }
}

fn serve<T>(transport: &mut T, identity: Identity) -> Result<(), ServerError>
where
    T: Transport,
{
    let service = IdentityService::new(identity);
    loop {
        match transport.recv().map_err(|err| ServerError::Transport(err.into()))? {
            Some(frame) => {
                let response = service.handle_frame(&frame)?;
                transport.send(&response).map_err(|err| ServerError::Transport(err.into()))?;
            }
            None => return Ok(()),
        }
    }
}

/// Creates a loopback transport pair for host tests.
#[cfg(nexus_env = "host")]
pub fn loopback_transport() -> (nexus_ipc::LoopbackClient, IpcTransport<nexus_ipc::LoopbackServer>)
{
    let (client, server) = nexus_ipc::loopback_channel();
    (client, IpcTransport::new(server))
}

/// Touches the Cap'n Proto schema so release builds keep the generated module.
pub fn touch_schemas() {
    #[cfg(feature = "idl-capnp")]
    {
        let _ = core::any::type_name::<get_device_id_response::Reader<'static>>();
        let _ = core::any::type_name::<sign_request::Reader<'static>>();
        let _ = core::any::type_name::<sign_response::Reader<'static>>();
        let _ = core::any::type_name::<verify_request::Reader<'static>>();
        let _ = core::any::type_name::<verify_response::Reader<'static>>();
    }
}

/// Attempts to register the daemon with `samgr` if a client is available.
fn try_register_with_samgr() -> Result<(), String> {
    // Placeholder: once a shared IPC client is available, send a register frame
    // to `samgrd` using the standard nexus-ipc client path. For now return Ok.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_device_id_roundtrip() {
        let identity = Identity::generate().expect("identity");
        let service = IdentityService::new(identity.clone());
        let response = service.handle_frame(&[OPCODE_GET_DEVICE_ID]).expect("device id response");
        assert_eq!(response.first(), Some(&OPCODE_GET_DEVICE_ID));
        let mut cursor = Cursor::new(&response[1..]);
        let message =
            serialize::read_message(&mut cursor, ReaderOptions::new()).expect("read response");
        let reader =
            message.get_root::<get_device_id_response::Reader<'_>>().expect("response root");
        assert_eq!(
            reader.get_device_id().expect("device id").to_str().expect("utf8"),
            identity.device_id().as_str()
        );
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let identity = Identity::generate().expect("identity");
        let service = IdentityService::new(identity.clone());
        let payload = b"sign-me";

        let mut message = Builder::new_default();
        {
            let mut request = message.init_root::<sign_request::Builder<'_>>();
            request.set_payload(payload);
        }
        let mut body = Vec::new();
        serialize::write_message(&mut body, &message).expect("serialize sign request");
        let mut frame = Vec::with_capacity(1 + body.len());
        frame.push(OPCODE_SIGN);
        frame.extend_from_slice(&body);

        let response = service.handle_frame(&frame).expect("sign response");
        assert_eq!(response.first(), Some(&OPCODE_SIGN));

        let mut cursor = Cursor::new(&response[1..]);
        let message =
            serialize::read_message(&mut cursor, ReaderOptions::new()).expect("read sign response");
        let reader = message.get_root::<sign_response::Reader<'_>>().expect("sign response root");
        assert!(reader.get_ok(), "sign operation succeeds");
        let signature = reader.get_signature().expect("signature").to_vec();

        let mut verify_message = Builder::new_default();
        {
            let mut request = verify_message.init_root::<verify_request::Builder<'_>>();
            request.set_payload(payload);
            request.set_signature(&signature);
            request.set_verifying_key(identity.verifying_key().as_bytes());
        }
        let mut verify_body = Vec::new();
        serialize::write_message(&mut verify_body, &verify_message)
            .expect("serialize verify request");
        let mut verify_frame = Vec::with_capacity(1 + verify_body.len());
        verify_frame.push(OPCODE_VERIFY);
        verify_frame.extend_from_slice(&verify_body);

        let verify_response = service.handle_frame(&verify_frame).expect("verify response");
        assert_eq!(verify_response.first(), Some(&OPCODE_VERIFY));
        let mut cursor = Cursor::new(&verify_response[1..]);
        let message = serialize::read_message(&mut cursor, ReaderOptions::new())
            .expect("read verify response");
        let reader =
            message.get_root::<verify_response::Reader<'_>>().expect("verify response root");
        assert!(reader.get_valid(), "signature must validate");
    }
}
