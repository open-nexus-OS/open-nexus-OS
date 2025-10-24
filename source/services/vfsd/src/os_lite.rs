use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use nexus_ipc::{IpcError, LiteServer, Wait};

const OPCODE_STAT: u8 = 4;
const OPCODE_OPEN: u8 = 1;
const OPCODE_READ: u8 = 2;
const OPCODE_CLOSE: u8 = 3;

const KIND_FILE: u16 = 0;
const KIND_DIRECTORY: u16 = 1;

/// Result type returned by the os-lite backend.
pub type Result<T> = core::result::Result<T, Error>;

/// Errors produced by the os-lite backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Transport,
    InvalidPath,
    NotFound,
    BadHandle,
}

pub struct ReadyNotifier<F: FnOnce() + Send>(F);

impl<F: FnOnce() + Send> ReadyNotifier<F> {
    pub fn new(func: F) -> Self {
        Self(func)
    }

    pub fn notify(self) {
        (self.0)();
    }
}

#[derive(Default)]
struct Namespace {
    bundles: BTreeMap<String, Bundle>,
}

impl Namespace {
    fn stat(&self, path: &str) -> Result<Entry> {
        let entry = self.resolve(path)?.clone();
        Ok(entry)
    }

    fn open(&self, path: &str) -> Result<FileHandle> {
        let entry = self.resolve(path)?;
        if entry.kind != KIND_FILE {
            return Err(Error::InvalidPath);
        }
        Ok(FileHandle { bytes: entry.bytes.clone(), kind: entry.kind })
    }

    fn resolve(&self, path: &str) -> Result<&Entry> {
        let path = path.strip_prefix("pkg:/").ok_or(Error::InvalidPath)?;
        let (bundle, rest) = path.split_once('/').ok_or(Error::InvalidPath)?;
        let (bundle_name, version) = if let Some((name, ver)) = bundle.split_once('@') {
            (name, ver)
        } else {
            let bundle = self.bundles.get(bundle).ok_or(Error::NotFound)?;
            return bundle.entries.get(rest).ok_or(Error::NotFound);
        };
        let key = format!("{bundle_name}@{version}");
        self.bundles
            .get(&key)
            .and_then(|bundle| bundle.entries.get(rest))
            .ok_or(Error::NotFound)
    }
}

#[derive(Default)]
struct Bundle {
    entries: BTreeMap<String, Entry>,
}

#[derive(Clone)]
struct Entry {
    kind: u16,
    size: u64,
    bytes: Vec<u8>,
}

struct FileHandle {
    bytes: Vec<u8>,
    kind: u16,
}

pub fn service_main_loop<F: FnOnce() + Send>(notifier: ReadyNotifier<F>) -> Result<()> {
    debug_print("vfsd: ready\n");
    notifier.notify();
    let server = LiteServer::new().map_err(|_| Error::Transport)?;
    let namespace = seed_namespace();
    run_loop(server, namespace)
}

fn run_loop(server: LiteServer, namespace: Namespace) -> Result<()> {
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
                            frame[5], frame[6], frame[7], frame[8], frame[9], frame[10], frame[11], frame[12],
                        ]);
                        let len = u32::from_le_bytes([frame[13], frame[14], frame[15], frame[16]]);
                        let mut reply = Vec::new();
                        match handles.get(&fh) {
                            Some(handle) => {
                                let start = off.min(handle.bytes.len() as u64) as usize;
                                let end = start.saturating_add(len as usize).min(handle.bytes.len());
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

fn seed_namespace() -> Namespace {
    let mut namespace = Namespace::default();

    let mut hello_entries = BTreeMap::new();
    hello_entries.insert(
        "manifest.json".to_string(),
        Entry { kind: KIND_FILE, size: 64, bytes: b"{\"name\":\"demo.hello\"}".to_vec() },
    );
    hello_entries.insert(
        "payload.elf".to_string(),
        Entry { kind: KIND_FILE, size: HELLO_ELF.len() as u64, bytes: HELLO_ELF.to_vec() },
    );
    hello_entries.insert(".".to_string(), Entry { kind: KIND_DIRECTORY, size: 0, bytes: Vec::new() });
    namespace.bundles.insert(
        "demo.hello@1.0.0".to_string(),
        Bundle { entries: hello_entries },
    );

    let mut exit_entries = BTreeMap::new();
    exit_entries.insert(
        "manifest.json".to_string(),
        Entry { kind: KIND_FILE, size: 48, bytes: b"{\"name\":\"demo.exit0\"}".to_vec() },
    );
    exit_entries.insert(
        "payload.elf".to_string(),
        Entry { kind: KIND_FILE, size: EXIT_ELF.len() as u64, bytes: EXIT_ELF.to_vec() },
    );
    exit_entries.insert(".".to_string(), Entry { kind: KIND_DIRECTORY, size: 0, bytes: Vec::new() });
    namespace.bundles.insert(
        "demo.exit0@1.0.0".to_string(),
        Bundle { entries: exit_entries },
    );

    namespace
}

const HELLO_ELF: &[u8] = b"HELLO_ELF_PAYLOAD";
const EXIT_ELF: &[u8] = b"EXIT0_ELF_PAYLOAD";

fn debug_print(s: &str) {
    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    let _ = nexus_abi::debug_write(s.as_bytes());
}

// raw UART helper removed in favor of debug_write syscall
