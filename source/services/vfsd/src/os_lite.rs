extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use core::fmt;

use nexus_abi;
use nexus_ipc::{Client, IpcError, KernelClient, KernelServer, Server, Wait};

const OPCODE_STAT: u8 = 4;
const OPCODE_OPEN: u8 = 1;
const OPCODE_READ: u8 = 2;
const OPCODE_CLOSE: u8 = 3;

const KIND_FILE: u16 = 0;

/// Result type returned by the os-lite backend.
pub type Result<T> = core::result::Result<T, Error>;

/// Errors produced by the os-lite backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Mailbox transport failure.
    Transport,
    /// Path does not match `pkg:/bundle@version/path`.
    InvalidPath,
    /// Bundle or entry missing from the namespace.
    NotFound,
    /// File handle referenced after it was closed.
    BadHandle,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport => write!(f, "transport error"),
            Self::InvalidPath => write!(f, "invalid path"),
            Self::NotFound => write!(f, "entry not found"),
            Self::BadHandle => write!(f, "invalid file handle"),
        }
    }
}

/// Signals init-lite once the service is ready.
pub struct ReadyNotifier<F: FnOnce() + Send>(F);

impl<F: FnOnce() + Send> ReadyNotifier<F> {
    /// Creates a notifier from the provided closure.
    pub fn new(func: F) -> Self {
        Self(func)
    }

    /// Invokes the stored closure to emit readiness.
    pub fn notify(self) {
        (self.0)();
    }
}

#[derive(Default)]
struct Namespace;

impl Namespace {
    fn packagefs_resolve(&self, path: &str) -> Result<Entry> {
        // Forward resolution to packagefsd over IPC (real data path).
        const PKGFS_OPCODE_RESOLVE: u8 = 2;
        let rel = path.strip_prefix("pkg:/").ok_or(Error::InvalidPath)?;
        let client = KernelClient::new_for("packagefsd").map_err(|_| Error::Transport)?;
        let mut frame = Vec::with_capacity(1 + rel.len());
        frame.push(PKGFS_OPCODE_RESOLVE);
        frame.extend_from_slice(rel.as_bytes());
        client.send(&frame, Wait::Blocking).map_err(|_| Error::Transport)?;
        let rsp = client.recv(Wait::Blocking).map_err(|_| Error::Transport)?;
        if rsp.len() < 1 + 8 + 2 || rsp[0] != 1 {
            return Err(Error::NotFound);
        }
        let size =
            u64::from_le_bytes([rsp[1], rsp[2], rsp[3], rsp[4], rsp[5], rsp[6], rsp[7], rsp[8]]);
        let kind = u16::from_le_bytes([rsp[9], rsp[10]]);
        let bytes = rsp[11..].to_vec();
        Ok(Entry { kind, size, bytes })
    }

    fn stat(&self, path: &str) -> Result<Entry> {
        // Prefer real data from packagefsd for pkg:/ paths.
        if path.starts_with("pkg:/") {
            return self.packagefs_resolve(path);
        }
        Err(Error::InvalidPath)
    }

    fn open(&self, path: &str) -> Result<FileHandle> {
        // Prefer real data from packagefsd for pkg:/ paths.
        let entry = if path.starts_with("pkg:/") {
            self.packagefs_resolve(path)?
        } else {
            return Err(Error::InvalidPath);
        };
        if entry.kind != KIND_FILE {
            return Err(Error::InvalidPath);
        }
        Ok(FileHandle { bytes: entry.bytes })
    }
}

#[derive(Clone)]
struct Entry {
    kind: u16,
    size: u64,
    bytes: Vec<u8>,
}

struct FileHandle {
    bytes: Vec<u8>,
}

/// Runs the cooperative vfsd loop and emits a readiness marker once.
pub fn service_main_loop<F: FnOnce() + Send>(notifier: ReadyNotifier<F>) -> Result<()> {
    debug_print("vfsd: ready\n");
    notifier.notify();
    // RFC-0005: For kernel IPC v1, init transfers vfs request/reply endpoints into deterministic
    // slots. Use name-based construction so call sites don't hardcode slot numbers.
    let server = KernelServer::new_for("vfsd").map_err(|_| Error::Transport)?;
    // VFS bring-up: proxy pkg:/ reads to packagefsd (real data). Non-pkg schemes are unsupported.
    run_loop(server, Namespace::default())
}

fn run_loop(server: KernelServer, namespace: Namespace) -> Result<()> {
    let mut handles: BTreeMap<u32, FileHandle> = BTreeMap::new();
    let mut next_handle: u32 = 1;
    loop {
        match server.recv(Wait::Blocking) {
            Ok(frame) => {
                if frame.is_empty() {
                    continue;
                }
                let opcode = frame[0];
                match opcode {
                    OPCODE_STAT => {
                        let path = core::str::from_utf8(&frame[1..]).unwrap_or("");
                        let mut reply = Vec::new();
                        match namespace.stat(path) {
                            Ok(entry) => {
                                reply.push(1);
                                reply.extend_from_slice(&entry.size.to_le_bytes());
                                reply.extend_from_slice(&entry.kind.to_le_bytes());
                            }
                            Err(_) => {
                                reply.push(0);
                            }
                        }
                        server.send(&reply, Wait::Blocking).map_err(|_| Error::Transport)?;
                    }
                    OPCODE_OPEN => {
                        let path = core::str::from_utf8(&frame[1..]).unwrap_or("");
                        let mut reply = Vec::new();
                        match namespace.open(path) {
                            Ok(handle) => {
                                let fh = next_handle;
                                next_handle = next_handle.wrapping_add(1).max(1);
                                handles.insert(fh, handle);
                                reply.push(1);
                                reply.extend_from_slice(&fh.to_le_bytes());
                            }
                            Err(_) => reply.push(0),
                        }
                        server.send(&reply, Wait::Blocking).map_err(|_| Error::Transport)?;
                    }
                    OPCODE_READ => {
                        if frame.len() < 1 + 4 + 8 + 4 {
                            continue;
                        }
                        let fh = u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
                        let off = u64::from_le_bytes([
                            frame[5], frame[6], frame[7], frame[8], frame[9], frame[10], frame[11],
                            frame[12],
                        ]);
                        let len = u32::from_le_bytes([frame[13], frame[14], frame[15], frame[16]]);
                        let mut reply = Vec::new();
                        match handles.get(&fh) {
                            Some(handle) => {
                                let start = off.min(handle.bytes.len() as u64) as usize;
                                let end =
                                    start.saturating_add(len as usize).min(handle.bytes.len());
                                reply.push(1);
                                reply.extend_from_slice(&handle.bytes[start..end]);
                            }
                            None => reply.push(0),
                        }
                        server.send(&reply, Wait::Blocking).map_err(|_| Error::Transport)?;
                    }
                    OPCODE_CLOSE => {
                        if frame.len() < 5 {
                            continue;
                        }
                        let fh = u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
                        let mut reply = Vec::new();
                        if handles.remove(&fh).is_some() {
                            reply.push(1);
                        } else {
                            reply.push(0);
                        }
                        server.send(&reply, Wait::Blocking).map_err(|_| Error::Transport)?;
                    }
                    _ => {
                        let _ = nexus_abi::yield_();
                    }
                }
            }
            Err(IpcError::Disconnected) => return Err(Error::Transport),
            Err(IpcError::WouldBlock) | Err(IpcError::Timeout) => {
                let _ = nexus_abi::yield_();
            }
            Err(_) => return Err(Error::Transport),
        }
    }
}

fn debug_print(_s: &str) {
    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    let _ = nexus_abi::debug_write(_s.as_bytes());
}

// raw UART helper removed in favor of debug_write syscall
