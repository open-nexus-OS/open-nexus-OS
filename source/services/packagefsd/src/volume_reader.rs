// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: packagefsd's bundlemgrd-facing half — how `pkg:/` gets its index at
//! mount time and its bytes at read time (TASK-0321 P5, TASK-0033 P2).
//!
//! The two are not symmetric, and that asymmetry is the point. The INDEX is
//! copied once into packagefsd's own VMO, because packagefsd has to parse it.
//! ENTRY BYTES are not copied at all: the caller's VMO is forwarded through to
//! bundlemgrd, which verifies the entry digest and writes both the payload and
//! the release header into it. packagefsd keeps only a small VMO for the inline
//! tier — it used to size that one for the LARGEST entry on the volume (7.25 MiB
//! for a bundle ELF) because every read passed through it.
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU (`packagefsd: mounted`, `packagefsd: read vmo forwarded`,
//!   `SELFTEST: pkgimg vmo ok`)
//! RFC: docs/rfcs/RFC-0097-payload-vmo-header-v2-pkg-passthrough.md

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use nexus_vfs_types::VfsError;
use storage::pkgimg::PkgImgCaps;
use storage::pkgimg_bundles::parse_index;

use crate::os_lite::{debug_print, emit_mounted, BundleRegistry, Entry, INDEX_VMO_BYTES};

/// The on-demand file reader: bundlemgrd client + ONE reusable VMO sized
/// for the largest entry on the volume (the VMO arena never frees, so a
/// per-request VMO would leak).
pub(crate) struct VolumeReader {
    bundle_send: u32,
    /// The CAP_MOVE reply inbox: every VMO op answers here (TASK-0324 P7-d).
    inbox: nexus_service_topology::SlotPair,
    vmo: u32,
    vmo_len: usize,
}

impl VolumeReader {
    /// Encodes a `GET_FILE_VMO` request for one entry.
    fn request(bundle: &str, path: &str, out: &mut [u8; 160]) -> Option<usize> {
        nexus_abi::bundlemgrd::encode_get_file_vmo(bundle.as_bytes(), path.as_bytes(), out)
    }

    /// FORWARDS the CALLER's VMO to bundlemgrd (TASK-0033 P2): the entry's bytes
    /// are streamed and digest-checked by the authority straight into the VMO the
    /// client created, and the success header is written there by bundlemgrd —
    /// packagefsd never sees a byte of it.
    ///
    /// Returns bundlemgrd's `(status, len)`; the header in the VMO carries the
    /// same status, written LAST. A transport failure is `Io` — the caller writes
    /// that header itself, because nobody else can now.
    pub(crate) fn forward(&self, bundle: &str, path: &str, caller_vmo: u32) -> (u16, u32) {
        use nexus_abi::bundlemgrd as wire;
        // Clear the header first: `CODE_OK` is 0, so on a VMO that was used
        // before, only the absent magic distinguishes "not written yet" from a
        // stale release (RFC-0097 §1).
        if nexus_abi::vmo_write(caller_vmo, 0, &nexus_abi::payload_vmo::ZEROED_HEADER).is_err() {
            return (VfsError::Io.code(), 0);
        }
        let mut req = [0u8; 160];
        let Some(n) = Self::request(bundle, path, &mut req) else {
            return (VfsError::Invalid.code(), 0);
        };
        // ARM the caller's VMO, then ask with a reply cap: bundlemgrd streams,
        // writes the header LAST and answers — the answer is waited for (or its
        // death), never polled (P7-d).
        match vmo_op(self.bundle_send, self.inbox, caller_vmo, &req[..n], wire::OP_GET_FILE_VMO) {
            Some(answer) => answer,
            None => (VfsError::Io.code(), 0),
        }
    }

    /// Reads one entry into `out` through packagefsd's OWN small VMO, for the
    /// inline tier only. Bulk never lands here: this VMO holds
    /// `INLINE_IO_MAX` bytes and the service runs on a heap that never frees,
    /// so `out` is the loop's reusable buffer, not a fresh `Vec`.
    pub(crate) fn read_inline(
        &self,
        bundle: &str,
        path: &str,
        size: u64,
        out: &mut [u8],
    ) -> Option<usize> {
        use nexus_abi::bundlemgrd as wire;
        use nexus_abi::payload_vmo as hdr;
        let size = usize::try_from(size).ok()?;
        if size > out.len() || !hdr::fits(size, self.vmo_len) {
            return None;
        }
        nexus_abi::vmo_write(self.vmo, 0, &hdr::ZEROED_HEADER).ok()?;
        let mut req = [0u8; 160];
        let n = Self::request(bundle, path, &mut req)?;
        let (status, len) =
            vmo_op(self.bundle_send, self.inbox, self.vmo, &req[..n], wire::OP_GET_FILE_VMO)?;
        if status != nexus_abi::status::CODE_OK || len as usize != size {
            return None;
        }
        let len = len as usize;
        nexus_abi::vmo_read(self.vmo, hdr::DATA_OFFSET, &mut out[..len]).ok()?;
        Some(len)
    }
}

/// One VMO op against bundlemgrd (TASK-0324 P7-d): `OP_ARM_VMO` with a clone of `vmo` as
/// the moved cap, then `req` with a reply-SEND clone; the answer `(status, len)` is WAITED
/// for on the reply inbox — bundlemgrd's reply or its death (EOF). Foreign inbox frames
/// (another op's late answer) are skipped.
fn vmo_op(
    bundle_send: u32,
    inbox: nexus_service_topology::SlotPair,
    vmo: u32,
    req: &[u8],
    op: u8,
) -> Option<(u16, u32)> {
    use nexus_abi::bundlemgrd as wire;
    let moved = nexus_abi::cap_clone(vmo).ok()?;
    if nexus_ipc::exchange::send_with_cap(bundle_send, &arm_frame(), moved).is_err() {
        let _ = nexus_abi::cap_close(moved);
        return None;
    }
    let mut buf = [0u8; 64];
    nexus_ipc::exchange::call_matching(bundle_send, inbox, req, &mut buf, |rsp| {
        wire::decode_payload_done_rsp(rsp, op)
    })
    .ok()
}

fn arm_frame() -> [u8; 4] {
    let mut arm = [0u8; 4];
    nexus_abi::bundlemgrd::encode_arm_vmo(&mut arm);
    arm
}

/// Runs the minimal packagefs daemon, emitting a readiness marker once.

/// TASK-0321 P5: `pkg:/` from the verified system volume. VOLUME_STATUS
/// (slot + bundle count for the marker), then GET_INDEX into a bounded VMO
/// (the NXSV-bound index bytes bundlemgrd verified) → registry with file
/// sizes + kinds; bytes stay on the volume until resolved.
pub(crate) fn load_registry_from_volume() -> Option<(BundleRegistry, VolumeReader)> {
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
    use nexus_abi::payload_vmo as hdr;
    // Nonce-correlated route queries: this service's own server route
    // query (issued at start, answered by init only after bootstrap) leaves
    // a reply in the ctrl queue that a nonce-less `new_for` consumed as the
    // answer to "bundlemgrd" — handing us OUR OWN server pair (4,3). The
    // pre-P5 registry load silently fell back to the seed image that way.
    // The declared legs (TASK-0324 P7-b/P7-d): bundlemgrd's request endpoint and our reply
    // inbox, pinned by init before this task runs — no route ask.
    let bnd = nexus_service_topology::slots::packagefsd::BUNDLEMGRD;
    let inbox = nexus_service_topology::slots::packagefsd::REPLY;

    // VOLUME_STATUS on the reply path: the moved cap is a SEND clone of our
    // reply inbox, so the answer arrives on the inbox's RECV side.
    let mut req = [0u8; 8];
    let n = wire::encode_volume_status(&mut req).ok_or("encode status")?;
    let mut buf = [0u8; 64];
    let (status, slot, verified, bundles) =
        nexus_ipc::exchange::call_matching(bnd.send, inbox, &req[..n], &mut buf, |rsp| {
            wire::decode_volume_status_rsp(rsp).map(|(s, sl, v, b, _build8)| (s, sl, v, b))
        })
        .map_err(|_| "status answer")?;
    if status != wire::STATUS_OK || verified != 1 {
        return Err("volume unverified");
    }

    // GET_INDEX into an ARMED VMO; bundlemgrd's answer (after the header write) is waited
    // for — TASK-0324 P7-d, no header poll.
    let index_vmo = nexus_abi::vmo_create(INDEX_VMO_BYTES).map_err(|_| "index vmo")?;
    let n = wire::encode_get_index(&mut req).ok_or("encode index")?;
    let (status, len) =
        vmo_op(bnd.send, inbox, index_vmo, &req[..n], wire::OP_GET_INDEX).ok_or("index answer")?;
    if status != nexus_abi::status::CODE_OK || (len as usize) > INDEX_VMO_BYTES - hdr::DATA_OFFSET {
        return Err("index header");
    }
    let index_len = len as usize;
    let mut head = vec![0u8; index_len];
    nexus_abi::vmo_read(index_vmo, hdr::DATA_OFFSET, &mut head).map_err(|_| "index read")?;
    let index = parse_index(&head, &PkgImgCaps::default()).map_err(|_| "index parse")?;

    let mut registry = BundleRegistry::default();
    let mut groups: BTreeMap<String, Vec<(String, Entry)>> = BTreeMap::new();
    let mut versions: BTreeMap<String, String> = BTreeMap::new();
    let mut files = 0usize;
    for e in &index.entries {
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
    // ONE reusable VMO for the INLINE tier only (TASK-0033 P2). It used to be
    // sized for the LARGEST entry on the volume — 7.25 MiB for a bundle ELF —
    // because every read passed through packagefsd. Bulk is forwarded now: the
    // caller's own VMO goes to bundlemgrd and packagefsd holds no entry bytes.
    let vmo_len = (hdr::DATA_OFFSET + nexus_vfs_types::INLINE_IO_MAX).div_ceil(4096) * 4096;
    let vmo = nexus_abi::vmo_create(vmo_len).map_err(|_| "file vmo")?;
    emit_mounted(slot, bundles as usize, files);
    Ok((registry, VolumeReader { bundle_send: bnd.send, inbox, vmo, vmo_len }))
}
