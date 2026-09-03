// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]
#![forbid(unsafe_code)]

//! CONTEXT: updated's apply engine v2 OS glue (TASK-0179; RFC-0089 §8).
//! Staging is PATH-BASED: the container is pulled from
//! `/updates/*.nxs` (data volume) over vfsd's canonical bulk data plane
//! (`OP_READ_VMO` splice — one round trip, bytes stay in the VMO and are
//! mapped read-only; the bump heap never sees a multi-MB allocation),
//! then `updates::component_set::verify_and_apply` streams it in 64-KiB
//! units into the [`SlotSink`], which writes the INACTIVE boot slot over
//! the partition-scoped block plane and lands the NXBD LAST after a full
//! readback verify. The active slot is never writable through this path.
//! OWNERS: @services-team @security
//! STATUS: Experimental (TASK-0179)
//! API_STABILITY: Internal
//! TEST_COVERAGE: pipeline host-proven in tests/updates_host (block-fake
//!   sink + power-cut matrix); this glue is proven by the QEMU `ota`
//!   crown lane and the headless stage rungs.
//! ADR: docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use core::sync::atomic::{AtomicU32, Ordering};
use core::time::Duration;

use nexus_abi::MsgHeader;
use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};
use storage::remote_blk::RemoteBlockDevice;
use storage::{blockproto, BlockDevice};
use updates::component_set::{ComponentMeta, ComponentSink, RejectReason};
use updates::Slot;

use crate::os_lite::emit_line;

/// init-lite control-channel slots (route requests via the responder).
const CTRL_SEND_SLOT: u32 = 1;
const CTRL_RECV_SLOT: u32 = 2;
/// updated's deterministic CAP_MOVE reply inbox (slot_map SSOT).
const REPLY_RECV_SLOT: u32 = 0x0a;
const REPLY_SEND_SLOT: u32 = 0x0b;

/// Staging source root (RFC-0089 §9 offline feed v1). NOTE the namespace
/// shape: the RFC's `/data/updates/` names the DATA VOLUME's updates
/// directory; that volume is mounted at the VFS root (its home layout is
/// `/Bilder`, `/Dokumente`, …), so on the wire the path is `/updates/…`.
pub(crate) const SOURCE_ROOT: &str = "/updates/";
/// Hard ceiling for a staging container (slot budget is the real bound;
/// this refuses absurd sizes before any allocation). The VMO itself is
/// sized to the ACTUAL file — a fixed 32 MiB reservation per attempt
/// exhausted the kernel VMO arena after four stages, because arena memory
/// is bump-allocated and a destroyed VMO does not return to the pool.
const SOURCE_MAX_BYTES: usize = 56 * 1024 * 1024;
/// Bounded splice-header poll (yield between attempts).
const SPLICE_POLL_MAX: u32 = 400_000;
/// Slot geometry (RFC-0089 §5): NXBD at sector 0, image from sector 8.
pub(crate) const SECTOR: usize = 512;
pub(crate) const IMAGE_START_SECTOR: u64 = 8;

/// A mapped staging source (VMO handle + read-only mapping).
pub(crate) struct MappedSource {
    vmo: u32,
    va: usize,
    pub(crate) len: usize,
}

impl MappedSource {
    pub(crate) fn bytes(&self) -> &[u8] {
        // The unsafe slice view lives in `mapmem` (the crate's single
        // bounded unsafe surface); the safety contract is documented there.
        crate::mapmem::ro_slice(self.va, self.len)
    }
}

impl Drop for MappedSource {
    fn drop(&mut self) {
        let _ = nexus_abi::vm_unmap(self.va, self.len);
        let _ = nexus_abi::vmo_destroy(self.vmo);
    }
}

/// Validates a staging path (bounded, rooted, `.nxs`, single segment).
pub(crate) fn validate_source_path(path: &str) -> Result<(), RejectReason> {
    if !path.starts_with(SOURCE_ROOT) || !path.ends_with(".nxs") {
        return Err(RejectReason::Path);
    }
    let name = &path[SOURCE_ROOT.len()..];
    if name.is_empty() || name.contains('/') || name.contains('\0') || name.len() > 64 {
        return Err(RejectReason::Path);
    }
    Ok(())
}

/// Pulls the container bytes from vfsd via the OP_READ_VMO splice plane.
pub(crate) fn read_source(path: &str) -> Result<MappedSource, RejectReason> {
    validate_source_path(path)?;
    let send_slot = vfsd_send_slot().ok_or(RejectReason::Io)?;

    // Size the VMO to the FILE, not to a worst case: ask first.
    let file_len = stat_size(send_slot, path)?;
    if file_len == 0 || file_len > SOURCE_MAX_BYTES {
        return Err(RejectReason::Bounds);
    }
    let vmo_cap = (nexus_vfs_types::SPLICE_DATA_OFFSET + file_len).div_ceil(4096) * 4096;

    let payload = nexus_vfs_types::encode_read_vmo_request(path).ok_or(RejectReason::Path)?;
    let mut frame = Vec::with_capacity(1 + payload.len());
    frame.push(nexus_vfs_types::OP_READ_VMO);
    frame.extend_from_slice(&payload);

    let vmo = nexus_abi::vmo_create(vmo_cap).map_err(|_| RejectReason::Io)?;
    let clone = match nexus_abi::cap_clone(vmo) {
        Ok(clone) => clone,
        Err(_) => {
            let _ = nexus_abi::vmo_destroy(vmo);
            return Err(RejectReason::Io);
        }
    };
    let hdr = MsgHeader::new(clone, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, frame.len() as u32);
    if send_bounded(send_slot, &hdr, &frame, 2_000_000_000).is_err() {
        let _ = nexus_abi::cap_close(clone);
        let _ = nexus_abi::vmo_destroy(vmo);
        return Err(RejectReason::Io);
    }

    // Poll the splice header (payload-first, header-last discipline).
    let mut header = [0u8; nexus_vfs_types::SPLICE_HEADER_LEN];
    let mut attempts = 0u32;
    let (status, len) = loop {
        if nexus_abi::vmo_read(vmo, 0, &mut header).is_ok() {
            if let Some(decoded) = nexus_vfs_types::decode_splice_header(&header) {
                break decoded;
            }
        }
        attempts += 1;
        if attempts > SPLICE_POLL_MAX {
            let _ = nexus_abi::vmo_destroy(vmo);
            return Err(RejectReason::Io);
        }
        let _ = nexus_abi::yield_();
    };
    if status != nexus_vfs_types::CODE_OK || len == 0 {
        let _ = nexus_abi::vmo_destroy(vmo);
        // A missing file is a PATH-level reject; transport stays io.
        return Err(if status == nexus_vfs_types::CODE_OK {
            RejectReason::Bounds
        } else {
            RejectReason::Path
        });
    }

    let total = nexus_vfs_types::SPLICE_DATA_OFFSET + len as usize;
    let map_len = total.div_ceil(4096) * 4096;
    let flags =
        nexus_abi::page_flags::VALID | nexus_abi::page_flags::USER | nexus_abi::page_flags::READ;
    let va = nexus_abi::vm_map(vmo, 0, map_len, flags).map_err(|_| {
        let _ = nexus_abi::vmo_destroy(vmo);
        RejectReason::Io
    })?;
    Ok(MappedSource { vmo, va: va + nexus_vfs_types::SPLICE_DATA_OFFSET, len: len as usize })
}

/// One bounded STAT round trip: `[OP_STAT, path]` → `[1, size u64, kind u16]`.
fn stat_size(send_slot: u32, path: &str) -> Result<usize, RejectReason> {
    let mut frame = Vec::with_capacity(1 + path.len());
    frame.push(nexus_vfs_types::fileops::OP_STAT);
    frame.extend_from_slice(path.as_bytes());
    let reply_clone = nexus_abi::cap_clone(REPLY_SEND_SLOT).map_err(|_| RejectReason::Io)?;
    let hdr = MsgHeader::new(reply_clone, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, frame.len() as u32);
    if send_bounded(send_slot, &hdr, &frame, 2_000_000_000).is_err() {
        let _ = nexus_abi::cap_close(reply_clone);
        return Err(RejectReason::Io);
    }
    let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(2_000_000_000);
    loop {
        if nexus_abi::nsec().unwrap_or(u64::MAX) >= deadline {
            return Err(RejectReason::Io);
        }
        let mut rh = MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 64];
        match nexus_abi::ipc_recv_v1(
            REPLY_RECV_SLOT,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                if n >= 11 && buf[0] == 1 {
                    let mut size = [0u8; 8];
                    size.copy_from_slice(&buf[1..9]);
                    return Ok(u64::from_le_bytes(size) as usize);
                }
                if n >= 1 && buf[0] == 0 {
                    return Err(RejectReason::Path);
                }
                // Foreign inbox frame: consumed, skipped.
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = nexus_abi::yield_();
            }
            Err(_) => return Err(RejectReason::Io),
        }
    }
}

/// The slot sink: partition-scoped writer over the block plane. Body from
/// sector 8; the descriptor sector is zeroed in `begin` and written LAST
/// in `finish` after a full readback digest.
pub(crate) struct SlotSink {
    dev: RemoteBlockDevice,
    budget_sectors: u64,
    /// Tail carry for non-sector-aligned final chunks.
    partial: Vec<u8>,
    next_sector: u64,
    written: u64,
}

impl SlotSink {
    /// Attaches the INACTIVE slot's partition (deny-by-default elsewhere).
    pub(crate) fn attach(inactive: Slot) -> Result<Self, RejectReason> {
        let part = match inactive {
            Slot::A => blockproto::PART_BOOT_A,
            Slot::B => blockproto::PART_BOOT_B,
        };
        let dev = RemoteBlockDevice::open_with_deadline(
            blockproto::CLIENT_REQ_SLOT,
            blockproto::CLIENT_REPLY_SEND_SLOT,
            blockproto::CLIENT_REPLY_RECV_SLOT,
            part,
            2_000_000_000,
        )
        .ok_or(RejectReason::Io)?;
        let budget_sectors = dev.block_count();
        Ok(Self {
            dev,
            budget_sectors,
            partial: Vec::new(),
            next_sector: IMAGE_START_SECTOR,
            written: 0,
        })
    }
}

impl ComponentSink for SlotSink {
    fn begin(&mut self, meta: &ComponentMeta) -> Result<(), RejectReason> {
        let body_budget =
            self.budget_sectors.saturating_sub(IMAGE_START_SECTOR).saturating_mul(SECTOR as u64);
        if meta.size > body_budget || meta.kind_data.len() != SECTOR {
            return Err(RejectReason::Bounds);
        }
        // Invalidate the commit point FIRST: a torn stage must leave the
        // slot NXBD-invalid (never half-bootable).
        self.dev.write_blocks(0, &[0u8; SECTOR]).map_err(|_| RejectReason::Io)?;
        self.dev.sync().map_err(|_| RejectReason::Io)?;
        self.partial.clear();
        self.next_sector = IMAGE_START_SECTOR;
        self.written = 0;
        Ok(())
    }

    fn chunk(&mut self, _offset: u64, bytes: &[u8]) -> Result<(), RejectReason> {
        self.partial.extend_from_slice(bytes);
        let full = self.partial.len() / SECTOR * SECTOR;
        if full > 0 {
            self.dev
                .write_blocks(self.next_sector, &self.partial[..full])
                .map_err(|_| RejectReason::Io)?;
            self.next_sector += (full / SECTOR) as u64;
            self.written += full as u64;
            self.partial.drain(..full);
        }
        Ok(())
    }

    fn finish(&mut self, meta: &ComponentMeta) -> Result<(), RejectReason> {
        if !self.partial.is_empty() {
            let mut tail = core::mem::take(&mut self.partial);
            self.written += tail.len() as u64;
            tail.resize(tail.len().div_ceil(SECTOR) * SECTOR, 0);
            self.dev.write_blocks(self.next_sector, &tail).map_err(|_| RejectReason::Io)?;
            self.next_sector += (tail.len() / SECTOR) as u64;
        }
        if self.written != meta.size {
            return Err(RejectReason::Bounds);
        }
        // Readback verify: hash the on-disk body before the commit point.
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        let mut remaining = meta.size;
        let mut sector = IMAGE_START_SECTOR;
        // Heap (ONE allocation per component), not a 16 KiB stack array:
        // service stacks are small and this runs deep in the apply path.
        let mut buf = alloc::vec![0u8; 32 * SECTOR];
        while remaining > 0 {
            let take = remaining.min(buf.len() as u64) as usize;
            let padded = take.div_ceil(SECTOR) * SECTOR;
            self.dev.read_blocks(sector, &mut buf[..padded]).map_err(|_| RejectReason::Io)?;
            hasher.update(&buf[..take]);
            sector += (padded / SECTOR) as u64;
            remaining -= take as u64;
            let _ = nexus_abi::yield_();
        }
        let mut digest = [0u8; 32];
        digest.copy_from_slice(&hasher.finalize());
        if digest != meta.sha256 {
            return Err(RejectReason::Digest);
        }
        // Commit point: the signed NXBD lands VERBATIM, last.
        let mut nxbd = [0u8; SECTOR];
        nxbd.copy_from_slice(&meta.kind_data);
        self.dev.write_blocks(0, &nxbd).map_err(|_| RejectReason::Io)?;
        self.dev.sync().map_err(|_| RejectReason::Io)?;
        Ok(())
    }
}

/// Feed v1 (RFC-0089 §9): deterministic listing of `.nxs` candidates in
/// `/updates/` (data volume) via vfsd READDIR (CAP_MOVE reply on our inbox).
pub(crate) fn feed_list() -> Result<Vec<String>, RejectReason> {
    let send_slot = vfsd_send_slot().ok_or(RejectReason::Io)?;
    let payload = nexus_vfs_types::encode_readdir_request("/updates", 0, 64)
        .map_err(|_| RejectReason::Path)?;
    let mut frame = Vec::with_capacity(1 + payload.len());
    frame.push(nexus_vfs_types::fileops::OP_READDIR);
    frame.extend_from_slice(&payload);

    let reply_clone = nexus_abi::cap_clone(REPLY_SEND_SLOT).map_err(|_| RejectReason::Io)?;
    let hdr = MsgHeader::new(reply_clone, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, frame.len() as u32);
    if send_bounded(send_slot, &hdr, &frame, 2_000_000_000).is_err() {
        let _ = nexus_abi::cap_close(reply_clone);
        return Err(RejectReason::Io);
    }
    // TASK-0140: the FIRST feed call is what mounts the data partition
    // (nxfsd attach + journal replay over the 512B/QD1 block plane) — a 2s
    // reply budget failed honestly on exactly that cold path once the op
    // gained its first live caller. Bounded, but sized for the cold mount.
    let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(15_000_000_000);
    loop {
        if nexus_abi::nsec().unwrap_or(u64::MAX) >= deadline {
            return Err(RejectReason::Io);
        }
        let mut rh = MsgHeader::new(0, 0, 0, 0, 0);
        let mut buf = [0u8; 4096];
        match nexus_abi::ipc_recv_v1(
            REPLY_RECV_SLOT,
            &mut rh,
            &mut buf,
            nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
            0,
        ) {
            Ok(n) => {
                let n = core::cmp::min(n as usize, buf.len());
                // TASK-0140 (first live caller of this op): the home-mount
                // data plane replies with the RAW readdir page — NO opcode
                // echo (`DataStore::handle` returns the encoded page
                // directly; only the /packages namespace arm echoes). The
                // original echo-byte match skipped every real reply until
                // the deadline. Accept both shapes; a frame that decodes
                // as neither is foreign inbox traffic (statefs/logd acks)
                // and is skipped, bounded by the deadline.
                let body = if n >= 1 && buf[0] == nexus_vfs_types::fileops::OP_READDIR {
                    &buf[1..n]
                } else {
                    &buf[..n]
                };
                if let Ok(page) = nexus_vfs_types::decode_readdir_response(body) {
                    let mut names: Vec<String> = page
                        .entries
                        .iter()
                        .filter(|e| e.name.ends_with(".nxs"))
                        .map(|e| e.name.clone())
                        .collect();
                    names.sort();
                    return Ok(names);
                }
            }
            Err(nexus_abi::IpcError::QueueEmpty) => {
                let _ = nexus_abi::yield_();
            }
            Err(_) => return Err(RejectReason::Io),
        }
    }
}

fn vfsd_send_slot() -> Option<u32> {
    static SEND: AtomicU32 = AtomicU32::new(0);
    let cached = SEND.load(Ordering::Relaxed);
    if cached != 0 {
        return Some(cached);
    }
    match budget::route_with_nonce_budgeted(
        b"vfsd",
        CTRL_SEND_SLOT,
        CTRL_RECV_SLOT,
        Duration::from_secs(2),
        NonceMismatchBudget::new(64),
    ) {
        RouteRetryOutcome::Success { send_slot, .. } => {
            SEND.store(send_slot, Ordering::Relaxed);
            Some(send_slot)
        }
        _ => {
            emit_line("updated: vfsd route unavailable");
            None
        }
    }
}

fn send_bounded(slot: u32, hdr: &MsgHeader, frame: &[u8], budget_ns: u64) -> Result<(), ()> {
    let deadline = nexus_abi::nsec().map_err(|_| ())?.saturating_add(budget_ns);
    loop {
        match nexus_abi::ipc_send_v1(slot, hdr, frame, nexus_abi::IPC_SYS_NONBLOCK, 0) {
            Ok(_) => return Ok(()),
            Err(nexus_abi::IpcError::QueueFull) => {
                if nexus_abi::nsec().unwrap_or(u64::MAX) >= deadline {
                    return Err(());
                }
                let _ = nexus_abi::yield_();
            }
            Err(_) => return Err(()),
        }
    }
}
