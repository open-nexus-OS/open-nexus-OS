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
use nexus_ipc::Server;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use nexus_ipc::{Client, IpcError, KernelClient, KernelServer, Wait};
use nexus_vfs_types::{DirEntry, VfsError};
use storage::pkgimg::PkgImgCaps;
use storage::pkgimg_bundles::parse_index;

use crate::listing;

const OPCODE_RESOLVE: u8 = 2;
const OPCODE_MOUNT_STATUS: u8 = 3;
const OPCODE_LIST: u8 = 4;
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
const INDEX_VMO_BYTES: usize = nexus_abi::bundlemgrd::PAYLOAD_DATA_OFFSET + 256 * 1024;

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
struct BundleRegistry {
    bundles: BTreeMap<String, BTreeMap<String, Entry>>, // bundle@version -> path -> entry
    active: BTreeMap<String, String>,                   // bundle -> version
}

impl BundleRegistry {
    fn publish(&mut self, bundle: &str, version: &str, entries: &[(String, Entry)]) {
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
struct Entry {
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
    fn directory() -> Self {
        Self { size: 0, kind: KIND_DIRECTORY, source: Source::Inline(Vec::new()) }
    }

    fn file(bytes: &[u8]) -> Self {
        Self { size: bytes.len() as u64, kind: KIND_FILE, source: Source::Inline(bytes.to_vec()) }
    }

    fn volume_file(size: u64, bundle: &str, path: &str) -> Self {
        Self {
            size,
            kind: KIND_FILE,
            source: Source::Volume { bundle: bundle.to_string(), path: path.to_string() },
        }
    }
}

/// The on-demand file reader: bundlemgrd client + ONE reusable VMO sized
/// for the largest entry on the volume (the VMO arena never frees, so a
/// per-request VMO would leak).
struct VolumeReader {
    bundle: KernelClient,
    /// The CAP_MOVE reply inbox: every VMO op answers here (TASK-0324 P7-d).
    reply: KernelClient,
    reply_send_slot: u32,
    vmo: u32,
    vmo_len: usize,
}

impl VolumeReader {
    /// Fetches one entry's bytes through `GET_FILE_VMO` (header-last poll).
    fn fetch(&self, bundle: &str, path: &str, size: u64) -> Option<Vec<u8>> {
        use nexus_abi::bundlemgrd as wire;
        let size = usize::try_from(size).ok()?;
        if wire::PAYLOAD_DATA_OFFSET + size > self.vmo_len {
            return None;
        }
        // Clear the header so a stale OK from the previous fetch can never
        // be mistaken for this one.
        let zero = [0u8; wire::PAYLOAD_DATA_OFFSET];
        nexus_abi::vmo_write(self.vmo, 0, &zero).ok()?;
        let mut req = [0u8; 160];
        let n = wire::encode_get_file_vmo(bundle.as_bytes(), path.as_bytes(), &mut req)?;
        // ARM the VMO, then ask with a reply cap: bundlemgrd streams, writes the header LAST
        // and answers — the answer is waited for (or its death), never polled (P7-d).
        let (status, len) = vmo_op(
            &self.bundle,
            &self.reply,
            self.reply_send_slot,
            self.vmo,
            &req[..n],
            wire::OP_GET_FILE_VMO,
        )?;
        if status != wire::PAYLOAD_STATUS_OK || len as usize != size {
            return None;
        }
        let len = len as usize;
        let mut bytes = vec![0u8; len];
        nexus_abi::vmo_read(self.vmo, wire::PAYLOAD_DATA_OFFSET, &mut bytes).ok()?;
        Some(bytes)
    }
}

/// One VMO op against bundlemgrd (TASK-0324 P7-d): `OP_ARM_VMO` with a clone of `vmo` as
/// the moved cap, then `req` with a reply-SEND clone; the answer `(status, len)` is WAITED
/// for on the reply inbox — bundlemgrd's reply or its death (EOF). Foreign inbox frames
/// (another op's late answer) are skipped.
fn vmo_op(
    bundle: &KernelClient,
    reply: &KernelClient,
    reply_send_slot: u32,
    vmo: u32,
    req: &[u8],
    op: u8,
) -> Option<(u8, u32)> {
    use nexus_abi::bundlemgrd as wire;
    let moved = nexus_abi::cap_clone(vmo).ok()?;
    let mut arm = [0u8; 4];
    wire::encode_arm_vmo(&mut arm);
    if bundle.send_with_cap_move_wait(&arm, moved, Wait::Blocking).is_err() {
        let _ = nexus_abi::cap_close(moved);
        return None;
    }
    let reply_clone = nexus_abi::cap_clone(reply_send_slot).ok()?;
    if bundle.send_with_cap_move_wait(req, reply_clone, Wait::Blocking).is_err() {
        let _ = nexus_abi::cap_close(reply_clone);
        return None;
    }
    let mut buf = [0u8; 64];
    loop {
        let n = reply.recv_into_eof(Wait::Blocking, &mut buf).ok()?;
        if let Some(done) = wire::decode_payload_done_rsp(&buf[..n.min(buf.len())], op) {
            return Some(done);
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

fn run_loop(
    server: &KernelServer,
    registry: &BundleRegistry,
    mount_mode: MountMode,
    reader: Option<&VolumeReader>,
) -> LiteResult<()> {
    let mut response = Vec::with_capacity(256);
    loop {
        match server.recv(Wait::Blocking) {
            Ok(bytes) => {
                if bytes.is_empty() {
                    continue;
                }
                match bytes[0] {
                    OPCODE_RESOLVE => {
                        let entry = match core::str::from_utf8(&bytes[1..]) {
                            Ok(rel) => registry.resolve(rel),
                            Err(_) => None,
                        };
                        // Volume-backed bytes are fetched on demand; a fetch
                        // that fails (digest, transport) is a NotFound — never
                        // partial bytes.
                        let resolved = entry.and_then(|entry| match &entry.source {
                            Source::Inline(bytes) => Some((entry.size, entry.kind, bytes.clone())),
                            Source::Volume { bundle, path } => reader
                                .and_then(|r| r.fetch(bundle, path, entry.size))
                                .map(|bytes| (entry.size, entry.kind, bytes)),
                        });
                        response.clear();
                        if let Some((size, kind, bytes)) = resolved {
                            response.push(1);
                            response.extend_from_slice(&size.to_le_bytes());
                            response.extend_from_slice(&kind.to_le_bytes());
                            response.extend_from_slice(&bytes);
                        } else {
                            response.push(0);
                            response.extend_from_slice(&0u64.to_le_bytes());
                            response.extend_from_slice(&0u16.to_le_bytes());
                        }
                        server.send(&response, Wait::Blocking).map_err(|_| LiteError::Transport)?;
                    }
                    OPCODE_MOUNT_STATUS => {
                        response.clear();
                        response.push(mount_mode as u8);
                        server.send(&response, Wait::Blocking).map_err(|_| LiteError::Transport)?;
                    }
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
                        response.clear();
                        response.extend_from_slice(&payload);
                        server.send(&response, Wait::Blocking).map_err(|_| LiteError::Transport)?;
                    }
                    _ => {
                        response.clear();
                        response.push(0);
                        response.extend_from_slice(&0u64.to_le_bytes());
                        response.extend_from_slice(&0u16.to_le_bytes());
                        server.send(&response, Wait::Blocking).map_err(|_| LiteError::Transport)?;
                    }
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

/// TASK-0321 P5: `pkg:/` from the verified system volume. VOLUME_STATUS
/// (slot + bundle count for the marker), then GET_INDEX into a bounded VMO
/// (the NXSV-bound index bytes bundlemgrd verified) → registry with file
/// sizes + kinds; bytes stay on the volume until resolved.
fn load_registry_from_volume() -> Option<(BundleRegistry, VolumeReader)> {
    let outcome = load_registry_from_volume_inner();
    if let Err(step) = &outcome {
        // Honest fallback reason (the seed registry mounts as Legacy next).
        let line = format!("packagefsd: volume mount FAIL ({step})\n");
        debug_print(&line);
    }
    outcome.ok()
}

fn load_registry_from_volume_inner() -> Result<(BundleRegistry, VolumeReader), &'static str> {
    use nexus_abi::bundlemgrd as wire;
    // Nonce-correlated route queries: this service's own server route
    // query (issued at start, answered by init only after bootstrap) leaves
    // a reply in the ctrl queue that a nonce-less `new_for` consumed as the
    // answer to "bundlemgrd" — handing us OUR OWN server pair (4,3). The
    // pre-P5 registry load silently fell back to the seed image that way.
    // The declared legs (TASK-0324 P7-b/P7-d): bundlemgrd's request endpoint and our reply
    // inbox, pinned by init before this task runs — no route ask.
    let bnd = nexus_service_topology::slots::packagefsd::BUNDLEMGRD;
    let inbox = nexus_service_topology::slots::packagefsd::REPLY;
    let (reply_send_slot, reply_recv_slot) = (inbox.send, inbox.recv);
    let bundle = KernelClient::new_with_slots(bnd.send, bnd.recv).map_err(|_| "client")?;
    let reply =
        KernelClient::new_with_slots(reply_send_slot, reply_recv_slot).map_err(|_| "client")?;
    let wait = Wait::Blocking;

    // VOLUME_STATUS on the reply path: the moved cap is a SEND clone of our
    // reply inbox, so the answer arrives on the inbox's RECV side.
    let reply_clone = nexus_abi::cap_clone(reply_send_slot).map_err(|_| "reply clone")?;
    let mut req = [0u8; 8];
    let n = wire::encode_volume_status(&mut req).ok_or("encode status")?;
    bundle.send_with_cap_move_wait(&req[..n], reply_clone, wait).map_err(|_| "send status")?;
    let rsp = reply.recv(Wait::Blocking).map_err(|_| "recv status")?;
    let (status, slot, verified, bundles, _build8) =
        wire::decode_volume_status_rsp(&rsp).ok_or("decode status")?;
    if status != wire::STATUS_OK || verified != 1 {
        return Err("volume unverified");
    }

    // GET_INDEX into an ARMED VMO; bundlemgrd's answer (after the header write) is waited
    // for — TASK-0324 P7-d, no header poll.
    let index_vmo = nexus_abi::vmo_create(INDEX_VMO_BYTES).map_err(|_| "index vmo")?;
    let n = wire::encode_get_index(&mut req).ok_or("encode index")?;
    let (status, len) =
        vmo_op(&bundle, &reply, reply_send_slot, index_vmo, &req[..n], wire::OP_GET_INDEX)
            .ok_or("index answer")?;
    if status != wire::PAYLOAD_STATUS_OK
        || (len as usize) > INDEX_VMO_BYTES - wire::PAYLOAD_DATA_OFFSET
    {
        return Err("index header");
    }
    let index_len = len as usize;
    let mut head = vec![0u8; index_len];
    nexus_abi::vmo_read(index_vmo, wire::PAYLOAD_DATA_OFFSET, &mut head)
        .map_err(|_| "index read")?;
    let index = parse_index(&head, &PkgImgCaps::default()).map_err(|_| "index parse")?;

    let mut registry = BundleRegistry::default();
    let mut groups: BTreeMap<String, Vec<(String, Entry)>> = BTreeMap::new();
    let mut versions: BTreeMap<String, String> = BTreeMap::new();
    let mut largest = 0u64;
    let mut files = 0usize;
    for e in &index.entries {
        largest = largest.max(e.data_len);
        files += 1;
        let key = format!("{}@{}", e.bundle, e.version);
        groups
            .entry(key)
            .or_insert_with(|| vec![(".".to_string(), Entry::directory())])
            .push((e.path.clone(), Entry::volume_file(e.data_len, &e.bundle, &e.path)));
        versions.insert(e.bundle.clone(), e.version.clone());
    }
    for (canonical, entries) in groups {
        let (bundle, version) = canonical.split_once('@').ok_or("index key")?;
        registry.publish(bundle, version, &entries);
    }
    for (b, v) in versions {
        registry.active.insert(b, v);
    }
    // ONE reusable file VMO sized for the largest entry (page-rounded).
    let largest = usize::try_from(largest).map_err(|_| "entry size")?;
    let vmo_len = (wire::PAYLOAD_DATA_OFFSET + largest).div_ceil(4096) * 4096;
    let vmo = nexus_abi::vmo_create(vmo_len).map_err(|_| "file vmo")?;
    emit_mounted(slot, bundles as usize, files);
    Ok((registry, VolumeReader { bundle, vmo, vmo_len, reply, reply_send_slot }))
}

/// `packagefsd: mounted (system volume slot=<s> bundles=N files=M)`.
fn emit_mounted(slot: u8, bundles: usize, files: usize) {
    let line = format!(
        "packagefsd: mounted (system volume slot={} bundles={} files={})\n",
        slot as char, bundles, files
    );
    debug_print(&line);
}

fn debug_print(_s: &str) {
    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    let _ = nexus_abi::debug_write(_s.as_bytes());
}

// raw UART helper removed in favor of debug_write syscall

/// Keeps Cap'n Proto schemas referenced on host builds.
pub fn touch_schemas() {}
