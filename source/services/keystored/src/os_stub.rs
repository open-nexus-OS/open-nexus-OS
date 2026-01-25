extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use core::fmt;
use core::marker::PhantomData;

use nexus_abi::{debug_putc, yield_};
use nexus_ipc::{KernelServer, Server as _, Wait};

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

/// Main service loop; notifies readiness and yields cooperatively.
pub fn service_main_loop(notifier: ReadyNotifier) -> LiteResult<()> {
    notifier.notify();
    emit_line("keystored: ready");
    let server = route_keystored_blocking().ok_or(ServerError::Unsupported("ipc route failed"))?;
    // Identity-binding hardening (bring-up semantics):
    //
    // This keystore implementation is a bring-up shim. To avoid cross-service key collisions and
    // "ambient" overwrites, we scope keys to the kernel-derived sender service identity.
    //
    // NOTE: this is not a full policy model; it is a safety floor until policyd-mediated access
    // control is wired for the keystore protocol.
    let mut store: BTreeMap<(u64, Vec<u8>), Vec<u8>> = BTreeMap::new();
    let mut logged_capmove = false;
    let mut logged_capmove_req = false;
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
                let rsp = handle_frame(&mut store, sender_service_id, frame.as_slice());
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
    const CTRL_SEND_SLOT: u32 = 1;
    const CTRL_RECV_SLOT: u32 = 2;
    let name = b"keystored";
    let mut req = [0u8; 5 + nexus_abi::routing::MAX_SERVICE_NAME_LEN];
    let req_len = nexus_abi::routing::encode_route_get(name, &mut req)?;
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, req_len as u32);
    loop {
        if nexus_abi::ipc_send_v1(CTRL_SEND_SLOT, &hdr, &req[..req_len], 0, 0).is_err() {
            let _ = yield_();
            continue;
        }
        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 32];
        match nexus_abi::ipc_recv_v1(
            CTRL_RECV_SLOT,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = n as usize;
                let (status, send_slot, recv_slot) =
                    nexus_abi::routing::decode_route_rsp(&buf[..n])?;
                if status != nexus_abi::routing::STATUS_OK {
                    let _ = yield_();
                    continue;
                }
                return KernelServer::new_with_slots(recv_slot, send_slot).ok();
            }
            Err(_) => {
                let _ = yield_();
            }
        }
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

const STATUS_OK: u8 = 0;
const STATUS_NOT_FOUND: u8 = 1;
const STATUS_MALFORMED: u8 = 2;
const STATUS_TOO_LARGE: u8 = 3;
const STATUS_UNSUPPORTED: u8 = 4;
const STATUS_DENY: u8 = 5;

const MAX_KEY_LEN: usize = 64;
const MAX_VAL_LEN: usize = 256;
const MAX_VERIFY_PAYLOAD: usize = 1 * 1024 * 1024;
const MAX_SIGN_PAYLOAD: usize = 1 * 1024 * 1024;

fn handle_frame(
    store: &mut BTreeMap<(u64, Vec<u8>), Vec<u8>>,
    sender_service_id: u64,
    frame: &[u8],
) -> Vec<u8> {
    // Request: [K, S, ver, op, key_len:u8, val_len:u16le, key..., val...]
    if frame.len() < 7 || frame[0] != MAGIC0 || frame[1] != MAGIC1 {
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
        return handle_sign(sender_service_id, frame);
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
    let scoped_key = (sender_service_id, key.to_vec());

    match op {
        OP_PUT => {
            store.insert(scoped_key, val.to_vec());
            rsp(OP_PUT, STATUS_OK, &[])
        }
        OP_GET => match store.get(&scoped_key) {
            Some(v) => rsp(OP_GET, STATUS_OK, v),
            None => rsp(OP_GET, STATUS_NOT_FOUND, &[]),
        },
        OP_DEL => {
            let existed = store.remove(&scoped_key).is_some();
            rsp(OP_DEL, if existed { STATUS_OK } else { STATUS_NOT_FOUND }, &[])
        }
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

fn handle_sign(sender_service_id: u64, frame: &[u8]) -> Vec<u8> {
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

    if !policyd_allows(sender_service_id, b"crypto.sign") {
        return rsp(OP_SIGN, STATUS_DENY, &[]);
    }

    // NOTE: Real device keys and signing are handled in TASK-0008B.
    rsp(OP_SIGN, STATUS_UNSUPPORTED, &[])
}

fn policyd_allows(subject_id: u64, cap: &[u8]) -> bool {
    const MAGIC0: u8 = b'P';
    const MAGIC1: u8 = b'O';
    const VERSION: u8 = 1;
    const OP_CHECK_CAP: u8 = nexus_abi::policyd::OP_CHECK_CAP;
    const STATUS_ALLOW: u8 = 0;

    if cap.is_empty() || cap.len() > 48 {
        return false;
    }
    let mut frame = Vec::with_capacity(13 + cap.len());
    frame.push(MAGIC0);
    frame.push(MAGIC1);
    frame.push(VERSION);
    frame.push(OP_CHECK_CAP);
    frame.extend_from_slice(&subject_id.to_le_bytes());
    frame.push(cap.len() as u8);
    frame.extend_from_slice(cap);

    let client = match nexus_ipc::KernelClient::new_for("policyd") {
        Ok(client) => client,
        Err(_) => return false,
    };
    let (send_slot, recv_slot) = client.slots();
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
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
                        return false;
                    }
                }
                let _ = yield_();
            }
            Err(_) => return false,
        }
        i = i.wrapping_add(1);
    }

    let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 16];
    let mut j: usize = 0;
    loop {
        if (j & 0x7f) == 0 {
            let now = match nexus_abi::nsec() {
                Ok(value) => value,
                Err(_) => return false,
            };
            if now >= deadline {
                return false;
            }
        }
        match nexus_abi::ipc_recv_v1(
            recv_slot,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                if n != 6 || buf[0] != MAGIC0 || buf[1] != MAGIC1 || buf[2] != VERSION {
                    continue;
                }
                if buf[3] != (OP_CHECK_CAP | 0x80) {
                    continue;
                }
                return buf[4] == STATUS_ALLOW;
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = yield_();
            }
            Err(_) => return false,
        }
        j = j.wrapping_add(1);
    }
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
        let out = handle_sign(sender_service_id, &frame);
        assert_eq!(rsp_status(out), STATUS_DENY);
    }

    #[test]
    fn test_reject_sign_oversized_payload() {
        let payload_len = (MAX_SIGN_PAYLOAD as u32).saturating_add(1);
        let mut frame = Vec::new();
        frame.extend_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_SIGN]);
        frame.extend_from_slice(&payload_len.to_le_bytes());
        let sender_service_id = nexus_abi::service_id_from_name(b"samgrd");
        let out = handle_sign(sender_service_id, &frame);
        assert_eq!(rsp_status(out), STATUS_TOO_LARGE);
    }
}
