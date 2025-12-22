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
        Self {
            _marker: PhantomData,
        }
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
    Err(ServerError::Unsupported(
        "keystored run_with_transport_default_anchors",
    ))
}

/// Main service loop; notifies readiness and yields cooperatively.
pub fn service_main_loop(notifier: ReadyNotifier) -> LiteResult<()> {
    notifier.notify();
    emit_line("keystored: ready");
    let server = KernelServer::new_for("keystored").map_err(|_| ServerError::Unsupported("ipc"))?;
    let mut store: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    loop {
        match server.recv_request(Wait::Blocking) {
            Ok((frame, reply)) => {
                let rsp = handle_frame(&mut store, frame.as_slice());
                if let Some(reply) = reply {
                    let _ = reply.reply_and_close(&rsp);
                } else {
                    let _ = server.send(&rsp, Wait::Blocking);
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = yield_();
            }
            Err(nexus_ipc::IpcError::Disconnected) => return Err(ServerError::Unsupported("ipc")),
            Err(_) => return Err(ServerError::Unsupported("ipc")),
        }
    }
}

const MAGIC0: u8 = b'K';
const MAGIC1: u8 = b'S';
const VERSION: u8 = 1;

const OP_PUT: u8 = 1;
const OP_GET: u8 = 2;
const OP_DEL: u8 = 3;

const STATUS_OK: u8 = 0;
const STATUS_NOT_FOUND: u8 = 1;
const STATUS_MALFORMED: u8 = 2;
const STATUS_TOO_LARGE: u8 = 3;
const STATUS_UNSUPPORTED: u8 = 4;

const MAX_KEY_LEN: usize = 64;
const MAX_VAL_LEN: usize = 256;

fn handle_frame(store: &mut BTreeMap<Vec<u8>, Vec<u8>>, frame: &[u8]) -> Vec<u8> {
    // Request: [K, S, ver, op, key_len:u8, val_len:u16le, key..., val...]
    if frame.len() < 7 || frame[0] != MAGIC0 || frame[1] != MAGIC1 {
        return rsp(OP_GET, STATUS_MALFORMED, &[]);
    }
    let ver = frame[2];
    let op = frame[3];
    if ver != VERSION {
        return rsp(op, STATUS_UNSUPPORTED, &[]);
    }
    let key_len = frame[4] as usize;
    let val_len = u16::from_le_bytes([frame[5], frame[6]]) as usize;
    let total = 7usize
        .saturating_add(key_len)
        .saturating_add(val_len);
    if key_len == 0 || key_len > MAX_KEY_LEN || val_len > MAX_VAL_LEN || frame.len() != total {
        return rsp(op, if key_len > MAX_KEY_LEN || val_len > MAX_VAL_LEN { STATUS_TOO_LARGE } else { STATUS_MALFORMED }, &[]);
    }
    let key_start = 7;
    let key_end = key_start + key_len;
    let val_start = key_end;
    let val_end = val_start + val_len;
    let key = &frame[key_start..key_end];
    let val = &frame[val_start..val_end];

    match op {
        OP_PUT => {
            store.insert(key.to_vec(), val.to_vec());
            rsp(OP_PUT, STATUS_OK, &[])
        }
        OP_GET => match store.get(key) {
            Some(v) => rsp(OP_GET, STATUS_OK, v),
            None => rsp(OP_GET, STATUS_NOT_FOUND, &[]),
        },
        OP_DEL => {
            let existed = store.remove(key).is_some();
            rsp(OP_DEL, if existed { STATUS_OK } else { STATUS_NOT_FOUND }, &[])
        }
        _ => rsp(op, STATUS_UNSUPPORTED, &[]),
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
    for byte in message
        .as_bytes()
        .iter()
        .copied()
        .chain(core::iter::once(b'\n'))
    {
        let _ = debug_putc(byte);
    }
}
