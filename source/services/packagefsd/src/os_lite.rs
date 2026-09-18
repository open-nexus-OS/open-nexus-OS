// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: OS-lite packagefs daemon path — `pkg:/` is the VERIFIED SYSTEM
//! VOLUME (TASK-0321 P5, RFC-0089 §12, ADR-0060): the bundle index comes
//! from bundlemgrd (`GET_INDEX`, the NXSV-bound bytes it verified) and file
//! bytes are fetched on demand (`GET_FILE_VMO`, digest-checked by the
//! authority, header-last) through ONE reusable VMO. No RAM image, no
//! pkgimg v2 transcode. Without a verified volume (recovery / direct-kernel
//! boots) the seed registry mounts as `Legacy`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by single-VM QEMU marker ladder and selftest VFS phase.

extern crate alloc;

use core::fmt;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use nexus_ipc::{IpcError, KernelServer, Wait};
use nexus_vfs_types::{DirEntry, VfsError};

use crate::listing;
use crate::volume_reader::{load_registry_from_volume, VolumeReader};

// packagefsd's OWN op byte space (not the vfs surface's — vfsd translates).
/// Metadata for one entry: `[found u8][size u64le][kind u16le]`. NEVER bytes.
const OPCODE_RESOLVE: u8 = 2;
const OPCODE_MOUNT_STATUS: u8 = 3;
const OPCODE_LIST: u8 = 4;
/// Arms the moved VMO as the destination of this sender's next `OPCODE_READ_VMO`
/// (TASK-0033 P2). No reply: a message moves one capability, and for this op that
/// one is the VMO.
const OPCODE_ARM_VMO: u8 = 5;
/// Reads one entry into the armed VMO by forwarding it to bundlemgrd
/// (RFC-0097 §2). Reply `[status u16le][len u32le]`; the VMO carries the same
/// status in its header, written LAST by whoever wrote the bytes.
const OPCODE_READ_VMO: u8 = 6;
/// The inline tier: one whole entry at or below `INLINE_IO_MAX`. Reply
/// `[status u8][bytes…]`. A larger entry is `TooBig` — it has a VMO path.
const OPCODE_READ: u8 = 7;

/// The metadata reply for "no such entry": found = 0, size = 0, kind = 0.
const NOT_FOUND_RESOLVE_RSP: [u8; 11] = [0u8; 11];
const KIND_FILE: u16 = 0;
const KIND_DIRECTORY: u16 = 1;

const DEMO_HELLO_MANIFEST_NXB: &[u8] = exec_payloads::HELLO_MANIFEST_NXB;
const DEMO_HELLO_PAYLOAD: &[u8] = b"HELLO_PAYLOAD_BYTES";
const DEMO_EXIT_MANIFEST_NXB: &[u8] = exec_payloads::EXIT0_MANIFEST_NXB;
const DEMO_EXIT_PAYLOAD: &[u8] = b"EXIT0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MountMode {
    Legacy = 0,
    /// TASK-0321 P5: `pkg:/` = the verified system volume (index from
    /// bundlemgrd, files on demand). The pkgimg RAM modes (1..3) are retired.
    SystemVolume = 4,
}

/// Bounded index handoff (RFC-0089 §12.2: index ≤ 256 KiB).
pub(crate) const INDEX_VMO_BYTES: usize = nexus_abi::payload_vmo::DATA_OFFSET + 256 * 1024;

/// Result type used by the os-lite backend.
pub type LiteResult<T> = core::result::Result<T, LiteError>;

/// Errors surfaced by the os-lite backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiteError {
    /// IPC layer failed.
    Transport,
    /// Registry lookups failed.
    Registry,
}

impl fmt::Display for LiteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport => write!(f, "transport error"),
            Self::Registry => write!(f, "registry error"),
        }
    }
}

/// Ready notifier used by init.
pub struct ReadyNotifier<F: FnOnce() + Send>(F);

impl<F: FnOnce() + Send> ReadyNotifier<F> {
    /// Creates a notifier from the provided closure.
    pub fn new(func: F) -> Self {
        Self(func)
    }

    /// Emits the readiness signal.
    pub fn notify(self) {
        (self.0)();
    }
}

#[derive(Default)]
pub(crate) struct BundleRegistry {
    bundles: BTreeMap<String, BTreeMap<String, Entry>>, // bundle@version -> path -> entry
    pub(crate) active: BTreeMap<String, String>,        // bundle -> version
}

impl BundleRegistry {
    pub(crate) fn publish(&mut self, bundle: &str, version: &str, entries: &[(String, Entry)]) {
        let key = format!("{bundle}@{version}");
        let record = self.bundles.entry(key).or_default();
        record.clear();
        for (path, entry) in entries {
            record.insert(path.clone(), entry.clone());
        }
        self.active.insert(bundle.to_string(), version.to_string());
    }

    fn resolve(&self, rel: &str) -> Option<Entry> {
        let rel = rel.trim_start_matches('/');
        let (bundle, path) = rel.split_once('/')?;
        let canonical = if bundle.contains('@') {
            bundle.to_string()
        } else {
            let version = self.active.get(bundle)?;
            format!("{bundle}@{version}")
        };
        let entries = self.bundles.get(&canonical)?;
        entries.get(path).cloned()
    }

    /// Lists direct children of `rel` (`"."` = bundle roots) in canonical
    /// order; errors follow the RFC-0072 table (fail-closed).
    fn list(&self, rel: &str) -> core::result::Result<Vec<DirEntry>, VfsError> {
        let rel = rel.trim_start_matches('/');
        if rel.is_empty() || rel == listing::ROOT_REL {
            return Ok(listing::list_roots(self.active.keys().map(String::as_str)));
        }
        let (bundle, sub) = match rel.split_once('/') {
            Some((bundle, sub)) => (bundle, sub),
            None => (rel, ""),
        };
        let canonical = if bundle.contains('@') {
            bundle.to_string()
        } else {
            let version = self.active.get(bundle).ok_or(VfsError::NotFound)?;
            format!("{bundle}@{version}")
        };
        let entries = self.bundles.get(&canonical).ok_or(VfsError::NotFound)?;
        listing::list_children(
            entries.iter().map(|(path, entry)| (path.as_str(), entry.kind, entry.size)),
            sub,
        )
    }
}

#[derive(Clone)]
pub(crate) struct Entry {
    size: u64,
    kind: u16,
    source: Source,
}

/// Where an entry's bytes live: inline (seed registry) or on the system
/// volume, fetched on demand through bundlemgrd (digest-checked there).
#[derive(Clone)]
enum Source {
    Inline(Vec<u8>),
    Volume { bundle: String, path: String },
}

impl Entry {
    pub(crate) fn directory() -> Self {
        Self { size: 0, kind: KIND_DIRECTORY, source: Source::Inline(Vec::new()) }
    }

    fn file(bytes: &[u8]) -> Self {
        Self { size: bytes.len() as u64, kind: KIND_FILE, source: Source::Inline(bytes.to_vec()) }
    }

    pub(crate) fn volume_file(size: u64, bundle: &str, path: &str) -> Self {
        Self {
            size,
            kind: KIND_FILE,
            source: Source::Volume { bundle: bundle.to_string(), path: path.to_string() },
        }
    }
}

/// Runs the minimal packagefs daemon, emitting a readiness marker once.
pub fn service_main_loop<F: FnOnce() + Send>(notifier: ReadyNotifier<F>) -> LiteResult<()> {
    // Marker contract: emit only after the IPC endpoint exists.
    let _ = nexus_service_entry::ready("packagefsd: ready");
    notifier.notify();
    // The declared server pair (TASK-0324 P4): pinned before this task runs — no route ask.
    let slots = nexus_service_topology::slots::packagefsd::SERVER;
    let server =
        KernelServer::new_with_slots(slots.recv, slots.send).map_err(|_| LiteError::Transport)?;
    let (registry, mount_mode, reader) = match load_registry_from_volume() {
        Some((registry, reader)) => (registry, MountMode::SystemVolume, Some(reader)),
        None => {
            let (registry, mode) = seed_registry();
            (registry, mode, None)
        }
    };
    run_loop(&server, &registry, mount_mode, reader.as_ref())
}

/// packagefsd's ONE server step, for every op (TASK-0054C P5c shape).
///
/// A message moves ONE capability. For `OPCODE_ARM_VMO` that capability is the
/// caller's VMO — the destination of the read that follows — and there is no
/// answer. For every other op it is the reply channel, and the answer is parked
/// to ride out with the next receive.
fn run_loop(
    server: &KernelServer,
    registry: &BundleRegistry,
    mount_mode: MountMode,
    reader: Option<&VolumeReader>,
) -> LiteResult<()> {
    let mut pending = nexus_ipc::PendingReply::new();
    // ONE request buffer for the service lifetime: the os-lite heap never frees, so an
    // allocating recv is a countdown (TASK-0054C P2-g). Transport-capped, never truncates.
    let mut recv_frame = alloc::vec![0u8; nexus_abi::IPC_PAYLOAD_MAX];
    // Inline reads are bounded by the transfer contract (RFC-0072), so one buffer
    // serves every one of them for the service lifetime. BULK never lands here —
    // it is forwarded as a VMO and packagefsd sees none of it (RFC-0097 §2).
    let mut inline = alloc::vec![0u8; nexus_vfs_types::INLINE_IO_MAX];
    let mut response = Vec::with_capacity(256);
    let mut armed = nexus_ipc::armed_vmo::ArmedVmos::new();
    let mut forwarded: u32 = 0;
    loop {
        match server.serve_next(&mut pending, Wait::Blocking, &mut recv_frame) {
            Ok((_hdr, frame_len, sid, moved)) => {
                let bytes = &recv_frame[..frame_len];
                let Some(&op) = bytes.first() else {
                    if let Some(cap) = moved {
                        cap.close();
                    }
                    continue;
                };
                if op == OPCODE_ARM_VMO {
                    // The moved capability is the DESTINATION VMO, not a reply
                    // channel: take the raw slot and keep it for this sender's
                    // next read. No answer — the read that follows carries it.
                    let Some(cap) = moved else {
                        continue;
                    };
                    let slot = cap.slot();
                    core::mem::forget(cap);
                    match armed.arm(sid, slot) {
                        nexus_ipc::armed_vmo::Armed::Stored => {}
                        nexus_ipc::armed_vmo::Armed::Replaced(old)
                        | nexus_ipc::armed_vmo::Armed::Full(old) => {
                            let _ = nexus_abi::cap_close(old);
                        }
                    }
                    continue;
                }
                response.clear();
                match op {
                    OPCODE_RESOLVE => {
                        // METADATA ONLY. Entry bytes never ride a reply frame:
                        // that is what made 22 of the volume's 115 entries
                        // unreadable and fatal (TASK-0033, RFC-0097).
                        let entry = match core::str::from_utf8(&bytes[1..]) {
                            Ok(rel) => registry.resolve(rel),
                            Err(_) => None,
                        };
                        match entry {
                            Some(entry) => {
                                response.push(1);
                                response.extend_from_slice(&entry.size.to_le_bytes());
                                response.extend_from_slice(&entry.kind.to_le_bytes());
                            }
                            None => response.extend_from_slice(&NOT_FOUND_RESOLVE_RSP),
                        }
                    }
                    OPCODE_READ => {
                        // The inline tier: whole entries at or below
                        // `INLINE_IO_MAX`. A larger entry is `TooBig` — it has a
                        // VMO path and must use it, never a silent truncation.
                        let rel = core::str::from_utf8(&bytes[1..]).ok();
                        let entry = rel.and_then(|rel| registry.resolve(rel));
                        let read = entry.as_ref().and_then(|entry| {
                            if entry.kind != KIND_FILE || entry.size as usize > inline.len() {
                                return None;
                            }
                            match &entry.source {
                                Source::Inline(bytes) => {
                                    inline[..bytes.len()].copy_from_slice(bytes);
                                    Some(bytes.len())
                                }
                                Source::Volume { bundle, path } => reader.and_then(|r| {
                                    r.read_inline(bundle, path, entry.size, &mut inline)
                                }),
                            }
                        });
                        match read {
                            Some(len) => {
                                response.push(nexus_abi::status::CODE_OK as u8);
                                response.extend_from_slice(&inline[..len]);
                            }
                            None => {
                                let oversize =
                                    entry.map(|e| e.size as usize > inline.len()).unwrap_or(false);
                                let code =
                                    if oversize { VfsError::TooBig } else { VfsError::NotFound };
                                response.push(code.code() as u8);
                            }
                        }
                    }
                    OPCODE_READ_VMO => {
                        // THE PASS-THROUGH (RFC-0097 §2): the caller's VMO goes
                        // on to bundlemgrd, which is the only byte writer and
                        // the only one that may write a success header. Every
                        // failure BEFORE that point is a header packagefsd
                        // writes itself, because nobody else can.
                        let vmo = armed.take(sid);
                        let (status, len) = read_vmo(reader, registry, &bytes[1..], vmo);
                        if let Some(vmo) = vmo {
                            if status != nexus_abi::status::CODE_OK {
                                let hdr = nexus_abi::payload_vmo::encode_header(status, 0);
                                let _ = nexus_abi::vmo_write(vmo, 0, &hdr);
                            } else {
                                forwarded = forwarded.saturating_add(1);
                                if forwarded.is_power_of_two() {
                                    let _ = nexus_abi::debug_println(&format!(
                                        "packagefsd: read vmo forwarded (bytes={len} n={forwarded})"
                                    ));
                                }
                            }
                            let _ = nexus_abi::cap_close(vmo);
                        }
                        response.extend_from_slice(&status.to_le_bytes());
                        response.extend_from_slice(&len.to_le_bytes());
                    }
                    OPCODE_MOUNT_STATUS => response.push(mount_mode as u8),
                    OPCODE_LIST => {
                        // Payload = shared ReadDir codec (nexus-vfs-types); the
                        // reply payload is relayed verbatim by vfsd.
                        let payload = match nexus_vfs_types::decode_readdir_request(&bytes[1..]) {
                            Ok(req) => match registry.list(&req.path) {
                                Ok(entries) => nexus_vfs_types::encode_readdir_response(
                                    &entries,
                                    req.cursor as usize,
                                    req.limit,
                                    true,
                                )
                                .map(|(payload, _included)| payload)
                                .unwrap_or_else(nexus_vfs_types::encode_readdir_error),
                                Err(err) => nexus_vfs_types::encode_readdir_error(err),
                            },
                            Err(err) => nexus_vfs_types::encode_readdir_error(err),
                        };
                        response.extend_from_slice(&payload);
                    }
                    _ => response.extend_from_slice(&NOT_FOUND_RESOLVE_RSP),
                }
                // P2-c's rule: answer exactly the senders that moved a reply cap.
                if let Some(cap) = moved {
                    pending.park(cap, &response);
                }
            }
            Err(IpcError::Disconnected) => return Err(LiteError::Transport),
            Err(IpcError::WouldBlock) | Err(IpcError::Timeout) => {
                let _ = nexus_abi::yield_();
            }
            Err(_) => return Err(LiteError::Transport),
        }
    }
}

/// Resolves the `OP_READ_VMO` payload (a `pkg:/`-relative path) and forwards the
/// caller's armed VMO to bundlemgrd. Returns the `(status, len)` that both the
/// reply and the VMO header carry.
fn read_vmo(
    reader: Option<&VolumeReader>,
    registry: &BundleRegistry,
    payload: &[u8],
    vmo: Option<u32>,
) -> (u16, u32) {
    let Some(vmo) = vmo else {
        // Nothing was armed, so there is nowhere to put bytes OR a header.
        return (VfsError::Invalid.code(), 0);
    };
    let Ok(rel) = core::str::from_utf8(payload) else {
        return (VfsError::Invalid.code(), 0);
    };
    let Some(entry) = registry.resolve(rel) else {
        return (VfsError::NotFound.code(), 0);
    };
    if entry.kind != KIND_FILE {
        return (VfsError::IsDir.code(), 0);
    }
    // D4: an entry larger than the caller's VMO is refused BEFORE any byte is
    // written. packagefsd is the hop that knows both numbers — the entry's size
    // from the verified index, and the VMO's length from the capability it was
    // handed. Leaving it to the writer means the refusal depends on which layer
    // notices the overrun first (`Io` from the block read, `TooBig` from the
    // hash read-back), which is not a contract. bundlemgrd's own bound stays as
    // the backstop.
    if !nexus_abi::payload_vmo::fits(entry.size as usize, vmo_len(vmo).unwrap_or(0)) {
        return (VfsError::TooBig.code(), 0);
    }
    match &entry.source {
        // Seed-registry entries have no volume behind them; they are small by
        // construction, so packagefsd writes them itself — the only case where
        // it touches a byte, and it is not volume data.
        Source::Inline(bytes) => {
            if nexus_abi::vmo_write(vmo, nexus_abi::payload_vmo::DATA_OFFSET, bytes).is_err() {
                return (VfsError::Io.code(), 0);
            }
            let hdr = nexus_abi::payload_vmo::encode_header(
                nexus_abi::status::CODE_OK,
                bytes.len() as u32,
            );
            if nexus_abi::vmo_write(vmo, 0, &hdr).is_err() {
                return (VfsError::Io.code(), 0);
            }
            (nexus_abi::status::CODE_OK, bytes.len() as u32)
        }
        Source::Volume { bundle, path } => match reader {
            Some(reader) => reader.forward(bundle, path, vmo),
            None => (VfsError::Unsupported.code(), 0),
        },
    }
}

/// A VMO capability's byte length (RFC-0040 `cap_query`, `kind_tag` 1 = VMO).
fn vmo_len(slot: u32) -> Option<usize> {
    let mut query = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
    if nexus_abi::cap_query(slot, &mut query).is_err() || query.kind_tag != 1 {
        return None;
    }
    Some(query.len as usize)
}

fn seed_registry() -> (BundleRegistry, MountMode) {
    let mut registry = BundleRegistry::default();
    // Deterministic system properties for VFS bring-up tests.
    let system_entries = vec![
        (".".to_string(), Entry::directory()),
        ("build.prop".to_string(), Entry::file(b"ro.nexus.build=dev\n")),
    ];
    registry.publish("system", "1.0.0", &system_entries);

    let hello_entries = vec![
        (".".to_string(), Entry::directory()),
        ("manifest.nxb".to_string(), Entry::file(DEMO_HELLO_MANIFEST_NXB)),
        ("payload.elf".to_string(), Entry::file(DEMO_HELLO_PAYLOAD)),
    ];
    registry.publish("demo.hello", "1.0.0", &hello_entries);

    let exit_entries = vec![
        (".".to_string(), Entry::directory()),
        ("manifest.nxb".to_string(), Entry::file(DEMO_EXIT_MANIFEST_NXB)),
        ("payload.elf".to_string(), Entry::file(DEMO_EXIT_PAYLOAD)),
    ];
    registry.publish("demo.exit0", "1.0.0", &exit_entries);

    (registry, MountMode::Legacy)
}

/// `packagefsd: mounted (system volume slot=<s> bundles=N files=M)`.
pub(crate) fn emit_mounted(slot: u8, bundles: usize, files: usize) {
    let line = format!(
        "packagefsd: mounted (system volume slot={} bundles={} files={})\n",
        slot as char, bundles, files
    );
    debug_print(&line);
}

pub(crate) fn debug_print(_s: &str) {
    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    let _ = nexus_abi::debug_write(_s.as_bytes());
}

// raw UART helper removed in favor of debug_write syscall

/// Keeps Cap'n Proto schemas referenced on host builds.
pub fn touch_schemas() {}
