// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]
#![forbid(unsafe_code)]

//! CONTEXT: the OS half of bundle-set staging (RFC-0089 §12.4, TASK-0321
//! P3): the block-plane devices and marker events the generic
//! `updates::volume_apply::VolumeAssembler` drives. `BlockVolumeDev` wraps
//! a partition-scoped `RemoteBlockDevice` (virtioblkd grants `updated`
//! both system partitions — the engine scopes the SLOT: writes go only to
//! the inactive one, the active one is read for unchanged-bundle reuse).
//! `VolumeMarkers` prints the §12.7 lines. `active_nxbd_digest` gives a
//! volume-only set its pairing target (the loader-verified ACTIVE NXBD).
//! `restage_clean` is the §8 probe: an inactive slot pair with no valid
//! NXBD/NXSV at stage begin prints `updated: restage clean`.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: assembler logic host-proven in
//!   tests/updates_host/tests/component_set_volume.rs; transport via the
//!   QEMU `ota-bundle` lane markers.
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

extern crate alloc;

use storage::{blockproto, remote_blk::RemoteBlockDevice, BlockDevice};
use updates::component_set::RejectReason;
use updates::volume_apply::{VolumeDev, VolumeEvents};
use updates::Slot;

use crate::apply_os::SECTOR;
use crate::os_lite::emit_bytes;

fn system_part(slot: Slot) -> u8 {
    match slot {
        Slot::A => blockproto::PART_SYSTEM_A,
        Slot::B => blockproto::PART_SYSTEM_B,
    }
}

fn boot_part(slot: Slot) -> u8 {
    match slot {
        Slot::A => blockproto::PART_BOOT_A,
        Slot::B => blockproto::PART_BOOT_B,
    }
}

fn open(part: u8) -> Result<RemoteBlockDevice, RejectReason> {
    RemoteBlockDevice::open_with_deadline(
        blockproto::CLIENT_REQ_SLOT,
        blockproto::CLIENT_REPLY_SEND_SLOT,
        blockproto::CLIENT_REPLY_RECV_SLOT,
        part,
        2_000_000_000,
    )
    .ok_or(RejectReason::Io)
}

/// A system partition as the assembler's sector device.
pub(crate) struct BlockVolumeDev {
    dev: RemoteBlockDevice,
}

impl BlockVolumeDev {
    pub(crate) fn attach(slot: Slot) -> Result<Self, RejectReason> {
        Ok(Self { dev: open(system_part(slot))? })
    }
}

impl VolumeDev for BlockVolumeDev {
    fn block_count(&self) -> u64 {
        self.dev.block_count()
    }
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), RejectReason> {
        self.dev.read_blocks(lba, buf).map_err(|_| RejectReason::Io)
    }
    fn write(&mut self, lba: u64, buf: &[u8]) -> Result<(), RejectReason> {
        self.dev.write_blocks(lba, buf).map_err(|_| RejectReason::Io)
    }
    fn sync(&mut self) -> Result<(), RejectReason> {
        self.dev.sync().map_err(|_| RejectReason::Io)
    }
}

/// The ACTIVE boot slot's image digest (its NXBD, loader-verified this
/// boot) — the pairing target of a volume-only set. `None` on a
/// direct-kernel boot (blank/invalid NXBD): such a set honestly rejects
/// `volume-binding` instead of pairing with nothing.
pub(crate) fn active_nxbd_digest(active: Slot) -> Option<[u8; 32]> {
    let dev = open(boot_part(active)).ok()?;
    let mut sector = [0u8; SECTOR];
    dev.read_blocks(0, &mut sector).ok()?;
    bootfmt::nxbd::decode(&sector).ok().map(|(d, _)| d.image_sha256)
}

/// §8 `updated: restage clean`: neither the inactive boot slot nor the
/// inactive system slot holds a valid descriptor at stage begin (a torn
/// or blank pair) — the marker names the state the engine now converges
/// from. Best-effort: a block-plane hiccup here is not a stage verdict.
pub(crate) fn restage_clean(inactive: Slot) {
    let boot_valid = open(boot_part(inactive))
        .ok()
        .and_then(|dev| {
            let mut s = [0u8; SECTOR];
            dev.read_blocks(0, &mut s).ok()?;
            Some(bootfmt::nxbd::decode(&s).is_ok())
        })
        .unwrap_or(true);
    let volume_valid = open(system_part(inactive))
        .ok()
        .and_then(|dev| {
            let mut s = [0u8; SECTOR];
            dev.read_blocks(0, &mut s).ok()?;
            Some(bootfmt::nxsv::decode(&s).is_ok())
        })
        .unwrap_or(true);
    if !boot_valid && !volume_valid {
        emit_bytes(b"updated: restage clean\n");
    }
}

/// §12.7 marker events.
pub(crate) struct VolumeMarkers;

impl VolumeEvents for VolumeMarkers {
    fn volume_verified(&mut self, build_id: &str, bundles: usize) {
        // `updated: component system-volume verified (build=<id8> bundles=N)`
        let mut line = [0u8; 96];
        let mut n = put(&mut line, 0, b"updated: component system-volume verified (build=");
        n = put(&mut line, n, &build_id.as_bytes()[..build_id.len().min(8)]);
        n = put(&mut line, n, b" bundles=");
        n = put_dec(&mut line, n, bundles);
        n = put(&mut line, n, b")\n");
        emit_bytes(&line[..n]);
    }
    fn bundle_verified(&mut self, bundle: &str, version: &str) {
        let mut line = [0u8; 128];
        let mut n = put(&mut line, 0, b"updated: component bundle verified (name=");
        n = put(&mut line, n, &bundle.as_bytes()[..bundle.len().min(48)]);
        n = put(&mut line, n, b"@");
        n = put(&mut line, n, &version.as_bytes()[..version.len().min(24)]);
        n = put(&mut line, n, b")\n");
        emit_bytes(&line[..n]);
    }
    fn restage_resume(&mut self, completed: usize, total: usize) {
        // `updated: restage resume (bundles=<k>/<n>)`
        let mut line = [0u8; 64];
        let mut n = put(&mut line, 0, b"updated: restage resume (bundles=");
        n = put_dec(&mut line, n, completed);
        n = put(&mut line, n, b"/");
        n = put_dec(&mut line, n, total);
        n = put(&mut line, n, b")\n");
        emit_bytes(&line[..n]);
    }
    fn bundle_reused(&mut self, bundle: &str, version: &str, sha8: &[u8; 8]) {
        let mut line = [0u8; 128];
        let mut n = put(&mut line, 0, b"updated: bundle reused (name=");
        n = put(&mut line, n, &bundle.as_bytes()[..bundle.len().min(48)]);
        n = put(&mut line, n, b"@");
        n = put(&mut line, n, &version.as_bytes()[..version.len().min(24)]);
        n = put(&mut line, n, b" sha=");
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for b in sha8 {
            line[n] = HEX[(b >> 4) as usize];
            line[n + 1] = HEX[(b & 0xf) as usize];
            n += 2;
        }
        n = put(&mut line, n, b")\n");
        emit_bytes(&line[..n]);
    }
}

fn put(line: &mut [u8], at: usize, bytes: &[u8]) -> usize {
    let take = bytes.len().min(line.len().saturating_sub(at));
    line[at..at + take].copy_from_slice(&bytes[..take]);
    at + take
}

fn put_dec(line: &mut [u8], at: usize, value: usize) -> usize {
    let mut digits = [0u8; 20];
    let mut d = 0;
    let mut v = value;
    loop {
        digits[d] = b'0' + (v % 10) as u8;
        d += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    let mut n = at;
    while d > 0 {
        d -= 1;
        if n < line.len() {
            line[n] = digits[d];
            n += 1;
        }
    }
    n
}
