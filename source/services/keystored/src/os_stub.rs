// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Keystored (os-lite) — key/value shim plus device identity key operations (bring-up)
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 9 unit tests (os-lite)
//! ADR: docs/adr/0017-service-architecture.md
//!
//! SECURITY INVARIANTS:
//! - Never log entropy bytes or private key material
//! - Bind policy checks to `sender_service_id` (kernel-provided identity)
//! - Deny-by-default via policyd for sensitive operations

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use core::fmt;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicU32, Ordering};
use core::time::Duration;

use nexus_abi::{debug_putc, yield_};
use nexus_ipc::budget::{deadline_after, OsClock};
use nexus_ipc::reqrep::{recv_match_until, ReplyBuffer};
use nexus_ipc::{KernelClient, KernelServer, Server as _, Wait};
use statefs::client::StatefsClient;
use statefs::StatefsError;

/// Result type surfaced by the lite keystored shim.
pub type LiteResult<T> = Result<T, ServerError>;

/// Placeholder transport trait retained for API compatibility.
pub trait Transport {
    /// Associated error type for the transport.
    type Error;
}

/// Stub transport wrapper; no runtime transport support in os-lite yet.
pub struct IpcTransport<T> {
    _marker: PhantomData<T>,
}

impl<T> IpcTransport<T> {
    /// Constructs the transport wrapper.
    pub fn new(_server: T) -> Self {
        Self { _marker: PhantomData }
    }
}

/// Notifies init once the service reports readiness.
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    /// Creates a notifier from the provided closure.
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

/// Transport level errors surfaced by the shim implementation.
#[derive(Debug)]
pub enum TransportError {
    /// Transport support is not yet implemented in the os-lite runtime.
    Unsupported,
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "transport unsupported"),
        }
    }
}

/// Server level errors.
#[derive(Debug)]
pub enum ServerError {
    /// Functionality not yet implemented in the os-lite path.
    Unsupported(&'static str),
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(msg) => write!(f, "{msg} unsupported"),
        }
    }
}

impl From<TransportError> for ServerError {
    fn from(_err: TransportError) -> Self {
        Self::Unsupported("transport")
    }
}

/// Runs the keystored daemon with the provided transport (stubbed).
pub fn run_with_transport<T: Transport>(_transport: &mut T) -> LiteResult<()> {
    Err(ServerError::Unsupported("keystored run_with_transport"))
}

/// Runs the keystored daemon using the default transport (stubbed).
pub fn run_default() -> LiteResult<()> {
    Err(ServerError::Unsupported("keystored run_default"))
}

/// Runs the keystored daemon using the default transport and anchor set (stubbed).
pub fn run_with_transport_default_anchors<T: Transport>(_transport: &mut T) -> LiteResult<()> {
    Err(ServerError::Unsupported("keystored run_with_transport_default_anchors"))
}

enum KeyStoreBackend {
    Statefs(StatefsStore),
    Memory(BTreeMap<(u64, Vec<u8>), Vec<u8>>),
}

struct KeyStore {
    backend: KeyStoreBackend,
    device_key_bytes: Option<[u8; 32]>,
}

impl KeyStore {
    fn new() -> Self {
        if let Some(mut store) = StatefsStore::new() {
            emit_line("keystored: statefs backend ok");
            let device_key_bytes = store.load_device_key().ok().flatten();
            return Self { backend: KeyStoreBackend::Statefs(store), device_key_bytes };
        }
        emit_line("keystored: memory backend fallback");
        Self { backend: KeyStoreBackend::Memory(BTreeMap::new()), device_key_bytes: None }
    }

    #[cfg(test)]
    fn new_memory() -> Self {
        Self { backend: KeyStoreBackend::Memory(BTreeMap::new()), device_key_bytes: None }
    }

    fn get(&mut self, sender_service_id: u64, key: &[u8]) -> Result<Option<Vec<u8>>, StatefsError> {
        match &mut self.backend {
            KeyStoreBackend::Statefs(store) => store.get(sender_service_id, key),
            KeyStoreBackend::Memory(map) => {
                Ok(map.get(&(sender_service_id, key.to_vec())).cloned())
            }
        }
    }

    fn put(
        &mut self,
        sender_service_id: u64,
        key: &[u8],
        value: &[u8],
    ) -> Result<(), StatefsError> {
        match &mut self.backend {
            KeyStoreBackend::Statefs(store) => store.put(sender_service_id, key, value),
            KeyStoreBackend::Memory(map) => {
                map.insert((sender_service_id, key.to_vec()), value.to_vec());
                Ok(())
            }
        }
    }

    fn delete(&mut self, sender_service_id: u64, key: &[u8]) -> Result<bool, StatefsError> {
        match &mut self.backend {
            KeyStoreBackend::Statefs(store) => store.delete(sender_service_id, key),
            KeyStoreBackend::Memory(map) => {
                Ok(map.remove(&(sender_service_id, key.to_vec())).is_some())
            }
        }
    }

    fn device_key_bytes(&self) -> Option<[u8; 32]> {
        self.device_key_bytes
    }

    /// Reload device key from statefsd (for persistence proof after reboot).
    fn reload_device_key(&mut self) -> Result<Option<[u8; 32]>, StatefsError> {
        match &mut self.backend {
            KeyStoreBackend::Statefs(store) => match store.load_device_key() {
                Ok(Some(bytes)) => {
                    emit_line("keystored: reload from statefs ok");
                    self.device_key_bytes = Some(bytes);
                    Ok(Some(bytes))
                }
                Ok(None) => {
                    emit_line("keystored: reload from statefs (not found)");
                    Ok(None)
                }
                Err(err) => {
                    emit_line("keystored: reload from statefs err");
                    Err(err)
                }
            },
            KeyStoreBackend::Memory(_) => {
                emit_line("keystored: reload from memory backend");
                Ok(self.device_key_bytes)
            }
        }
    }

    fn set_device_key_bytes(&mut self, bytes: [u8; 32]) -> Result<(), StatefsError> {
        if let KeyStoreBackend::Statefs(store) = &mut self.backend {
            store.store_device_key(&bytes)?;
        }
        self.device_key_bytes = Some(bytes);
        Ok(())
    }
}

struct StatefsStore {
    client: StatefsClient,
}

impl StatefsStore {
    fn new() -> Option<Self> {
        // init-lite deterministic slots for keystored -> statefsd:
        // - send=0x07, reply recv=0x05, reply send=0x06
        const STATEFS_SEND_SLOT: u32 = 0x07;
        const REPLY_RECV_SLOT: u32 = 0x05;
        const REPLY_SEND_SLOT: u32 = 0x06;
        let client = KernelClient::new_with_slots(STATEFS_SEND_SLOT, REPLY_RECV_SLOT).ok()?;
        let reply = KernelClient::new_with_slots(REPLY_SEND_SLOT, REPLY_RECV_SLOT).ok();
        let client = StatefsClient::from_clients(client, reply);
        Some(Self { client })
    }

    fn get(&mut self, sender_service_id: u64, key: &[u8]) -> Result<Option<Vec<u8>>, StatefsError> {
        let path = self.key_path(sender_service_id, key)?;
        match self.client.get(&path) {
            Ok(value) => Ok(Some(value)),
            Err(StatefsError::NotFound) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn put(
        &mut self,
        sender_service_id: u64,
        key: &[u8],
        value: &[u8],
    ) -> Result<(), StatefsError> {
        let path = self.key_path(sender_service_id, key)?;
        self.client.put(&path, value)
    }

    fn delete(&mut self, sender_service_id: u64, key: &[u8]) -> Result<bool, StatefsError> {
        let path = self.key_path(sender_service_id, key)?;
        match self.client.delete(&path) {
            Ok(()) => Ok(true),
            Err(StatefsError::NotFound) => Ok(false),
            Err(err) => Err(err),
        }
    }

    fn load_device_key(&mut self) -> Result<Option<[u8; 32]>, StatefsError> {
        match self.client.get(STATEFS_DEVICE_KEY_PATH) {
            Ok(bytes) => {
                if bytes.len() != 32 {
                    return Err(StatefsError::Corrupted);
                }
                let mut out = [0u8; 32];
                out.copy_from_slice(&bytes);
                Ok(Some(out))
            }
            Err(StatefsError::NotFound) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn store_device_key(&mut self, key_bytes: &[u8; 32]) -> Result<(), StatefsError> {
        self.client.put(STATEFS_DEVICE_KEY_PATH, key_bytes)
    }

    fn key_path(&self, sender_service_id: u64, key: &[u8]) -> Result<String, StatefsError> {
        if key.is_empty() || key.len() > MAX_KEY_LEN {
            return Err(StatefsError::KeyTooLong);
        }
        let mut path = String::with_capacity(STATEFS_KEY_PREFIX.len() + 16 + 1 + key.len() * 2);
        path.push_str(STATEFS_KEY_PREFIX);
        push_hex_u64(&mut path, sender_service_id);
        path.push('/');
        push_hex_bytes(&mut path, key);
        if path.len() > statefs::MAX_KEY_LEN {
            return Err(StatefsError::KeyTooLong);
        }
        Ok(path)
    }
}

/// Device identity keypair storage.
/// Stores the signing key (private) and allows deriving the verifying key (public).
struct DeviceKeyPair {
    signing_key: Option<ed25519_dalek::SigningKey>,
}

impl DeviceKeyPair {
    const fn new() -> Self {
        Self { signing_key: None }
    }

    fn load_from_bytes(&mut self, bytes: [u8; 32]) {
        self.signing_key = Some(ed25519_dalek::SigningKey::from_bytes(&bytes));
    }

    fn is_generated(&self) -> bool {
        self.signing_key.is_some()
    }

    fn generate(&mut self, entropy: &[u8; 32]) -> bool {
        if self.signing_key.is_some() {
            return false; // Already generated
        }
        self.signing_key = Some(ed25519_dalek::SigningKey::from_bytes(entropy));
        true
    }

    fn public_key(&self) -> Option<[u8; 32]> {
        self.signing_key.as_ref().map(|sk| sk.verifying_key().to_bytes())
    }

    fn sign(&self, message: &[u8]) -> Option<[u8; 64]> {
        use ed25519_dalek::Signer;
        self.signing_key.as_ref().map(|sk| sk.sign(message).to_bytes())
    }
}

/// Main service loop; notifies readiness and yields cooperatively.
pub fn service_main_loop(notifier: ReadyNotifier) -> LiteResult<()> {
    // Signal readiness to init as early as possible; init sequences service spawns based on this.
    notifier.notify();
    // Emit readiness marker early to keep `scripts/qemu-test.sh` marker ordering stable.
    // The service may still need to wait for late-bound slots before handling some operations.
    emit_line("keystored: ready");
    let server = route_keystored_blocking().ok_or(ServerError::Unsupported("ipc route failed"))?;
    // Identity-binding hardening (bring-up semantics):
    //
    // This keystore implementation is a bring-up shim. To avoid cross-service key collisions and
    // "ambient" overwrites, we scope keys to the kernel-derived sender service identity.
    //
    // NOTE: this is not a full policy model; it is a safety floor until policyd-mediated access
    // control is wired for the keystore protocol.
    let mut store = KeyStore::new();
    // Device identity keypair (OS-lite bring-up).
    let mut device_keypair = DeviceKeyPair::new();
    if let Some(bytes) = store.device_key_bytes() {
        device_keypair.load_from_bytes(bytes);
    }
    let mut logged_capmove = false;
    let mut logged_capmove_req = false;
    let mut pending_replies: ReplyBuffer<16, 512> = ReplyBuffer::new();
    loop {
        match server.recv_request_with_meta(Wait::Blocking) {
            Ok((frame, sender_service_id, reply)) => {
                if reply.is_some() && !logged_capmove {
                    emit_line("keystored: capmove seen");
                    logged_capmove = true;
                }
                if !logged_capmove_req {
                    if frame.len() >= 7
                        && frame[0] == MAGIC0
                        && frame[1] == MAGIC1
                        && frame[2] == VERSION
                        && frame[3] == OP_GET
                    {
                        let key_len = frame[4] as usize;
                        let val_len = u16::from_le_bytes([frame[5], frame[6]]) as usize;
                        let total = 7usize.saturating_add(key_len).saturating_add(val_len);
                        if frame.len() == total {
                            let key_start = 7;
                            let key_end = key_start + key_len;
                            if frame.get(key_start..key_end) == Some(b"capmove.miss") {
                                emit_line("keystored: capmove req");
                                logged_capmove_req = true;
                            }
                        }
                    }
                }
                let rsp = handle_frame(
                    &mut store,
                    &mut device_keypair,
                    &mut pending_replies,
                    sender_service_id,
                    frame.as_slice(),
                );
                if let Some(reply) = reply {
                    let _ = reply.reply_and_close(&rsp);
                } else {
                    let _ = server.send(&rsp, Wait::Blocking);
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(nexus_ipc::IpcError::Disconnected) => {
                emit_line("keystored: recv disconnected");
                return Err(ServerError::Unsupported("ipc"));
            }
            Err(nexus_ipc::IpcError::NoSpace) => {
                emit_line("keystored: recv nospace");
                return Err(ServerError::Unsupported("ipc"));
            }
            Err(nexus_ipc::IpcError::Unsupported) => {
                emit_line("keystored: recv unsupported");
                return Err(ServerError::Unsupported("ipc"));
            }
            Err(nexus_ipc::IpcError::Kernel(_)) => {
                emit_line("keystored: recv kernel");
                return Err(ServerError::Unsupported("ipc"));
            }
            Err(_) => {
                emit_line("keystored: recv other");
                return Err(ServerError::Unsupported("ipc"));
            }
        }
    }
}

fn route_keystored_blocking() -> Option<KernelServer> {
    // init-lite wires service slots after spawn; avoid crashing if we race that wiring.
    // Keep this silent (no slot-spam) but bounded.
    const RECV_SLOT: u32 = 0x03;
    const SEND_SLOT: u32 = 0x04;
    let deadline = match nexus_abi::nsec() {
        Ok(now) => now.saturating_add(10_000_000_000), // 10s
        Err(_) => 0,
    };
    loop {
        // `KernelServer::new_with_slots` does not validate capability presence; probe via cap_clone.
        let recv_ok = nexus_abi::cap_clone(RECV_SLOT).map(|tmp| nexus_abi::cap_close(tmp)).is_ok();
        let send_ok = nexus_abi::cap_clone(SEND_SLOT).map(|tmp| nexus_abi::cap_close(tmp)).is_ok();
        if recv_ok && send_ok {
            return KernelServer::new_with_slots(RECV_SLOT, SEND_SLOT).ok();
        }
        if deadline != 0 {
            if let Ok(now) = nexus_abi::nsec() {
                if now >= deadline {
                    return None;
                }
            }
        }
        let _ = yield_();
    }
}

const MAGIC0: u8 = b'K';
const MAGIC1: u8 = b'S';
const VERSION: u8 = 1;

const OP_PUT: u8 = 1;
const OP_GET: u8 = 2;
const OP_DEL: u8 = 3;
const OP_VERIFY: u8 = 4;
const OP_SIGN: u8 = 5;
// Device identity key operations
const OP_DEVICE_KEYGEN: u8 = 10;
const OP_GET_DEVICE_PUBKEY: u8 = 11;
const OP_DEVICE_SIGN: u8 = 12;
const OP_GET_DEVICE_PRIVKEY: u8 = 13;
const OP_DEVICE_RELOAD: u8 = 14;

const STATUS_OK: u8 = 0;
const STATUS_NOT_FOUND: u8 = 1;
const STATUS_MALFORMED: u8 = 2;
const STATUS_TOO_LARGE: u8 = 3;
const STATUS_UNSUPPORTED: u8 = 4;
const STATUS_DENY: u8 = 5;
// Device identity key status codes
const STATUS_KEY_EXISTS: u8 = 10;
const STATUS_KEY_NOT_FOUND: u8 = 11;
#[allow(dead_code)] // Used in test_reject_device_key_private_export
const STATUS_PRIVATE_EXPORT_DENIED: u8 = 12;

const MAX_KEY_LEN: usize = 64;
const MAX_VAL_LEN: usize = 256;
const MAX_VERIFY_PAYLOAD: usize = 1 * 1024 * 1024;
const MAX_SIGN_PAYLOAD: usize = 1 * 1024 * 1024;

const STATEFS_KEY_PREFIX: &str = "/state/keystore/";
const STATEFS_DEVICE_KEY_PATH: &str = "/state/keystore/device.signing";

fn handle_frame(
    store: &mut KeyStore,
    device_keypair: &mut DeviceKeyPair,
    pending: &mut ReplyBuffer<16, 512>,
    sender_service_id: u64,
    frame: &[u8],
) -> Vec<u8> {
    // Request: [K, S, ver, op, ...]
    if frame.len() < 4 || frame[0] != MAGIC0 || frame[1] != MAGIC1 {
        return rsp(OP_GET, STATUS_MALFORMED, &[]);
    }
    let ver = frame[2];
    let op = frame[3];
    if ver != VERSION {
        return rsp(op, STATUS_UNSUPPORTED, &[]);
    }
    if op == OP_VERIFY {
        return handle_verify(frame);
    }
    if op == OP_SIGN {
        return handle_sign(pending, sender_service_id, frame);
    }
    // Device identity key operations
    if op == OP_DEVICE_KEYGEN {
        return handle_device_keygen(pending, device_keypair, store, sender_service_id);
    }
    if op == OP_GET_DEVICE_PUBKEY {
        return handle_get_device_pubkey(pending, device_keypair, store, sender_service_id);
    }
    if op == OP_DEVICE_SIGN {
        return handle_device_sign(pending, device_keypair, store, sender_service_id, frame);
    }
    if op == OP_GET_DEVICE_PRIVKEY {
        return handle_get_device_privkey();
    }
    if op == OP_DEVICE_RELOAD {
        emit_line("keystored: rx reload");
        return handle_device_reload(pending, device_keypair, store, sender_service_id);
    }

    // For PUT/GET/DEL, require minimum frame length
    if frame.len() < 7 {
        return rsp(op, STATUS_MALFORMED, &[]);
    }

    let key_len = frame[4] as usize;
    let val_len = u16::from_le_bytes([frame[5], frame[6]]) as usize;
    let total = 7usize.saturating_add(key_len).saturating_add(val_len);
    if key_len == 0 || key_len > MAX_KEY_LEN || val_len > MAX_VAL_LEN || frame.len() != total {
        return rsp(
            op,
            if key_len > MAX_KEY_LEN || val_len > MAX_VAL_LEN {
                STATUS_TOO_LARGE
            } else {
                STATUS_MALFORMED
            },
            &[],
        );
    }
    let key_start = 7;
    let key_end = key_start + key_len;
    let val_start = key_end;
    let val_end = val_start + val_len;
    let key = &frame[key_start..key_end];
    let val = &frame[val_start..val_end];
    match op {
        OP_PUT => match store.put(sender_service_id, key, val) {
            Ok(()) => rsp(OP_PUT, STATUS_OK, &[]),
            Err(err) => rsp(OP_PUT, status_from_statefs_error(err), &[]),
        },
        OP_GET => match store.get(sender_service_id, key) {
            Ok(Some(v)) => rsp(OP_GET, STATUS_OK, &v),
            Ok(None) => rsp(OP_GET, STATUS_NOT_FOUND, &[]),
            Err(err) => rsp(OP_GET, status_from_statefs_error(err), &[]),
        },
        OP_DEL => match store.delete(sender_service_id, key) {
            Ok(true) => rsp(OP_DEL, STATUS_OK, &[]),
            Ok(false) => rsp(OP_DEL, STATUS_NOT_FOUND, &[]),
            Err(err) => rsp(OP_DEL, status_from_statefs_error(err), &[]),
        },
        _ => rsp(op, STATUS_UNSUPPORTED, &[]),
    }
}

fn handle_verify(frame: &[u8]) -> Vec<u8> {
    // VERIFY request:
    // [K, S, ver, OP_VERIFY, payload_len:u32le, pubkey(32), sig(64), payload...]
    const HEADER_LEN: usize = 4 + 4 + 32 + 64;
    if frame.len() < HEADER_LEN {
        return rsp(OP_VERIFY, STATUS_MALFORMED, &[]);
    }
    let payload_len = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]) as usize;
    if payload_len > MAX_VERIFY_PAYLOAD {
        return rsp(OP_VERIFY, STATUS_TOO_LARGE, &[]);
    }
    let expected = HEADER_LEN.saturating_add(payload_len);
    if frame.len() != expected {
        return rsp(OP_VERIFY, STATUS_MALFORMED, &[]);
    }
    let key_start = 8;
    let key_end = key_start + 32;
    let sig_start = key_end;
    let sig_end = sig_start + 64;
    let payload_start = sig_end;
    let payload_end = payload_start + payload_len;

    let mut pubkey = [0u8; 32];
    pubkey.copy_from_slice(&frame[key_start..key_end]);
    let mut signature = [0u8; 64];
    signature.copy_from_slice(&frame[sig_start..sig_end]);
    let payload = &frame[payload_start..payload_end];

    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    let key = match VerifyingKey::from_bytes(&pubkey) {
        Ok(key) => key,
        Err(_) => return rsp(OP_VERIFY, STATUS_MALFORMED, &[]),
    };
    let sig = Signature::from_bytes(&signature);
    let ok = key.verify(payload, &sig).is_ok();
    rsp(OP_VERIFY, STATUS_OK, &[if ok { 1 } else { 0 }])
}

fn handle_sign(
    pending: &mut ReplyBuffer<16, 512>,
    sender_service_id: u64,
    frame: &[u8],
) -> Vec<u8> {
    // SIGN request:
    // [K, S, ver, OP_SIGN, payload_len:u32le, payload...]
    const HEADER_LEN: usize = 4 + 4;
    if frame.len() < HEADER_LEN {
        return rsp(OP_SIGN, STATUS_MALFORMED, &[]);
    }
    let payload_len = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]) as usize;
    if payload_len > MAX_SIGN_PAYLOAD {
        return rsp(OP_SIGN, STATUS_TOO_LARGE, &[]);
    }
    let expected = HEADER_LEN.saturating_add(payload_len);
    if frame.len() != expected {
        return rsp(OP_SIGN, STATUS_MALFORMED, &[]);
    }

    if !policyd_allows(pending, sender_service_id, b"crypto.sign") {
        return rsp(OP_SIGN, STATUS_DENY, &[]);
    }

    // NOTE: Device-identity signing is handled via OP_DEVICE_SIGN.
    rsp(OP_SIGN, STATUS_UNSUPPORTED, &[])
}

fn policyd_allows(pending: &mut ReplyBuffer<16, 512>, subject_id: u64, cap: &[u8]) -> bool {
    const MAGIC0: u8 = b'P';
    const MAGIC1: u8 = b'O';
    const VERSION_V2: u8 = 2;
    // Delegated check: keystored is an enforcement point; policyd validates that keystored is allowed
    // to query policy for another subject id.
    const OP_CHECK_CAP_DELEGATED: u8 = 5;
    const STATUS_ALLOW: u8 = 0;

    // init-lite deterministic slots for keystored:
    // - reply inbox: recv=5, send=6
    // - policyd send cap: 9 (after log_req at slot 8)
    const POL_SEND_SLOT: u32 = 0x09;
    const REPLY_RECV_SLOT: u32 = 0x05;
    const REPLY_SEND_SLOT: u32 = 0x06;

    if cap.is_empty() || cap.len() > 48 {
        return false;
    }
    // v2 delegated CAP check request (nonce-correlated):
    // [P, O, ver=2, OP_CHECK_CAP_DELEGATED, nonce:u32le, subject_id:u64le, cap_len:u8, cap...]
    static NONCE: AtomicU32 = AtomicU32::new(1);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let mut frame = Vec::with_capacity(17 + cap.len());
    frame.push(MAGIC0);
    frame.push(MAGIC1);
    frame.push(VERSION_V2);
    frame.push(OP_CHECK_CAP_DELEGATED);
    frame.extend_from_slice(&nonce.to_le_bytes());
    frame.extend_from_slice(&subject_id.to_le_bytes());
    frame.push(cap.len() as u8);
    frame.extend_from_slice(cap);

    let send_slot = POL_SEND_SLOT;
    let reply_send_slot = REPLY_SEND_SLOT;
    let reply_recv_slot = REPLY_RECV_SLOT;
    let reply_send_clone = match nexus_abi::cap_clone(reply_send_slot) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let hdr = nexus_abi::MsgHeader::new(
        reply_send_clone,
        0,
        0,
        nexus_abi::ipc_hdr::CAP_MOVE,
        frame.len() as u32,
    );
    let start = match nexus_abi::nsec() {
        Ok(value) => value,
        Err(_) => return false,
    };
    let deadline = start.saturating_add(500_000_000);

    let mut i: usize = 0;
    loop {
        match nexus_abi::ipc_send_v1(send_slot, &hdr, &frame, nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => break,
            Err(nexus_abi::IpcError::QueueFull) => {
                if (i & 0x7f) == 0 {
                    let now = match nexus_abi::nsec() {
                        Ok(value) => value,
                        Err(_) => return false,
                    };
                    if now >= deadline {
                        let _ = nexus_abi::cap_close(reply_send_clone);
                        return false;
                    }
                }
                let _ = yield_();
            }
            Err(_) => return false,
        }
        i = i.wrapping_add(1);
    }
    // Best-effort close: keep local cap table bounded even though the cap was moved.
    let _ = nexus_abi::cap_close(reply_send_clone);

    struct ReplyInboxV1 {
        recv_slot: u32,
    }
    impl nexus_ipc::Client for ReplyInboxV1 {
        fn send(&self, _frame: &[u8], _wait: Wait) -> nexus_ipc::Result<()> {
            Err(nexus_ipc::IpcError::Unsupported)
        }
        fn recv(&self, _wait: Wait) -> nexus_ipc::Result<Vec<u8>> {
            let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
            let mut buf = [0u8; 512];
            match nexus_abi::ipc_recv_v1(
                self.recv_slot,
                &mut rh,
                &mut buf,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(n) => Ok(buf[..core::cmp::min(n as usize, buf.len())].to_vec()),
                Err(nexus_abi::IpcError::QueueEmpty) => Err(nexus_ipc::IpcError::WouldBlock),
                Err(other) => Err(nexus_ipc::IpcError::Kernel(other)),
            }
        }
    }

    let clock = OsClock;
    let deadline_ns = match deadline_after(&clock, Duration::from_millis(500)) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let inbox = ReplyInboxV1 { recv_slot: reply_recv_slot };
    let rsp = match recv_match_until(
        &clock,
        &inbox,
        pending,
        nonce as u64,
        deadline_ns,
        extract_shared_nonce_u32,
    ) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if rsp.len() != 10 || rsp[0] != MAGIC0 || rsp[1] != MAGIC1 || rsp[2] != VERSION_V2 {
        return false;
    }
    if rsp[3] != (OP_CHECK_CAP_DELEGATED | 0x80) {
        return false;
    }
    let got_nonce = u32::from_le_bytes([rsp[4], rsp[5], rsp[6], rsp[7]]);
    if got_nonce != nonce {
        return false;
    }
    rsp[8] == STATUS_ALLOW
}

fn extract_shared_nonce_u32(frame: &[u8]) -> Option<u64> {
    // policyd v2 delegated-cap reply:
    // [P,O,ver=2,OP|0x80, nonce:u32le, status:u8, _reserved:u8]
    if frame.len() == 10 && frame[0] == b'P' && frame[1] == b'O' && frame[2] == 2 {
        let nonce = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
        return Some(nonce as u64);
    }
    // rngd GET_ENTROPY reply:
    // [R,G,1,OP|0x80, STATUS, nonce:u32le, ...]
    if frame.len() >= 9 && frame[0] == b'R' && frame[1] == b'G' && frame[2] == 1 {
        let nonce = u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]);
        return Some(nonce as u64);
    }
    None
}

// =============================================================================
// Device identity key operations (keygen + pubkey + signing)
// =============================================================================

/// Handle DEVICE_KEYGEN request.
/// Generates a device identity keypair using entropy from rngd.
///
/// # Security
/// - Policy-gated via `device.keygen` capability
/// - Entropy is NOT logged
fn handle_device_keygen(
    pending: &mut ReplyBuffer<16, 512>,
    device_keypair: &mut DeviceKeyPair,
    store: &mut KeyStore,
    sender_service_id: u64,
) -> Vec<u8> {
    // Policy check: caller must have device.keygen capability
    if !policyd_allows(pending, sender_service_id, b"device.keygen") {
        return rsp(OP_DEVICE_KEYGEN, STATUS_DENY, &[]);
    }

    // Check if key already exists
    if device_keypair.is_generated() {
        return rsp(OP_DEVICE_KEYGEN, STATUS_KEY_EXISTS, &[]);
    }

    // Request entropy from rngd (entropy authority service).
    let entropy = match request_entropy_from_rngd(pending, 32) {
        Some(bytes) if bytes.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            arr
        }
        _ => {
            emit_line("keystored: entropy request failed");
            return rsp(OP_DEVICE_KEYGEN, STATUS_UNSUPPORTED, &[]);
        }
    };

    // Persist key bytes before acknowledging generation.
    if let Err(err) = store.set_device_key_bytes(entropy) {
        return rsp(OP_DEVICE_KEYGEN, status_from_statefs_error(err), &[]);
    }

    // Generate the keypair (in-memory cache).
    // SECURITY: Do NOT log entropy or private key bytes!
    if device_keypair.generate(&entropy) {
        emit_line("keystored: device key generated");
        rsp(OP_DEVICE_KEYGEN, STATUS_OK, &[])
    } else {
        rsp(OP_DEVICE_KEYGEN, STATUS_KEY_EXISTS, &[])
    }
}

#[cfg(test)]
fn handle_device_keygen_with<P, E>(
    device_keypair: &mut DeviceKeyPair,
    store: &mut KeyStore,
    sender_service_id: u64,
    policy_check: P,
    entropy_source: E,
) -> Vec<u8>
where
    P: FnOnce(u64) -> bool,
    E: FnOnce(usize) -> Option<Vec<u8>>,
{
    // Policy check: caller must have device.keygen capability
    if !policy_check(sender_service_id) {
        return rsp(OP_DEVICE_KEYGEN, STATUS_DENY, &[]);
    }

    // Check if key already exists
    if device_keypair.is_generated() {
        return rsp(OP_DEVICE_KEYGEN, STATUS_KEY_EXISTS, &[]);
    }

    // Request entropy from rngd (entropy authority service).
    let entropy = match entropy_source(32) {
        Some(bytes) if bytes.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            arr
        }
        _ => {
            emit_line("keystored: entropy request failed");
            return rsp(OP_DEVICE_KEYGEN, STATUS_UNSUPPORTED, &[]);
        }
    };

    // Persist key bytes before acknowledging generation.
    if let Err(err) = store.set_device_key_bytes(entropy) {
        return rsp(OP_DEVICE_KEYGEN, status_from_statefs_error(err), &[]);
    }

    // Generate the keypair (in-memory cache).
    // SECURITY: Do NOT log entropy or private key bytes!
    if device_keypair.generate(&entropy) {
        emit_line("keystored: device key generated");
        rsp(OP_DEVICE_KEYGEN, STATUS_OK, &[])
    } else {
        rsp(OP_DEVICE_KEYGEN, STATUS_KEY_EXISTS, &[])
    }
}

/// Handle GET_DEVICE_PUBKEY request.
/// Returns the device's public key (32 bytes).
///
/// # Security
/// - Policy-gated via `device.pubkey.read` capability
/// - Only returns public key, NEVER private key
fn handle_get_device_pubkey(
    pending: &mut ReplyBuffer<16, 512>,
    device_keypair: &mut DeviceKeyPair,
    store: &mut KeyStore,
    sender_service_id: u64,
) -> Vec<u8> {
    if !device_keypair.is_generated() {
        if let Some(bytes) = store.device_key_bytes() {
            device_keypair.load_from_bytes(bytes);
        }
    }
    handle_get_device_pubkey_with(device_keypair, sender_service_id, |sid| {
        policyd_allows(pending, sid, b"device.pubkey.read")
    })
}

fn handle_get_device_pubkey_with<P>(
    device_keypair: &DeviceKeyPair,
    sender_service_id: u64,
    policy_check: P,
) -> Vec<u8>
where
    P: FnOnce(u64) -> bool,
{
    // Policy check: caller must have device.pubkey.read capability
    if !policy_check(sender_service_id) {
        return rsp(OP_GET_DEVICE_PUBKEY, STATUS_DENY, &[]);
    }

    match device_keypair.public_key() {
        Some(pubkey) => rsp(OP_GET_DEVICE_PUBKEY, STATUS_OK, &pubkey),
        None => rsp(OP_GET_DEVICE_PUBKEY, STATUS_KEY_NOT_FOUND, &[]),
    }
}

/// Handle DEVICE_SIGN request.
/// Signs a payload with the device's private key.
///
/// # Security
/// - Policy-gated via `crypto.sign` capability
/// - Private key NEVER leaves keystored
/// - Only signature is returned
fn handle_device_sign(
    pending: &mut ReplyBuffer<16, 512>,
    device_keypair: &mut DeviceKeyPair,
    store: &mut KeyStore,
    sender_service_id: u64,
    frame: &[u8],
) -> Vec<u8> {
    // DEVICE_SIGN request: [K, S, ver, OP, payload_len:u32le, payload...]
    const HEADER_LEN: usize = 4 + 4;
    if frame.len() < HEADER_LEN {
        return rsp(OP_DEVICE_SIGN, STATUS_MALFORMED, &[]);
    }
    let payload_len = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]) as usize;
    if payload_len > MAX_SIGN_PAYLOAD {
        return rsp(OP_DEVICE_SIGN, STATUS_TOO_LARGE, &[]);
    }
    let expected = HEADER_LEN.saturating_add(payload_len);
    if frame.len() != expected {
        return rsp(OP_DEVICE_SIGN, STATUS_MALFORMED, &[]);
    }

    // Policy check: caller must have crypto.sign capability
    if !policyd_allows(pending, sender_service_id, b"crypto.sign") {
        return rsp(OP_DEVICE_SIGN, STATUS_DENY, &[]);
    }

    // Ensure key is loaded (if persisted).
    if !device_keypair.is_generated() {
        if let Some(bytes) = store.device_key_bytes() {
            device_keypair.load_from_bytes(bytes);
        }
    }
    if !device_keypair.is_generated() {
        return rsp(OP_DEVICE_SIGN, STATUS_KEY_NOT_FOUND, &[]);
    }

    let payload = &frame[HEADER_LEN..expected];
    match device_keypair.sign(payload) {
        Some(signature) => rsp(OP_DEVICE_SIGN, STATUS_OK, &signature),
        None => rsp(OP_DEVICE_SIGN, STATUS_KEY_NOT_FOUND, &[]),
    }
}

/// Handle GET_DEVICE_PRIVKEY request.
///
/// This operation is intentionally unsupported: private key export is forbidden.
fn handle_get_device_privkey() -> Vec<u8> {
    rsp(OP_GET_DEVICE_PRIVKEY, STATUS_PRIVATE_EXPORT_DENIED, &[])
}

/// Handle DEVICE_RELOAD request.
///
/// # Security
/// - Policy-gated via `device.key.reload` capability
fn handle_device_reload(
    pending: &mut ReplyBuffer<16, 512>,
    device_keypair: &mut DeviceKeyPair,
    store: &mut KeyStore,
    sender_service_id: u64,
) -> Vec<u8> {
    if !policyd_allows(pending, sender_service_id, b"device.key.reload") {
        emit_line("keystored: reload denied by policy");
        return rsp(OP_DEVICE_RELOAD, STATUS_DENY, &[]);
    }
    emit_line("keystored: reload policy ok");
    // Re-read from statefsd to prove persistence across reboots
    match store.reload_device_key() {
        Ok(Some(bytes)) => {
            device_keypair.load_from_bytes(bytes);
            rsp(OP_DEVICE_RELOAD, STATUS_OK, &[])
        }
        Ok(None) => rsp(OP_DEVICE_RELOAD, STATUS_KEY_NOT_FOUND, &[]),
        Err(_) => rsp(OP_DEVICE_RELOAD, STATUS_UNSUPPORTED, &[]),
    }
}

/// Request entropy from rngd service.
///
/// # Security
/// - Entropy bytes are NOT logged
fn request_entropy_from_rngd(pending: &mut ReplyBuffer<16, 512>, n: usize) -> Option<Vec<u8>> {
    if n == 0 || n > 256 {
        return None;
    }

    // Build rngd GET_ENTROPY request with nonce.
    // Request: [R, G, 1, OP_GET_ENTROPY=1, nonce:u32le, n:u16le]
    static NONCE: AtomicU32 = AtomicU32::new(0x1000);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let mut req = Vec::with_capacity(10);
    req.push(b'R'); // MAGIC0
    req.push(b'G'); // MAGIC1
    req.push(1); // VERSION
    req.push(1); // OP_GET_ENTROPY
    req.extend_from_slice(&nonce.to_le_bytes());
    req.extend_from_slice(&(n as u16).to_le_bytes());

    // init-lite deterministic slots for keystored:
    // - rngd send: 0x0A
    // - reply inbox: recv=0x05, send=0x06
    let rng_send_slot = 0x0a;
    let reply_send_slot = 0x06;
    let reply_recv_slot = 0x05;
    let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).ok()?;

    // Send request with CAP_MOVE reply cap so rngd can reply to us deterministically.
    let hdr = nexus_abi::MsgHeader::new(
        reply_send_clone,
        0,
        0,
        nexus_abi::ipc_hdr::CAP_MOVE,
        req.len() as u32,
    );

    // Send request
    let start = nexus_abi::nsec().ok()?;
    let deadline = start.saturating_add(500_000_000);

    let mut i: usize = 0;
    loop {
        match nexus_abi::ipc_send_v1(rng_send_slot, &hdr, &req, nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => break,
            Err(nexus_abi::IpcError::QueueFull) => {
                if (i & 0x7f) == 0 && nexus_abi::nsec().ok()? >= deadline {
                    let _ = nexus_abi::cap_close(reply_send_clone);
                    return None;
                }
                let _ = yield_();
            }
            Err(_) => return None,
        }
        i = i.wrapping_add(1);
    }

    struct ReplyInboxV1 {
        recv_slot: u32,
    }
    impl nexus_ipc::Client for ReplyInboxV1 {
        fn send(&self, _frame: &[u8], _wait: Wait) -> nexus_ipc::Result<()> {
            Err(nexus_ipc::IpcError::Unsupported)
        }
        fn recv(&self, _wait: Wait) -> nexus_ipc::Result<Vec<u8>> {
            let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
            let mut buf = [0u8; 512];
            match nexus_abi::ipc_recv_v1(
                self.recv_slot,
                &mut rh,
                &mut buf,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(n) => Ok(buf[..core::cmp::min(n as usize, buf.len())].to_vec()),
                Err(nexus_abi::IpcError::QueueEmpty) => Err(nexus_ipc::IpcError::WouldBlock),
                Err(other) => Err(nexus_ipc::IpcError::Kernel(other)),
            }
        }
    }
    let clock = OsClock;
    let deadline_ns = deadline_after(&clock, Duration::from_millis(500)).ok()?;
    let inbox = ReplyInboxV1 { recv_slot: reply_recv_slot };
    let rsp = recv_match_until(
        &clock,
        &inbox,
        pending,
        nonce as u64,
        deadline_ns,
        extract_shared_nonce_u32,
    )
    .ok()?;

    // Response: [R, G, 1, OP|0x80, STATUS, nonce:u32le, entropy...]
    if rsp.len() < 9 || rsp[0] != b'R' || rsp[1] != b'G' || rsp[2] != 1 {
        return None;
    }
    if rsp[3] != (1 | 0x80) || rsp[4] != 0 {
        return None;
    }
    let got_nonce = u32::from_le_bytes([rsp[5], rsp[6], rsp[7], rsp[8]]);
    if got_nonce != nonce {
        return None;
    }
    // SECURITY: Do NOT log entropy bytes!
    Some(rsp[9..].to_vec())
}
fn rsp(op: u8, status: u8, value: &[u8]) -> Vec<u8> {
    // Response: [K, S, ver, op|0x80, status, val_len:u16le, val...]
    let mut out = Vec::with_capacity(7 + value.len());
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(op | 0x80);
    out.push(status);
    let len: u16 = (value.len().min(u16::MAX as usize)) as u16;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&value[..len as usize]);
    out
}

fn status_from_statefs_error(err: StatefsError) -> u8 {
    match err {
        StatefsError::NotFound => STATUS_NOT_FOUND,
        StatefsError::AccessDenied => STATUS_DENY,
        StatefsError::ValueTooLarge | StatefsError::KeyTooLong => STATUS_TOO_LARGE,
        StatefsError::InvalidKey | StatefsError::Corrupted => STATUS_MALFORMED,
        StatefsError::IoError | StatefsError::ReplayLimitExceeded => STATUS_UNSUPPORTED,
    }
}

fn push_hex_u64(out: &mut String, value: u64) {
    for shift in (0..16).rev() {
        let nibble = ((value >> (shift * 4)) & 0xF) as u8;
        let ch = if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) };
        out.push(ch as char);
    }
}

fn push_hex_bytes(out: &mut String, bytes: &[u8]) {
    for byte in bytes {
        let high = (byte >> 4) & 0xF;
        let low = byte & 0xF;
        out.push(if high < 10 { (b'0' + high) as char } else { (b'a' + (high - 10)) as char });
        out.push(if low < 10 { (b'0' + low) as char } else { (b'a' + (low - 10)) as char });
    }
}

/// Touches schema types to keep host parity; no-op in the stub.
pub fn touch_schemas() {}

fn emit_line(message: &str) {
    for byte in message.as_bytes().iter().copied().chain(core::iter::once(b'\n')) {
        let _ = debug_putc(byte);
    }
}

#[cfg(all(test, nexus_env = "os", feature = "os-lite"))]
mod tests {
    use super::*;

    fn rsp_status(frame: Vec<u8>) -> u8 {
        assert!(frame.len() >= 5);
        frame[4]
    }

    #[test]
    fn test_reject_sign_without_policy() {
        let payload = vec![0u8; 8];
        let mut frame = Vec::new();
        frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_SIGN]);
        frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        frame.extend_from_slice(&payload);

        let sender_service_id = nexus_abi::service_id_from_name(b"demo.testsvc");
        let mut pending: ReplyBuffer<16, 512> = ReplyBuffer::new();
        let out = handle_sign(&mut pending, sender_service_id, &frame);
        assert_eq!(rsp_status(out), STATUS_DENY);
    }

    #[test]
    fn test_reject_sign_oversized_payload() {
        let payload_len = (MAX_SIGN_PAYLOAD as u32).saturating_add(1);
        let mut frame = Vec::new();
        frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_SIGN]);
        frame.extend_from_slice(&payload_len.to_le_bytes());
        let sender_service_id = nexus_abi::service_id_from_name(b"samgrd");
        let mut pending: ReplyBuffer<16, 512> = ReplyBuffer::new();
        let out = handle_sign(&mut pending, sender_service_id, &frame);
        assert_eq!(rsp_status(out), STATUS_TOO_LARGE);
    }

    #[test]
    fn test_reject_device_key_private_export() {
        // Private export operation must deterministically reject.
        let out = handle_get_device_privkey();
        assert_eq!(rsp_status(out), STATUS_PRIVATE_EXPORT_DENIED);
    }

    #[test]
    fn test_device_keygen_denied_by_policy() {
        let mut keypair = DeviceKeyPair::new();
        let mut store = KeyStore::new_memory();
        let out = handle_device_keygen_with(
            &mut keypair,
            &mut store,
            nexus_abi::service_id_from_name(b"demo.testsvc"),
            |_| false,
            |_| Some(vec![0u8; 32]),
        );
        assert_eq!(rsp_status(out), STATUS_DENY);
        assert!(!keypair.is_generated());
    }

    #[test]
    fn test_device_keygen_entropy_unavailable() {
        let mut keypair = DeviceKeyPair::new();
        let mut store = KeyStore::new_memory();
        let out = handle_device_keygen_with(
            &mut keypair,
            &mut store,
            nexus_abi::service_id_from_name(b"selftest-client"),
            |_| true,
            |_| None,
        );
        assert_eq!(rsp_status(out), STATUS_UNSUPPORTED);
        assert!(!keypair.is_generated());
    }

    #[test]
    fn test_device_keygen_success_and_pubkey() {
        let mut keypair = DeviceKeyPair::new();
        let mut store = KeyStore::new_memory();
        let out = handle_device_keygen_with(
            &mut keypair,
            &mut store,
            nexus_abi::service_id_from_name(b"selftest-client"),
            |_| true,
            |_| Some(vec![0x5a; 32]),
        );
        assert_eq!(rsp_status(out), STATUS_OK);
        assert!(keypair.is_generated());
        let pubkey_out = handle_get_device_pubkey_with(
            &keypair,
            nexus_abi::service_id_from_name(b"selftest-client"),
            |_| true,
        );
        assert_eq!(rsp_status(pubkey_out.clone()), STATUS_OK);
        let val_len = u16::from_le_bytes([pubkey_out[5], pubkey_out[6]]) as usize;
        assert_eq!(val_len, 32);
    }

    #[test]
    fn test_device_keygen_idempotent() {
        let mut keypair = DeviceKeyPair::new();
        let mut store = KeyStore::new_memory();
        let _ = handle_device_keygen_with(
            &mut keypair,
            &mut store,
            nexus_abi::service_id_from_name(b"selftest-client"),
            |_| true,
            |_| Some(vec![0x11; 32]),
        );
        let out = handle_device_keygen_with(
            &mut keypair,
            &mut store,
            nexus_abi::service_id_from_name(b"selftest-client"),
            |_| true,
            |_| Some(vec![0x22; 32]),
        );
        assert_eq!(rsp_status(out), STATUS_KEY_EXISTS);
    }

    #[test]
    fn test_device_pubkey_denied_by_policy() {
        let keypair = DeviceKeyPair::new();
        let out = handle_get_device_pubkey_with(
            &keypair,
            nexus_abi::service_id_from_name(b"demo.testsvc"),
            |_| false,
        );
        assert_eq!(rsp_status(out), STATUS_DENY);
    }

    #[test]
    fn test_device_pubkey_missing_key() {
        let keypair = DeviceKeyPair::new();
        let out = handle_get_device_pubkey_with(
            &keypair,
            nexus_abi::service_id_from_name(b"selftest-client"),
            |_| true,
        );
        assert_eq!(rsp_status(out), STATUS_KEY_NOT_FOUND);
    }
}
