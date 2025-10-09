use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use capnp::message::{Builder, ReaderOptions};
use capnp::serialize;
use ed25519_dalek::{Signature, VerifyingKey};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use snow::{params::NoiseParams, Builder as NoiseBuilder, HandshakeState, TransportState};

use crate::{
    derive_noise_keys, proof_message, Announcement, AuthError, Discovery, DiscoveryError,
    FramePayload, HandshakeProof, HandshakeRole, Session, SessionError, Stream, StreamError,
};
use identity::{DeviceId, Identity};
use nexus_idl_runtime::dsoftbus_capnp::{connect_request, connect_response, frame};

const MAX_MESSAGE: usize = 64 * 1024;
struct Registry {
    announcements: HashMap<String, Announcement>,
    watchers: Vec<mpsc::Sender<Announcement>>,
}

impl Registry {
    fn new() -> Self {
        Self { announcements: HashMap::new(), watchers: Vec::new() }
    }

    fn announce(&mut self, announcement: Announcement) {
        for sender in &self.watchers {
            let _ = sender.send(announcement.clone());
        }
        self.announcements.insert(announcement.device_id().as_str().to_string(), announcement);
    }

    fn watch(&mut self) -> mpsc::Receiver<Announcement> {
        let (tx, rx) = mpsc::channel();
        // Seed the watcher with current announcements.
        for announcement in self.announcements.values() {
            let _ = tx.send(announcement.clone());
        }
        self.watchers.push(tx);
        rx
    }
}

static REGISTRY: Lazy<Mutex<Registry>> = Lazy::new(|| Mutex::new(Registry::new()));

/// Host discovery implementation backed by an in-process registry.
pub struct HostDiscovery;

impl HostDiscovery {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HostDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

impl Discovery for HostDiscovery {
    type Error = DiscoveryError;
    type Stream = HostAnnouncementStream;

    fn announce(&self, announcement: Announcement) -> Result<(), Self::Error> {
        let mut registry = REGISTRY.lock();
        registry.announce(announcement);
        Ok(())
    }

    fn get(&self, device: &DeviceId) -> Result<Option<Announcement>, Self::Error> {
        let registry = REGISTRY.lock();
        Ok(registry.announcements.get(device.as_str()).cloned())
    }

    fn watch(&self) -> Result<Self::Stream, Self::Error> {
        let mut registry = REGISTRY.lock();
        let rx = registry.watch();
        Ok(HostAnnouncementStream { rx })
    }
}

/// Iterator yielding announcements discovered on the host backend.
pub struct HostAnnouncementStream {
    rx: mpsc::Receiver<Announcement>,
}

impl Iterator for HostAnnouncementStream {
    type Item = Announcement;

    fn next(&mut self) -> Option<Self::Item> {
        self.rx.recv().ok()
    }
}

fn noise_params() -> NoiseParams {
    // Parsing a constant; preserve panic-on-error semantics without using expect/unwrap directly
    match "Noise_XK_25519_ChaChaPoly_BLAKE2s".parse() {
        Ok(p) => p,
        Err(e) => panic!("invalid noise params: {e}"),
    }
}

fn read_frame(stream: &mut TcpStream) -> Result<Vec<u8>, std::io::Error> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_MESSAGE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame exceeds maximum size",
        ));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    Ok(buf)
}

fn try_read_frame(stream: &mut TcpStream) -> Result<Option<Vec<u8>>, std::io::Error> {
    let mut len_buf = [0u8; 4];
    match stream.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
        Err(err) => return Err(err),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_MESSAGE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame exceeds maximum size",
        ));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    Ok(Some(buf))
}

fn write_frame(stream: &mut TcpStream, payload: &[u8]) -> Result<(), std::io::Error> {
    let len = payload.len() as u32;
    stream.write_all(&len.to_be_bytes())?;
    stream.write_all(payload)?;
    stream.flush()?;
    Ok(())
}

fn build_responder(noise_secret: &[u8; 32]) -> Result<HandshakeState, AuthError> {
    NoiseBuilder::new(noise_params())
        .local_private_key(noise_secret)
        .build_responder()
        .map_err(|err| AuthError::Noise(err.to_string()))
}

fn build_initiator(
    noise_secret: &[u8; 32],
    remote: &[u8; 32],
) -> Result<HandshakeState, AuthError> {
    NoiseBuilder::new(noise_params())
        .local_private_key(noise_secret)
        .remote_public_key(remote)
        .build_initiator()
        .map_err(|err| AuthError::Noise(err.to_string()))
}

fn parse_verifying_key(bytes: &[u8]) -> Result<VerifyingKey, AuthError> {
    let array: [u8; 32] =
        bytes.try_into().map_err(|_| AuthError::Identity("invalid verifying key length".into()))?;
    VerifyingKey::from_bytes(&array)
        .map_err(|err| AuthError::Identity(format!("verifying key: {err}")))
}

fn parse_signature(bytes: &[u8]) -> Result<Signature, AuthError> {
    let array: [u8; 64] =
        bytes.try_into().map_err(|_| AuthError::Identity("invalid signature length".into()))?;
    Ok(Signature::from_bytes(&array))
}

fn decrypt_payload(state: &mut TransportState, frame: &[u8]) -> Result<Vec<u8>, StreamError> {
    let mut buf = vec![0u8; frame.len()];
    let len =
        state.read_message(frame, &mut buf).map_err(|err| StreamError::Crypto(err.to_string()))?;
    buf.truncate(len);
    Ok(buf)
}

fn encrypt_payload(state: &mut TransportState, data: &[u8]) -> Result<Vec<u8>, StreamError> {
    let mut buf = vec![0u8; data.len() + 64];
    let len =
        state.write_message(data, &mut buf).map_err(|err| StreamError::Crypto(err.to_string()))?;
    buf.truncate(len);
    Ok(buf)
}

fn serialize_message<F>(build: F) -> Result<Vec<u8>, AuthError>
where
    F: FnOnce(&mut Builder<capnp::message::HeapAllocator>),
{
    let mut message = Builder::new_default();
    build(&mut message);
    serialize::write_message(&mut Vec::new(), &message)
        .map(|_| {
            let mut buf = Vec::new();
            match serialize::write_message(&mut buf, &message) {
                Ok(()) => buf,
                Err(err) => panic!("serialize capnp message: {err}"),
            }
        })
        .map_err(|err| AuthError::Protocol(err.to_string()))
}

fn deserialize_connect_request(bytes: &[u8]) -> Result<String, AuthError> {
    let mut cursor = std::io::Cursor::new(bytes);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|err| AuthError::Protocol(err.to_string()))?;
    let reader: connect_request::Reader<'_> =
        message.get_root().map_err(|err| AuthError::Protocol(err.to_string()))?;
    let txt = reader.get_device_id().map_err(|err| AuthError::Protocol(err.to_string()))?;
    Ok(txt.to_str().map_err(|e| AuthError::Protocol(e.to_string()))?.to_string())
}

fn deserialize_frame(bytes: &[u8]) -> Result<FramePayload, StreamError> {
    let mut cursor = std::io::Cursor::new(bytes);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|err| StreamError::Protocol(err.to_string()))?;
    let reader: frame::Reader<'_> =
        message.get_root().map_err(|err| StreamError::Protocol(err.to_string()))?;
    let payload = reader.get_bytes().map_err(|err| StreamError::Protocol(err.to_string()))?;
    Ok(FramePayload { channel: reader.get_chan(), bytes: payload.to_vec() })
}

fn serialize_frame(channel: u32, payload: &[u8]) -> Result<Vec<u8>, StreamError> {
    let mut message = Builder::new_default();
    {
        let mut frame = message.init_root::<frame::Builder<'_>>();
        frame.set_chan(channel);
        frame.set_bytes(payload);
    }
    let mut buf = Vec::new();
    serialize::write_message(&mut buf, &message)
        .map_err(|err| StreamError::Protocol(err.to_string()))?;
    Ok(buf)
}

fn send_connect_response(
    stream: &mut TcpStream,
    state: &mut TransportState,
    ok: bool,
) -> Result<(), AuthError> {
    let bytes = serialize_message(|message| {
        let mut response: connect_response::Builder<'_> = message.init_root();
        response.set_ok(ok);
    })?;
    let encrypted =
        encrypt_payload(state, &bytes).map_err(|err| AuthError::Protocol(err.to_string()))?;
    eprintln!("[dsoftbus] server: sending encrypted connect_response len={}", encrypted.len());
    write_frame(stream, &encrypted)?;
    Ok(())
}

fn send_connect_request(
    stream: &mut TcpStream,
    state: &mut TransportState,
    device_id: &DeviceId,
) -> Result<(), AuthError> {
    let bytes = serialize_message(|message| {
        let mut request: connect_request::Builder<'_> = message.init_root();
        request.set_device_id(device_id.as_str());
    })?;
    let encrypted =
        encrypt_payload(state, &bytes).map_err(|err| AuthError::Protocol(err.to_string()))?;
    write_frame(stream, &encrypted)?;
    Ok(())
}

fn receive_connect_response(
    stream: &mut TcpStream,
    state: &mut TransportState,
) -> Result<bool, AuthError> {
    let frame = read_frame(stream)?;
    eprintln!("[dsoftbus] client: received encrypted connect_response len={}", frame.len());
    let bytes =
        decrypt_payload(state, &frame).map_err(|err| AuthError::Protocol(err.to_string()))?;
    let mut cursor = std::io::Cursor::new(bytes);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new())
        .map_err(|err| AuthError::Protocol(err.to_string()))?;
    let reader: connect_response::Reader<'_> =
        message.get_root().map_err(|err| AuthError::Protocol(err.to_string()))?;
    Ok(reader.get_ok())
}

fn receive_connect_request(
    stream: &mut TcpStream,
    state: &mut TransportState,
) -> Result<String, AuthError> {
    let frame = read_frame(stream)?;
    eprintln!("[dsoftbus] server: received encrypted connect_request len={}", frame.len());
    let bytes =
        decrypt_payload(state, &frame).map_err(|err| AuthError::Protocol(err.to_string()))?;
    deserialize_connect_request(&bytes)
}

fn validate_proof(
    proof: HandshakeProof,
    expected_role: HandshakeRole,
    expected_static: &[u8; 32],
) -> Result<(DeviceId, VerifyingKey), AuthError> {
    let verifying_key = parse_verifying_key(&proof.verifying_key)?;
    let device_id = DeviceId::from_verifying_key(&verifying_key);
    if device_id.as_str() != proof.device_id {
        return Err(AuthError::Identity("device id mismatch".into()));
    }

    let signature = parse_signature(&proof.signature)?;
    use ed25519_dalek::Verifier;
    verifying_key
        .verify(&proof_message(expected_role, expected_static), &signature)
        .map_err(|err| AuthError::Identity(format!("signature verify failed: {err}")))?;

    Ok((device_id, verifying_key))
}

fn read_handshake_proof(
    state: &mut HandshakeState,
    message: &[u8],
) -> Result<HandshakeProof, AuthError> {
    let mut buf = vec![0u8; MAX_MESSAGE];
    let len =
        state.read_message(message, &mut buf).map_err(|err| AuthError::Noise(err.to_string()))?;
    buf.truncate(len);
    serde_json::from_slice(&buf).map_err(|err| AuthError::Protocol(err.to_string()))
}

fn write_handshake_proof(
    state: &mut HandshakeState,
    proof: &HandshakeProof,
) -> Result<Vec<u8>, AuthError> {
    let payload = serde_json::to_vec(proof).map_err(|err| AuthError::Protocol(err.to_string()))?;
    let mut buf = vec![0u8; payload.len() + 64];
    let len =
        state.write_message(&payload, &mut buf).map_err(|err| AuthError::Noise(err.to_string()))?;
    buf.truncate(len);
    Ok(buf)
}

fn ensure_remote_static(state: &HandshakeState) -> Result<[u8; 32], AuthError> {
    let static_key = state
        .get_remote_static()
        .ok_or_else(|| AuthError::Identity("missing remote static".into()))?;
    let array: [u8; 32] = static_key
        .try_into()
        .map_err(|_| AuthError::Identity("invalid remote static length".into()))?;
    Ok(array)
}

fn handshake_accept(
    identity: &Identity,
    noise_secret: &[u8; 32],
    noise_public: &[u8; 32],
    stream: &mut TcpStream,
) -> Result<(TransportState, DeviceId), AuthError> {
    let mut state = build_responder(noise_secret)?;
    eprintln!("[dsoftbus] accept: waiting for msg1");

    // message 1
    let message1 = read_frame(stream)?;
    let mut scratch = vec![0u8; MAX_MESSAGE];
    state.read_message(&message1, &mut scratch).map_err(|err| AuthError::Noise(err.to_string()))?;
    eprintln!("[dsoftbus] accept: got msg1");

    // message 2 with server proof
    let message = proof_message(HandshakeRole::Server, noise_public);
    let signature = identity.sign(&message);
    let proof = HandshakeProof::new(identity.device_id(), &identity.verifying_key(), &signature);
    eprintln!("[dsoftbus] accept: sending msg2 (server proof)");
    let response = write_handshake_proof(&mut state, &proof)?;
    write_frame(stream, &response)?;
    eprintln!("[dsoftbus] accept: sent msg2");

    // message 3 with client proof
    eprintln!("[dsoftbus] accept: waiting for msg3");
    let message3 = read_frame(stream)?;
    let proof = read_handshake_proof(&mut state, &message3)?;

    let remote_static = ensure_remote_static(&state)?;
    let (device_id, _) = validate_proof(proof, HandshakeRole::Client, &remote_static)?;

    let transport = state.into_transport_mode().map_err(|err| AuthError::Noise(err.to_string()))?;
    eprintln!("[dsoftbus] accept: transport established for {}", device_id.as_str());
    Ok((transport, device_id))
}

fn handshake_connect(
    identity: &Identity,
    noise_secret: &[u8; 32],
    noise_public: &[u8; 32],
    announcement: &Announcement,
    stream: &mut TcpStream,
) -> Result<(TransportState, DeviceId), AuthError> {
    let mut state = build_initiator(noise_secret, announcement.noise_static())?;

    // message 1
    let mut message1 = vec![0u8; MAX_MESSAGE];
    eprintln!("[dsoftbus] connect: sending msg1");
    let len =
        state.write_message(&[], &mut message1).map_err(|err| AuthError::Noise(err.to_string()))?;
    message1.truncate(len);
    write_frame(stream, &message1)?;
    eprintln!("[dsoftbus] connect: sent msg1");

    // message 2
    eprintln!("[dsoftbus] connect: waiting for msg2");
    let message2 = read_frame(stream)?;
    let proof = read_handshake_proof(&mut state, &message2)?;
    let remote_static = ensure_remote_static(&state)?;

    if &remote_static != announcement.noise_static() {
        return Err(AuthError::Identity("server static mismatch".into()));
    }

    let (device_id, _) = validate_proof(proof, HandshakeRole::Server, &remote_static)?;

    let message = proof_message(HandshakeRole::Client, noise_public);
    let signature = identity.sign(&message);
    let proof = HandshakeProof::new(identity.device_id(), &identity.verifying_key(), &signature);
    eprintln!("[dsoftbus] connect: sending msg3 (client proof)");
    let final_message = write_handshake_proof(&mut state, &proof)?;
    write_frame(stream, &final_message)?;
    eprintln!("[dsoftbus] connect: sent msg3");

    let transport = state.into_transport_mode().map_err(|err| AuthError::Noise(err.to_string()))?;
    eprintln!("[dsoftbus] connect: transport established for {}", device_id.as_str());

    Ok((transport, device_id))
}

/// Host authenticator backed by TCP and Noise XK.
pub struct HostAuthenticator {
    listener: Arc<TcpListener>,
    identity: Identity,
    noise_secret: [u8; 32],
    noise_public: [u8; 32],
}

impl Clone for HostAuthenticator {
    fn clone(&self) -> Self {
        Self {
            listener: Arc::clone(&self.listener),
            identity: self.identity.clone(),
            noise_secret: self.noise_secret,
            noise_public: self.noise_public,
        }
    }
}

impl HostAuthenticator {
    pub fn local_noise_public(&self) -> [u8; 32] {
        self.noise_public
    }

    /// Returns the identity backing this authenticator.
    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    fn new(listener: TcpListener, identity: Identity) -> Self {
        let (noise_secret, noise_public) = derive_noise_keys(&identity);
        Self { listener: Arc::new(listener), identity, noise_secret, noise_public }
    }
}

impl crate::Authenticator for HostAuthenticator {
    type Session = HostSession;

    fn bind(addr: SocketAddr, identity: Identity) -> Result<Self, AuthError> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(false).map_err(AuthError::Io)?;
        Ok(Self::new(listener, identity))
    }

    fn accept(&self) -> Result<Self::Session, AuthError> {
        let (mut stream, _) = self.listener.accept()?;
        stream.set_nodelay(true)?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
        let (mut transport, device_id) =
            handshake_accept(&self.identity, &self.noise_secret, &self.noise_public, &mut stream)?;

        let request_id = receive_connect_request(&mut stream, &mut transport)?;
        if request_id != device_id.as_str() {
            send_connect_response(&mut stream, &mut transport, false)?;
            return Err(AuthError::Identity("device mismatch".into()));
        }
        send_connect_response(&mut stream, &mut transport, true)?;

        Ok(HostSession { stream, transport, remote_device: device_id })
    }

    fn connect(&self, announcement: &Announcement) -> Result<Self::Session, AuthError> {
        let addr = SocketAddr::from(([127, 0, 0, 1], announcement.port()));
        let mut stream = TcpStream::connect(addr)?;
        stream.set_nodelay(true)?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
        let (mut transport, device_id) = handshake_connect(
            &self.identity,
            &self.noise_secret,
            &self.noise_public,
            announcement,
            &mut stream,
        )?;
        eprintln!("[dsoftbus] connect: sending connect_request");
        send_connect_request(&mut stream, &mut transport, self.identity.device_id())?;
        eprintln!("[dsoftbus] connect: waiting for connect_response");
        let ok = receive_connect_response(&mut stream, &mut transport)?;
        if !ok {
            return Err(AuthError::Identity("connection rejected".into()));
        }
        Ok(HostSession { stream, transport, remote_device: device_id })
    }
}

/// Established session for the host transport.
pub struct HostSession {
    stream: TcpStream,
    transport: TransportState,
    remote_device: DeviceId,
}

impl Session for HostSession {
    type Stream = HostStream;

    fn remote_device_id(&self) -> &DeviceId {
        &self.remote_device
    }

    fn into_stream(self) -> Result<Self::Stream, SessionError> {
        Ok(HostStream { stream: self.stream, transport: self.transport })
    }
}

/// Reliable stream backed by an encrypted TCP transport.
pub struct HostStream {
    stream: TcpStream,
    transport: TransportState,
}

impl Stream for HostStream {
    fn send(&mut self, channel: u32, payload: &[u8]) -> Result<(), StreamError> {
        let frame = serialize_frame(channel, payload)?;
        let encrypted = encrypt_payload(&mut self.transport, &frame)?;
        write_frame(&mut self.stream, &encrypted).map_err(StreamError::from)
    }

    fn recv(&mut self) -> Result<Option<FramePayload>, StreamError> {
        match try_read_frame(&mut self.stream).map_err(StreamError::from)? {
            Some(frame) => {
                let bytes = decrypt_payload(&mut self.transport, &frame)?;
                deserialize_frame(&bytes).map(Some)
            }
            None => Ok(None),
        }
    }
}
