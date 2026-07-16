// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OS-lite backend for the vfsd (virtual filesystem daemon). Provides stat, open,
//! read, and close operations over kernel IPC, forwarding pkg:/ resolution to packagefsd
//! for real data and enforcing namespace view constraints.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//! ADR: docs/adr/0017-service-architecture.md

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use core::fmt;

use nexus_abi;
use nexus_ipc::{Client, IpcError, KernelClient, KernelServer, Server, Wait};
use nexus_vfs_types::{
    decode_readdir_response, encode_readdir_error, encode_readdir_request, VfsError,
};

use crate::{NamespaceView, SandboxError};

const OPCODE_STAT: u8 = 4;
const OPCODE_OPEN: u8 = 1;
const OPCODE_READ: u8 = 2;
const OPCODE_CLOSE: u8 = 3;
const OPCODE_READDIR: u8 = 6;

/// packagefsd's list opcode (see packagefsd os_lite dispatch).
const PKGFS_OPCODE_LIST: u8 = 4;

/// Bulk reply scratch for the packagefsd hop (file payloads + listing pages);
/// stays under the 8 KiB IPC frame cap.
const PKGFS_REPLY_BUF: usize = 8 * 1024;

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

struct Namespace {
    view: NamespaceView,
}

impl Namespace {
    fn new() -> Self {
        Self { view: NamespaceView::new(vec!["pkg:/".to_string()]) }
    }

    fn packagefs_resolve(&self, path: &str) -> Result<Entry> {
        // Forward resolution to packagefsd over IPC (real data path).
        const PKGFS_OPCODE_RESOLVE: u8 = 2;
        let canonical = self.view.assert_allowed(path).map_err(map_namespace_error)?;
        let rel = canonical.strip_prefix("pkg:/").ok_or(Error::InvalidPath)?;
        let client = KernelClient::new_for("packagefsd").map_err(|_| Error::Transport)?;
        let mut frame = Vec::with_capacity(1 + rel.len());
        frame.push(PKGFS_OPCODE_RESOLVE);
        frame.extend_from_slice(rel.as_bytes());
        client.send(&frame, Wait::Blocking).map_err(|_| Error::Transport)?;
        // Bulk-capable recv: the default client recv truncates at 512 bytes,
        // which silently corrupts multi-KB payloads.
        let mut buf = vec![0u8; PKGFS_REPLY_BUF];
        let n = client.recv_into(Wait::Blocking, &mut buf).map_err(|_| Error::Transport)?;
        let rsp = &buf[..n];
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
        Ok(FileHandle { owner_service_id: 0, bytes: entry.bytes })
    }

    /// Relays a ReadDir request to packagefsd and returns the validated reply
    /// payload (shared `nexus-vfs-types` codec on both hops). The returned
    /// payload is sent to the caller verbatim; errors are already encoded.
    fn read_dir(&self, request_payload: &[u8]) -> (Vec<u8>, Option<usize>) {
        let request = match nexus_vfs_types::decode_readdir_request(request_payload) {
            Ok(request) => request,
            Err(err) => return (encode_readdir_error(err), None),
        };
        // Namespace: only pkg:/ paths exist in os-lite; "pkg:/" is the root.
        let rel = if request.path == "pkg:/" {
            ".".to_string()
        } else {
            let canonical = match self.view.assert_allowed(&request.path) {
                Ok(canonical) => canonical,
                Err(_) => {
                    debug_print("vfsd: access denied\n");
                    return (encode_readdir_error(VfsError::Access), None);
                }
            };
            match canonical.strip_prefix("pkg:/") {
                Some(rel) if !rel.is_empty() => rel.to_string(),
                _ => return (encode_readdir_error(VfsError::Invalid), None),
            }
        };
        let forwarded = match encode_readdir_request(&rel, request.cursor, request.limit) {
            Ok(payload) => payload,
            Err(err) => return (encode_readdir_error(err), None),
        };
        let client = match KernelClient::new_for("packagefsd") {
            Ok(client) => client,
            Err(_) => return (encode_readdir_error(VfsError::Io), None),
        };
        let mut frame = Vec::with_capacity(1 + forwarded.len());
        frame.push(PKGFS_OPCODE_LIST);
        frame.extend_from_slice(&forwarded);
        if client.send(&frame, Wait::Blocking).is_err() {
            return (encode_readdir_error(VfsError::Io), None);
        }
        let mut buf = vec![0u8; PKGFS_REPLY_BUF];
        let n = match client.recv_into(Wait::Blocking, &mut buf) {
            Ok(n) => n,
            Err(_) => return (encode_readdir_error(VfsError::Io), None),
        };
        buf.truncate(n);
        // Validate before relaying: a malformed provider page must surface as
        // EIO here, never reach the app client half-broken.
        match decode_readdir_response(&buf) {
            Ok(page) => {
                let count = page.entries.len();
                (buf, Some(count))
            }
            Err(err) => (encode_readdir_error(err), None),
        }
    }
}

#[derive(Clone)]
struct Entry {
    kind: u16,
    size: u64,
    bytes: Vec<u8>,
}

struct FileHandle {
    owner_service_id: u64,
    bytes: Vec<u8>,
}

/// Runs the cooperative vfsd loop and emits a readiness marker once.
pub fn service_main_loop<F: FnOnce() + Send>(notifier: ReadyNotifier<F>) -> Result<()> {
    // Marker contract: emit only after the IPC endpoint exists.
    debug_print("vfsd: ready\n");
    debug_print("vfsd: namespace ready\n");
    notifier.notify();
    // RFC-0005: For kernel IPC v1, init transfers vfs request/reply endpoints into deterministic
    // slots. Use name-based construction so call sites don't hardcode slot numbers.
    let server = match KernelServer::new_for("vfsd") {
        Ok(server) => server,
        Err(_) => KernelServer::new_with_slots(3, 4).map_err(|_| Error::Transport)?,
    };
    // VFS bring-up: proxy pkg:/ reads to packagefsd (real data). Non-pkg schemes are unsupported.
    run_loop(server, Namespace::new())
}

/// True if the frame targets the writable user **home** (the nxfs container):
/// any write op (packagefs is read-only, so writes are always home), or a
/// STAT/READDIR whose path is not a read-only `pkg:/` path. The home IS the
/// root — `/`, `/Bilder`, … — so anything that is not `pkg:` is home.
fn targets_home(frame: &[u8]) -> bool {
    use nexus_vfs_types::fileops::{
        OP_COPY, OP_CREATE, OP_MKDIR, OP_REMOVE, OP_RENAME, OP_WRITE_TEXT,
    };
    match frame.first().copied() {
        Some(OP_MKDIR | OP_CREATE | OP_WRITE_TEXT | OP_REMOVE | OP_RENAME | OP_COPY) => true,
        Some(OPCODE_STAT) => is_home_path(core::str::from_utf8(&frame[1..]).unwrap_or("")),
        Some(OPCODE_READDIR) if frame.len() > 7 => {
            is_home_path(core::str::from_utf8(&frame[7..]).unwrap_or(""))
        }
        _ => false,
    }
}

/// A path belongs to the user home (nxfs) unless it is a read-only package path.
fn is_home_path(path: &str) -> bool {
    !path.starts_with("pkg:")
}

/// Reply for a home op when the store is not yet mounted (honest, never fake).
fn data_unavailable(opcode: u8) -> Vec<u8> {
    match opcode {
        OPCODE_READDIR => nxfsd::readdir_unavailable(),
        OPCODE_STAT => nxfsd::stat_unavailable(),
        _ => nxfsd::write_unavailable(),
    }
}

/// Queries a VMO capability's byte length (RFC-0040 `cap_query`, `kind_tag` 1 =
/// VMO). `None` when the slot is not a VMO the caller granted.
fn vmo_len(slot: u32) -> Option<usize> {
    let mut query = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
    if nexus_abi::cap_query(slot, &mut query).is_err() || query.kind_tag != 1 {
        return None;
    }
    Some(query.len as usize)
}

/// Writes the splice header at VMO offset 0. Call this AFTER the payload write
/// (release ordering): a client that sees the magic must see complete bytes.
fn write_splice_header(vmo: u32, status: u16, len: u32) {
    let hdr = nexus_vfs_types::encode_splice_header(status, len);
    let _ = nexus_abi::vmo_write(vmo, 0, &hdr);
}

/// Serves an `OP_READ_VMO` request (RFC-0072 Phase 3): resolve the path to
/// bytes (nxfs `/data` or read-only `pkg:/`), write them into the caller's
/// moved VMO payload-first + header-last, then close the moved cap. The header
/// carries the RFC-0072 status; oversize-for-VMO is `E2BIG`, never truncated.
#[allow(clippy::too_many_arguments)]
fn handle_read_vmo(
    frame: &[u8],
    vmo_slot: Option<u32>,
    namespace: &Namespace,
    data: &mut Option<nxfsd::DataStore>,
    data_attempts: &mut u8,
    max_data_attempts: u8,
    splice_bytes: &mut u64,
    splice_fallbacks: &mut u64,
) {
    let Some(vmo) = vmo_slot else {
        *splice_fallbacks += 1;
        debug_print("vfsd: FAIL splice (no vmo cap)\n");
        return;
    };
    let path = match nexus_vfs_types::decode_read_vmo_request(&frame[1..]) {
        Some(path) => path,
        None => {
            write_splice_header(vmo, VfsError::Invalid.code(), 0);
            let _ = nexus_abi::cap_close(vmo);
            return;
        }
    };
    // The caller's VMO capacity bounds the payload (minus the header prefix).
    let max_payload = match vmo_len(vmo) {
        Some(cap) if cap > nexus_vfs_types::SPLICE_DATA_OFFSET => {
            cap - nexus_vfs_types::SPLICE_DATA_OFFSET
        }
        _ => {
            *splice_fallbacks += 1;
            write_splice_header(vmo, VfsError::Io.code(), 0);
            let _ = nexus_abi::cap_close(vmo);
            return;
        }
    };
    // Resolve bytes from the owning provider (one surface, two providers).
    let bytes: core::result::Result<Vec<u8>, VfsError> = if is_home_path(&path) {
        if data.is_none() && *data_attempts < max_data_attempts {
            *data_attempts += 1;
            *data = nxfsd::DataStore::acquire();
        }
        match data.as_ref() {
            Some(store) => store.read_bytes(&path, max_payload),
            None => Err(VfsError::Io),
        }
    } else if path.starts_with("pkg:/") {
        namespace.open(&path).map(|handle| handle.bytes).map_err(|_| VfsError::NotFound)
    } else {
        Err(VfsError::NotFound)
    };
    match bytes {
        Ok(bytes) if bytes.len() <= max_payload => {
            // Payload FIRST, header LAST — the release fence for the poller.
            if nexus_abi::vmo_write(vmo, nexus_vfs_types::SPLICE_DATA_OFFSET, &bytes).is_ok() {
                write_splice_header(vmo, nexus_vfs_types::CODE_OK, bytes.len() as u32);
                *splice_bytes = splice_bytes.saturating_add(bytes.len() as u64);
                debug_print(&format!(
                    "vfsd: vmo splice read ok (bytes={}, fallbacks={})\n",
                    bytes.len(),
                    *splice_fallbacks
                ));
            } else {
                *splice_fallbacks += 1;
                write_splice_header(vmo, VfsError::Io.code(), 0);
            }
        }
        Ok(_) => {
            // Bytes exceed the caller's VMO — E2BIG, never a partial read.
            write_splice_header(vmo, VfsError::TooBig.code(), 0);
        }
        Err(err) => write_splice_header(vmo, err.code(), 0),
    }
    let _ = nexus_abi::cap_close(vmo);
}

fn run_loop(server: KernelServer, namespace: Namespace) -> Result<()> {
    let mut handles: BTreeMap<u32, FileHandle> = BTreeMap::new();
    let mut next_handle: u32 = 1;
    // Lazily-acquired user-data store (RFC-0071 nxfs on the data device). The
    // MMIO grant may land after the server endpoint, so retry on demand.
    let mut data: Option<nxfsd::DataStore> = None;
    let mut data_attempts: u8 = 0;
    const MAX_DATA_ATTEMPTS: u8 = 8;
    // Zero-copy read accounting (RFC-0072 Phase 3): total bytes moved through a
    // VMO and the number of reads that could NOT splice (honest fallback count).
    let mut splice_bytes: u64 = 0;
    let mut splice_fallbacks: u64 = 0;
    loop {
        // CAP_MOVE-aware receive: app-host children move a one-shot reply cap
        // into the request (their private inbox); direct clients (selftest)
        // send plainly and read the shared response endpoint. Replying on the
        // wrong path silently strands the caller — route per message.
        match server.recv_request_with_meta(Wait::Blocking) {
            Ok((frame, sender_service_id, reply_cap)) => {
                if frame.is_empty() {
                    if let Some(reply_cap) = reply_cap {
                        reply_cap.close();
                    }
                    continue;
                }
                let opcode = frame[0];
                // Zero-copy read (RFC-0072 Phase 3): the moved cap IS the
                // caller's VMO (CAP_MOVE, not a reply endpoint — the GET_PAYLOAD
                // handoff). vfsd fills it and the header it writes into the VMO
                // is the reply (header-last); there is no frame reply.
                if opcode == nexus_vfs_types::OP_READ_VMO {
                    let vmo_slot = reply_cap.map(|cap| {
                        let slot = cap.slot();
                        core::mem::forget(cap);
                        slot
                    });
                    handle_read_vmo(
                        &frame,
                        vmo_slot,
                        &namespace,
                        &mut data,
                        &mut data_attempts,
                        MAX_DATA_ATTEMPTS,
                        &mut splice_bytes,
                        &mut splice_fallbacks,
                    );
                    continue;
                }
                // Writable `/data` mount: route to the in-process nxfs store
                // (RFC-0072 Phase 2). Everything else is the read-only pkg path.
                if targets_home(&frame) {
                    if data.is_none() && data_attempts < MAX_DATA_ATTEMPTS {
                        data_attempts += 1;
                        data = nxfsd::DataStore::acquire();
                    }
                    let reply = match data.as_mut() {
                        Some(store) => {
                            let out = store.handle(&frame);
                            if opcode == OPCODE_READDIR {
                                debug_print("vfsd: readdir ok (mount=home)\n");
                            }
                            out
                        }
                        None => data_unavailable(opcode),
                    };
                    match reply_cap {
                        Some(reply_cap) => {
                            let _ = reply_cap.reply_and_close_wait(&reply, Wait::Blocking);
                        }
                        None => {
                            server.send(&reply, Wait::Blocking).map_err(|_| Error::Transport)?;
                        }
                    }
                    continue;
                }
                let reply: Vec<u8> = match opcode {
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
                                debug_print("vfsd: access denied\n");
                                reply.push(0);
                            }
                        }
                        reply
                    }
                    OPCODE_OPEN => {
                        let path = core::str::from_utf8(&frame[1..]).unwrap_or("");
                        let mut reply = Vec::new();
                        match namespace.open(path) {
                            Ok(handle) => {
                                let fh = next_handle;
                                next_handle = next_handle.wrapping_add(1).max(1);
                                handles.insert(
                                    fh,
                                    FileHandle {
                                        owner_service_id: sender_service_id,
                                        bytes: handle.bytes,
                                    },
                                );
                                reply.push(1);
                                reply.extend_from_slice(&fh.to_le_bytes());
                                debug_print("vfsd: capfd grant ok\n");
                            }
                            Err(_) => {
                                debug_print("vfsd: access denied\n");
                                reply.push(0);
                            }
                        }
                        reply
                    }
                    OPCODE_READ => {
                        if frame.len() < 1 + 4 + 8 + 4 {
                            if let Some(reply_cap) = reply_cap {
                                reply_cap.close();
                            }
                            continue;
                        }
                        let fh = u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
                        let off = u64::from_le_bytes([
                            frame[5], frame[6], frame[7], frame[8], frame[9], frame[10], frame[11],
                            frame[12],
                        ]);
                        let len = u32::from_le_bytes([frame[13], frame[14], frame[15], frame[16]]);
                        let mut reply = Vec::new();
                        if len as usize > nexus_vfs_types::INLINE_IO_MAX {
                            // Inline reads are capped at INLINE_IO_MAX; a larger
                            // read must use OP_READ_VMO (RFC-0072 Phase 3). Reply
                            // sentinel `2` = E2BIG — never a silent truncation.
                            reply.push(2);
                        } else {
                            match handles.get(&fh) {
                                Some(handle) if handle.owner_service_id == sender_service_id => {
                                    let start = off.min(handle.bytes.len() as u64) as usize;
                                    let end =
                                        start.saturating_add(len as usize).min(handle.bytes.len());
                                    reply.push(1);
                                    reply.extend_from_slice(&handle.bytes[start..end]);
                                }
                                Some(_) => {
                                    debug_print("vfsd: access denied\n");
                                    reply.push(0);
                                }
                                None => reply.push(0),
                            }
                        }
                        reply
                    }
                    OPCODE_CLOSE => {
                        if frame.len() < 5 {
                            if let Some(reply_cap) = reply_cap {
                                reply_cap.close();
                            }
                            continue;
                        }
                        let fh = u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]);
                        let mut reply = Vec::new();
                        match handles.get(&fh) {
                            Some(handle) if handle.owner_service_id == sender_service_id => {
                                let _ = handles.remove(&fh);
                                reply.push(1);
                            }
                            Some(_) => {
                                debug_print("vfsd: access denied\n");
                                reply.push(0);
                            }
                            None => {
                                reply.push(0);
                            }
                        }
                        reply
                    }
                    OPCODE_READDIR => {
                        let (reply, entries) = namespace.read_dir(&frame[1..]);
                        if let Some(count) = entries {
                            debug_print(&format!(
                                "vfsd: readdir ok (mount=/packages entries={count})\n"
                            ));
                        }
                        reply
                    }
                    _ => {
                        if let Some(reply_cap) = reply_cap {
                            reply_cap.close();
                        }
                        let _ = nexus_abi::yield_();
                        continue;
                    }
                };
                match reply_cap {
                    Some(reply_cap) => {
                        let _ = reply_cap.reply_and_close_wait(&reply, Wait::Blocking);
                    }
                    None => {
                        server.send(&reply, Wait::Blocking).map_err(|_| Error::Transport)?;
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

fn map_namespace_error(err: SandboxError) -> Error {
    #[cfg(all(nexus_env = "os", feature = "os-lite"))]
    {
        let _ = err;
        return Error::InvalidPath;
    }
    #[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
    match err {
        SandboxError::InvalidPath | SandboxError::Traversal | SandboxError::OutOfNamespace => {
            Error::InvalidPath
        }
        _ => Error::Transport,
    }
}
