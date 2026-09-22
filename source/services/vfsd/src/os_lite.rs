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
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use core::fmt;

use nexus_abi;
use nexus_ipc::{IpcError, KernelServer, Server, Wait};

use crate::namespace::Namespace;
use crate::SandboxError;

const OPCODE_STAT: u8 = 4;
const OPCODE_OPEN: u8 = 1;
const OPCODE_READ: u8 = 2;
const OPCODE_CLOSE: u8 = 3;
const OPCODE_READDIR: u8 = 6;

// packagefsd's op byte space (see its os_lite dispatch). Metadata, listing,
// the inline tier, and the VMO pass-through (TASK-0033 P2).
pub(crate) const PKGFS_OPCODE_RESOLVE: u8 = 2;
/// packagefsd's list opcode (see packagefsd os_lite dispatch).
pub(crate) const PKGFS_OPCODE_LIST: u8 = 4;
pub(crate) const PKGFS_OPCODE_ARM_VMO: u8 = 5;
pub(crate) const PKGFS_OPCODE_READ_VMO: u8 = 6;
pub(crate) const PKGFS_OPCODE_READ: u8 = 7;

/// Reply scratch for the packagefsd hop (metadata, listing pages, inline
/// reads). ONE buffer for the service lifetime — the os-lite heap never frees.
/// It no longer has to hold file payloads: bulk moves through a VMO.
const PKGFS_REPLY_BUF: usize = nexus_abi::IPC_PAYLOAD_MAX;

pub(crate) const KIND_FILE: u16 = 0;

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
    /// The entry is above the inline tier: it has a VMO path and must use it.
    TooBig,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport => write!(f, "transport error"),
            Self::InvalidPath => write!(f, "invalid path"),
            Self::NotFound => write!(f, "entry not found"),
            Self::BadHandle => write!(f, "invalid file handle"),
            Self::TooBig => write!(f, "entry above the inline tier"),
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

/// One entry's METADATA. Not its bytes: those move through the inline read op
/// or, above `INLINE_IO_MAX`, through the caller's VMO (RFC-0097 §3).
#[derive(Clone)]
pub(crate) struct Entry {
    pub(crate) kind: u16,
    pub(crate) size: u64,
}

/// An open `pkg:/` file. It holds the PATH, not the content — vfsd used to keep
/// a `Vec` the size of the file per open handle, on a heap that never frees.
pub(crate) struct FileHandle {
    owner_service_id: u64,
    path: String,
    size: u64,
}

/// Runs the cooperative vfsd loop and emits a readiness marker once.
pub fn service_main_loop<F: FnOnce() + Send>(notifier: ReadyNotifier<F>) -> Result<()> {
    // Marker contract: emit only after the IPC endpoint exists.
    let _ = nexus_service_entry::ready("vfsd: ready");
    debug_print("vfsd: namespace ready\n");
    notifier.notify();
    // The declared server pair (TASK-0324 P4), pinned before this task runs. No route ask at
    // start-up (P7-b): an ask has no clock and init may be blocked in a synchronous exchange
    // with a service that, in turn, waits for THIS server — the ask made that a deadlock.
    let slots = nexus_service_topology::slots::vfsd::SERVER;
    let server =
        KernelServer::new_with_slots(slots.recv, slots.send).map_err(|_| Error::Transport)?;
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
pub(crate) fn is_home_path(path: &str) -> bool {
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
pub(crate) fn vmo_len(slot: u32) -> Option<usize> {
    let mut query = nexus_abi::CapQuery { kind_tag: 0, irq: 0, base: 0, len: 0 };
    if nexus_abi::cap_query(slot, &mut query).is_err() || query.kind_tag != 1 {
        return None;
    }
    Some(query.len as usize)
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
    // TASK-0179: ONE reusable 64 KiB splice window for the lifetime of the
    // service. The bump heap never frees, so a per-request window buffer
    // would leak the size of every file ever spliced.
    let mut splice_window = alloc::vec![0u8; 64 * 1024];
    // ONE request buffer for the service lifetime: the os-lite heap never frees, so an
    // allocating recv is a countdown (TASK-0054C P2-g). Transport-capped, never truncates.
    let mut recv_frame = alloc::vec![0u8; nexus_abi::IPC_PAYLOAD_MAX];
    // ONE reply scratch for every packagefsd hop, for the same reason.
    let mut pkg_scratch = alloc::vec![0u8; PKGFS_REPLY_BUF];
    loop {
        // CAP_MOVE-aware receive: app-host children move a one-shot reply cap
        // into the request (their private inbox); direct clients (selftest)
        // send plainly and read the shared response endpoint. Replying on the
        // wrong path silently strands the caller — route per message.
        match server.recv_request_with_meta_into(Wait::Blocking, &mut recv_frame) {
            Ok((frame_len, sender_service_id, reply_cap)) => {
                let frame = &recv_frame[..frame_len];
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
                    crate::splice_os::handle_read_vmo(
                        &frame,
                        vmo_slot,
                        &namespace,
                        &mut data,
                        &mut data_attempts,
                        MAX_DATA_ATTEMPTS,
                        &mut splice_bytes,
                        &mut splice_fallbacks,
                        &mut splice_window,
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
                        match namespace.stat(path, &mut pkg_scratch) {
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
                        match namespace.open(path, &mut pkg_scratch) {
                            Ok(entry) => {
                                let fh = next_handle;
                                next_handle = next_handle.wrapping_add(1).max(1);
                                handles.insert(
                                    fh,
                                    FileHandle {
                                        owner_service_id: sender_service_id,
                                        path: path.to_string(),
                                        size: entry.size,
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
                                Some(handle)
                                    if handle.owner_service_id == sender_service_id
                                        && handle.size as usize
                                            > nexus_vfs_types::INLINE_IO_MAX =>
                                {
                                    // The entry is above the inline tier: E2BIG,
                                    // and it belongs on OP_READ_VMO (RFC-0097 §3).
                                    // `open` already told us the size, so this
                                    // costs no round trip and never truncates.
                                    reply.push(2);
                                }
                                Some(handle) if handle.owner_service_id == sender_service_id => {
                                    // The handle names the entry; packagefsd serves
                                    // the bytes.
                                    match namespace
                                        .packagefs_read_inline(&handle.path, &mut pkg_scratch)
                                    {
                                        Ok(n) => {
                                            let start = off.min(n as u64) as usize;
                                            let end = start.saturating_add(len as usize).min(n);
                                            reply.push(1);
                                            reply.extend_from_slice(&pkg_scratch[start..end]);
                                        }
                                        Err(Error::TooBig) => reply.push(2),
                                        Err(_) => reply.push(0),
                                    }
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
                        let (reply, entries) = namespace.read_dir(&frame[1..], &mut pkg_scratch);
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

pub(crate) fn debug_print(_s: &str) {
    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    let _ = nexus_abi::debug_write(_s.as_bytes());
}

// raw UART helper removed in favor of debug_write syscall

pub(crate) fn map_namespace_error(err: SandboxError) -> Error {
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
