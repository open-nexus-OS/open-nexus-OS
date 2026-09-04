// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
#![cfg(all(nexus_env = "os", feature = "os-lite"))]

//! CONTEXT: The verified **system volume** (RFC-0089 §12.3, ADR-0060,
//! TASK-0321 P2). bundlemgrd — a CORE service inside the loader-verified
//! boot image — is the volume's VERIFIER and READER: it attaches the system
//! slot paired with the MEASURED boot slot over the fixed-slot block plane,
//! verifies the NXSV against the build-baked OS keys (`BAKED_OS_KEYS`, the
//! same anchor `nxboot` holds), binds it to the measured image
//! (`boot_image_sha256`, same `rollback_index` line) and to the bounded
//! pkgimg v3 index (≤ 256 KiB, index digest), and serves a bundle's
//! `payload.elf` to init (the sole spawner) into a caller-provided VMO —
//! header LAST, only after the bytes hashed to the index entry digest.
//! Boot cost is O(NXSV + index); bundle digests are lazy per served bundle.
//! Fail-closed on every path: a direct-kernel boot (no measured record),
//! an unpaired volume, a bad signature or digest, a block plane that is not
//! up — all leave the volume UNTRUSTED (`STATUS_UNAVAILABLE`), and the
//! marker says why. Nothing here ever prints `verified` without the real
//! chain having run.
//! OWNERS: @runtime @security @reliability
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU ladder (`bundlemgrd: system volume verified …`,
//!   `init: spawn from volume svc=metricsd …`); codec/verify units live in
//!   `storage::pkgimg_bundles` + `bootfmt::nxsv`.
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

use alloc::vec::Vec;

use bootfmt::handoff::Handoff;
use sha2::{Digest, Sha256};
use storage::blockproto;
use storage::pkgimg::PkgImgCaps;
use storage::pkgimg_bundles::{parse_index, VolumeEntry, VolumeIndex, MAX_INDEX_BYTES_V3};
use storage::remote_blk::RemoteBlockDevice;
use storage::BlockDevice;

include!(concat!(env!("OUT_DIR"), "/os_trust_baked.rs"));

/// Volume payload starts at system-partition sector 8 (RFC-0089 §12.2).
const VOLUME_START_SECTOR: u64 = 8;
/// One block-plane request worth of bytes (12 sectors) — the streaming unit.
const CHUNK: usize = blockproto::MAX_BLOCKS_PER_REQ as usize * blockproto::SECTOR_SIZE;
/// Bounded attach window: virtioblkd may still be bringing the device up
/// when init asks for the first bundle (its MMIO grant lands late).
const ATTACH_DEADLINE_NS: u64 = 2_000_000_000;

/// Why the volume is not trusted — the marker vocabulary (RFC-0089 §12.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VolumeFail {
    /// No measured boot record (direct-kernel dev boot): pairing impossible.
    PairUnbound,
    /// Block plane / partition not reachable.
    Io,
    /// NXSV signature invalid (or zeroed / malformed descriptor).
    Sig,
    /// NXSV does not pair with the measured boot image or rollback line.
    Pair,
    /// Index length/digest or bundle-table bounds failed.
    Digest,
    Bounds,
}

impl VolumeFail {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::PairUnbound => "pair=unbound",
            Self::Io => "io",
            Self::Sig => "sig",
            Self::Pair => "pair",
            Self::Digest => "digest",
            Self::Bounds => "bounds",
        }
    }
}

/// The attached, verified volume.
pub(crate) struct Volume {
    dev: RemoteBlockDevice,
    pub(crate) slot: u8,
    pub(crate) build_id: [u8; 32],
    pub(crate) index: VolumeIndex,
    /// The NXSV-verified index bytes (superblock + index) — served verbatim
    /// to packagefsd (`GET_INDEX`), so every `pkg:/` view derives from the
    /// same digest-bound bytes bundlemgrd verified (one bundle authority).
    pub(crate) index_bytes: Vec<u8>,
    /// TASK-0321 P5: the installed-app registry, read from each bundle's
    /// `meta/app.properties` on the volume (label/icon/bundle_type) — the
    /// launcher list + the GET_PAYLOAD gate (only app bundles serve a
    /// ui-program payload).
    pub(crate) apps: Vec<AppInfo>,
}

/// One launchable-or-not app bundle on the volume (`meta/app.properties`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AppInfo {
    pub(crate) id: alloc::string::String,
    pub(crate) label: alloc::string::String,
    pub(crate) bundle_type: alloc::string::String,
    pub(crate) icon: alloc::string::String,
}

/// Largest `meta/app.properties` the registry reads (bounded input).
const APP_PROPERTIES_MAX: usize = 1024;

/// Parses `key=value` lines (`label`, `icon`, `bundle_type`) — the sidecar
/// `nx app compile --meta` writes. Missing keys fall back to the id / empty
/// / `app`; unknown keys are ignored (forward-compatible).
pub(crate) fn parse_app_properties(id: &str, text: &[u8]) -> AppInfo {
    use alloc::string::{String, ToString};
    let mut label: Option<String> = None;
    let mut icon = String::new();
    let mut bundle_type = "app".to_string();
    for line in text.split(|&b| b == b'\n') {
        let Ok(line) = core::str::from_utf8(line) else { continue };
        let line = line.trim();
        let Some((k, v)) = line.split_once('=') else { continue };
        match k.trim() {
            "label" => label = Some(v.trim().to_string()),
            "icon" => icon = v.trim().to_string(),
            "bundle_type" => bundle_type = v.trim().to_string(),
            _ => {}
        }
    }
    AppInfo {
        id: id.to_string(),
        label: label.unwrap_or_else(|| id.to_string()),
        bundle_type,
        icon,
    }
}

/// Verification state — attached lazily on the first volume op, then kept.
pub(crate) enum VolumeState {
    Unattached,
    Verified(Volume),
    Failed(VolumeFail),
}

impl VolumeState {
    pub(crate) const fn new() -> Self {
        Self::Unattached
    }

    /// The app registry (empty when the volume is not verified — recovery
    /// and direct-kernel boots launch nothing).
    pub(crate) fn apps(&self) -> &[AppInfo] {
        match self {
            Self::Verified(v) => &v.apps,
            _ => &[],
        }
    }

    /// Attaches + verifies once; later calls answer from the kept result.
    /// `Failed` is sticky for this boot — a volume that failed to verify
    /// never becomes trusted by retrying (the reason was printed once).
    pub(crate) fn ensure(&mut self) -> Result<&mut Volume, VolumeFail> {
        if matches!(self, Self::Unattached) {
            *self = match attach_and_verify() {
                Ok(v) => {
                    emit_verified(&v);
                    Self::Verified(v)
                }
                Err(fail) => {
                    emit_fail(fail);
                    Self::Failed(fail)
                }
            };
        }
        match self {
            Self::Verified(v) => Ok(v),
            Self::Failed(f) => Err(*f),
            Self::Unattached => Err(VolumeFail::Io),
        }
    }
}

fn measured() -> Result<Handoff, VolumeFail> {
    let raw = nexus_abi::boot_measured_read().ok_or(VolumeFail::PairUnbound)?;
    bootfmt::handoff::decode(&raw).map_err(|_| VolumeFail::PairUnbound)
}

fn attach_and_verify() -> Result<Volume, VolumeFail> {
    let handoff = measured()?;
    let (part, slot) = match handoff.boot_slot {
        bootfmt::bsb::Slot::A => (blockproto::PART_SYSTEM_A, b'a'),
        bootfmt::bsb::Slot::B => (blockproto::PART_SYSTEM_B, b'b'),
    };
    let dev = RemoteBlockDevice::open_with_deadline(
        blockproto::CLIENT_REQ_SLOT,
        blockproto::CLIENT_REPLY_SEND_SLOT,
        blockproto::CLIENT_REPLY_RECV_SLOT,
        part,
        ATTACH_DEADLINE_NS,
    )
    .ok_or(VolumeFail::Io)?;

    // NXSV: sector 0, verified against the baked OS anchor (fail closed).
    let mut sector = [0u8; blockproto::SECTOR_SIZE];
    dev.read_blocks(0, &mut sector).map_err(|_| VolumeFail::Io)?;
    let mut desc = None;
    for key in BAKED_OS_KEYS {
        if let Ok(d) = bootfmt::nxsv::verify(&sector, key) {
            desc = Some(d);
            break;
        }
    }
    let desc = desc.ok_or(VolumeFail::Sig)?;

    // Pairing: system-X belongs to the measured boot image on the same
    // rollback line (the loader already enforced that line ≥ the floor).
    if desc.boot_image_sha256 != handoff.image_sha256
        || desc.rollback_index != handoff.rollback_index
    {
        return Err(VolumeFail::Pair);
    }

    // Bounded index read: superblock + index only (the boot-time read).
    let index_len = desc.index_len as usize;
    if index_len == 0 || index_len > MAX_INDEX_BYTES_V3 || desc.volume_size < desc.index_len as u64
    {
        return Err(VolumeFail::Bounds);
    }
    let padded = index_len.div_ceil(blockproto::SECTOR_SIZE) * blockproto::SECTOR_SIZE;
    let budget =
        dev.block_count().saturating_sub(VOLUME_START_SECTOR) * blockproto::SECTOR_SIZE as u64;
    if padded as u64 > budget || desc.volume_size > budget {
        return Err(VolumeFail::Bounds);
    }
    let mut head = Vec::new();
    head.resize(padded, 0);
    dev.read_blocks(VOLUME_START_SECTOR, &mut head).map_err(|_| VolumeFail::Io)?;
    head.truncate(index_len);
    if Sha256::digest(&head).as_slice() != desc.index_sha256 {
        return Err(VolumeFail::Digest);
    }
    let index = parse_index(&head, &PkgImgCaps::default()).map_err(|_| VolumeFail::Digest)?;
    if index.superblock.index_end() != index_len {
        return Err(VolumeFail::Digest);
    }
    let mut volume =
        Volume { dev, slot, build_id: desc.build_id, index, index_bytes: head, apps: Vec::new() };
    volume.build_app_registry()?;
    Ok(volume)
}

impl Volume {
    /// The bundle-table row + its `payload.elf` entry for `name` (latest
    /// row by sorted order = deterministic; the volume carries one version
    /// per bundle by construction of the builder).
    pub(crate) fn lookup(
        &self,
        name: &[u8],
    ) -> Option<(&storage::pkgimg_bundles::VolumeBundle, &VolumeEntry)> {
        let row = self.index.bundles.iter().find(|b| b.bundle.as_bytes() == name)?;
        let entry = self.lookup_entry(name, b"payload.elf")?;
        Some((row, entry))
    }

    /// Any entry of the bundle's active (only) version by relative path.
    pub(crate) fn lookup_entry(&self, bundle: &[u8], path: &[u8]) -> Option<&VolumeEntry> {
        let row = self.index.bundles.iter().find(|b| b.bundle.as_bytes() == bundle)?;
        self.index.entries.iter().find(|e| {
            e.bundle == row.bundle && e.version == row.version && e.path.as_bytes() == path
        })
    }

    /// The app registry entry for `id`, if the bundle is an app bundle.
    pub(crate) fn app(&self, id: &[u8]) -> Option<&AppInfo> {
        self.apps.iter().find(|a| a.id.as_bytes() == id)
    }

    /// Reads one SMALL entry (≤ `max` bytes) through the block plane and
    /// checks it against the index digest — the registry sidecars.
    fn read_entry_small(&self, entry: &VolumeEntry, max: usize) -> Result<Vec<u8>, VolumeFail> {
        let len = entry.data_len as usize;
        if len > max {
            return Err(VolumeFail::Bounds);
        }
        let start = self.index.superblock.data_offset as u64 + entry.data_offset;
        let lba = VOLUME_START_SECTOR + start / blockproto::SECTOR_SIZE as u64;
        let skew = (start % blockproto::SECTOR_SIZE as u64) as usize;
        let sectors = (skew + len).div_ceil(blockproto::SECTOR_SIZE);
        let mut buf = Vec::new();
        buf.resize(sectors * blockproto::SECTOR_SIZE, 0);
        self.dev.read_blocks(lba, &mut buf).map_err(|_| VolumeFail::Io)?;
        let bytes = buf[skew..skew + len].to_vec();
        if Sha256::digest(&bytes).as_slice() != entry.sha256 {
            return Err(VolumeFail::Digest);
        }
        Ok(bytes)
    }

    /// Builds the app registry from every bundle carrying
    /// `meta/app.properties`; a sidecar that fails its digest makes the
    /// whole volume untrusted (the index bound it).
    fn build_app_registry(&mut self) -> Result<(), VolumeFail> {
        let mut apps = Vec::new();
        for row in &self.index.bundles {
            let Some(entry) = self.index.entries.iter().find(|e| {
                e.bundle == row.bundle
                    && e.version == row.version
                    && e.path == "meta/app.properties"
            }) else {
                continue;
            };
            let text = self.read_entry_small(entry, APP_PROPERTIES_MAX)?;
            apps.push(parse_app_properties(&row.bundle, &text));
        }
        self.apps = apps;
        Ok(())
    }

    /// Bulk-reads `entry`'s bytes into `vmo` at `data_offset` (one block-plane
    /// round trip) and hashes them back out of the VMO; returns `Ok(len)`
    /// only if the bytes hashed to the index digest — `(len, read_ms, hash_ms)`
    /// so the serve marker locates the cost (device path vs digest). The
    /// caller writes the header afterwards (header-last discipline).
    pub(crate) fn stream_entry_into_vmo(
        &self,
        entry: &VolumeEntry,
        vmo: u32,
        data_offset: usize,
    ) -> Result<(u32, u32, u32), VolumeFail> {
        let start = self.index.superblock.data_offset as u64 + entry.data_offset;
        let total = entry.data_len as usize;
        // TASK-0321 P4b: ONE bulk round trip — virtioblkd streams the entry's
        // byte range straight into the destination VMO (device runs, no IPC
        // per 6 KiB); the digest is then taken from the VMO itself (what the
        // consumer will map), header-last discipline unchanged. The arm is
        // released on every exit path.
        let part_byte_off = VOLUME_START_SECTOR * blockproto::SECTOR_SIZE as u64 + start;
        let t0 = nexus_abi::nsec().unwrap_or(0);
        self.dev.arm_vmo(vmo).map_err(|_| VolumeFail::Io)?;
        let bulk = self.dev.read_into_vmo(part_byte_off, total as u64, data_offset as u64);
        let released = self.dev.release_vmo();
        bulk.map_err(|_| VolumeFail::Io)?;
        released.map_err(|_| VolumeFail::Io)?;
        let t1 = nexus_abi::nsec().unwrap_or(0);
        let mut hasher = Sha256::new();
        let mut buf = [0u8; CHUNK];
        let mut done = 0usize;
        while done < total {
            let want = core::cmp::min(total - done, CHUNK);
            nexus_abi::vmo_read(vmo, data_offset + done, &mut buf[..want])
                .map_err(|_| VolumeFail::Bounds)?;
            hasher.update(&buf[..want]);
            done += want;
        }
        let digest: [u8; 32] = hasher.finalize().into();
        let t2 = nexus_abi::nsec().unwrap_or(0);
        if digest != entry.sha256 {
            return Err(VolumeFail::Digest);
        }
        let ms = |a: u64, b: u64| (b.saturating_sub(a) / 1_000_000) as u32;
        Ok((total as u32, ms(t0, t1), ms(t1, t2)))
    }
}

fn emit_verified(v: &Volume) {
    // `bundlemgrd: system volume verified (slot=a build=<id8> bundles=N)`
    let mut line = [0u8; 96];
    let mut n = 0;
    for b in b"bundlemgrd: system volume verified (slot=" {
        line[n] = *b;
        n += 1;
    }
    line[n] = v.slot;
    n += 1;
    for b in b" build=" {
        line[n] = *b;
        n += 1;
    }
    for b in v.build_id.iter().take(8) {
        if *b == 0 {
            break;
        }
        line[n] = *b;
        n += 1;
    }
    for b in b" bundles=" {
        line[n] = *b;
        n += 1;
    }
    let count = v.index.bundles.len().min(999);
    let mut digits = [0u8; 3];
    let mut d = 0;
    let mut rest = count;
    loop {
        digits[d] = b'0' + (rest % 10) as u8;
        d += 1;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    while d > 0 {
        d -= 1;
        line[n] = digits[d];
        n += 1;
    }
    line[n] = b')';
    n += 1;
    if let Ok(s) = core::str::from_utf8(&line[..n]) {
        crate::os_lite::emit_line(s);
    }
}

fn emit_fail(fail: VolumeFail) {
    // `bundlemgrd: system volume FAIL (<reason>)` — bounded, label ≤ 12 B.
    let mut line = [0u8; 64];
    let prefix = b"bundlemgrd: system volume FAIL (";
    line[..prefix.len()].copy_from_slice(prefix);
    let mut n = prefix.len();
    for b in fail.label().bytes() {
        line[n] = b;
        n += 1;
    }
    line[n] = b')';
    n += 1;
    if let Ok(s) = core::str::from_utf8(&line[..n]) {
        crate::os_lite::emit_line(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_properties_parse_and_defaults() {
        let a = parse_app_properties(
            "calculator",
            b"label=Calculator\nicon=calculator|#3d4757|#1a202c\nbundle_type=app\nunknown=x\n",
        );
        assert_eq!(a.id, "calculator");
        assert_eq!(a.label, "Calculator");
        assert_eq!(a.icon, "calculator|#3d4757|#1a202c");
        assert_eq!(a.bundle_type, "app");
        // Missing keys: label = id, icon empty, type app. Garbage lines ignored.
        let b = parse_app_properties("stash", b"\xff\xfe\nnot a pair\n");
        assert_eq!(
            (b.label.as_str(), b.icon.as_str(), b.bundle_type.as_str()),
            ("stash", "", "app")
        );
    }
}
