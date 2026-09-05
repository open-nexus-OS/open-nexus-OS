// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The system-volume ASSEMBLER (RFC-0089 §12.4, TASK-0321 P3) —
//! the [`ComponentSink`] that turns a bundle set (`system-volume` kind 6
//! and `bundle` kind 2 components) into the INACTIVE system volume, byte-
//! identical to the host build. Generic over a sector device so the EXACT
//! device logic is host-proven (`tests/updates_host`, the `DeltaAdapter`
//! pattern): the OS half only supplies the block-plane devices and the
//! marker events. Discipline: invalidate the NXSV first; the index lands
//! from sector 8 and is readback-verified + parsed; every shipped bundle
//! lands at its index window (sector-aligned by the v3 rule) and is
//! readback-verified; at the SET COMMIT every index bundle the set did
//! not ship is copied from the ACTIVE volume while being hashed against
//! the NEW index (signature-bound), the whole volume is readback-hashed
//! against the NXSV, and the NXSV lands LAST. A torn stage therefore
//! never yields a valid volume.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/updates_host/tests/component_set_volume.rs (accept
//!   and reuse, `test_reject_*` order/membership/digest/binding, power-cut
//!   matrix, journal resume, byte-identity with the host build)
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

#[cfg(all(feature = "os-lite", not(feature = "std")))]
use alloc::{string::String, vec, vec::Vec};
#[cfg(feature = "std")]
use std::{string::String, vec, vec::Vec};

use crate::stage_journal::{Journal, JOURNAL_SECTOR};
use sha2::{Digest, Sha256};
use storage::pkgimg::PkgImgCaps;
use storage::pkgimg_bundles::{parse_index, parse_superblock, VolumeIndex, MAX_INDEX_BYTES_V3};

use crate::component_set::{
    ComponentMeta, ComponentSink, RejectReason, KIND_BUNDLE, KIND_SYSTEM_VOLUME,
};

/// Sector size of the block plane.
pub const SECTOR: usize = 512;
/// Volume payload starts at system-partition sector 8 (RFC-0089 §12.2).
pub const VOLUME_START_SECTOR: u64 = 8;
/// v3 window alignment (`storage::pkgimg` ALIGNMENT) — windows and the
/// data section are 4 KiB-aligned in the volume.
const WINDOW_ALIGN: u64 = 4096;
/// Readback / copy scratch (sector multiple).
const SCRATCH: usize = 32 * SECTOR;

/// A partition-scoped sector device (the OS wraps `RemoteBlockDevice`).
pub trait VolumeDev {
    fn block_count(&self) -> u64;
    /// `buf.len()` is a sector multiple.
    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), RejectReason>;
    /// `buf.len()` is a sector multiple.
    fn write(&mut self, lba: u64, buf: &[u8]) -> Result<(), RejectReason>;
    fn sync(&mut self) -> Result<(), RejectReason>;
}

/// Progress events — the OS prints markers, the host collects.
pub trait VolumeEvents {
    fn volume_verified(&mut self, build_id: &str, bundles: usize);
    fn bundle_verified(&mut self, bundle: &str, version: &str);
    fn bundle_reused(&mut self, bundle: &str, version: &str, sha8: &[u8; 8]);
    /// TASK-0035 P1: a restage found `completed` of `total` bundle windows
    /// journalled AND readback-verified — they will not be rewritten.
    fn restage_resume(&mut self, _completed: usize, _total: usize) {}
    /// TASK-0035 P3: a `bundle-delta` component reconstructed `bundle` from
    /// the active volume's window (the window itself was then verified like
    /// any shipped one — `bundle_verified` follows).
    fn bundle_reconstructed(&mut self, _bundle: &str) {}
}

/// The events sink that records nothing.
pub struct NoEvents;
impl VolumeEvents for NoEvents {
    fn volume_verified(&mut self, _build_id: &str, _bundles: usize) {}
    fn bundle_verified(&mut self, _bundle: &str, _version: &str) {}
    fn bundle_reused(&mut self, _bundle: &str, _version: &str, _sha8: &[u8; 8]) {}
}

struct Current {
    kind: u8,
    next_sector: u64,
    written: u64,
    /// Row index for kind 2.
    row: Option<usize>,
    /// Absolute volume byte offset of the window / index start.
    start: u64,
    /// TASK-0035 P1: the window is journalled + readback-verified — the
    /// component is streamed (the engine hashes it) but not rewritten.
    skip: bool,
}

/// See the module doc.
pub struct VolumeAssembler<D: VolumeDev, A: VolumeDev, E: VolumeEvents> {
    inactive: D,
    /// The ACTIVE volume (read-only) for unchanged-bundle reuse.
    active: Option<A>,
    events: E,
    /// Pairing: the boot image digest this volume must belong to (the
    /// set's boot image, else the ACTIVE NXBD's).
    pair_with: Option<[u8; 32]>,
    nxsv_sector: Vec<u8>,
    desc: Option<bootfmt::nxsv::Nxsv>,
    index: Option<VolumeIndex>,
    populated: Vec<bool>,
    partial: Vec<u8>,
    cur: Option<Current>,
    /// The ACTIVE volume's index, read + parsed ONCE per set (a locator
    /// only — every reused byte is re-hashed). Per-bundle re-reads leaked
    /// a parsed index each on the OS bump heap (never frees) and killed
    /// `updated` at the 13th reuse.
    active_index: Option<VolumeIndex>,
    /// One scratch buffer for every streaming loop (allocated once).
    scratch: Vec<u8>,
    /// TASK-0035 P1: the stage journal for the target being assembled.
    journal: Option<Journal>,
}

impl<D: VolumeDev, A: VolumeDev, E: VolumeEvents> VolumeAssembler<D, A, E> {
    pub fn new(inactive: D, active: Option<A>, pair_with: Option<[u8; 32]>, events: E) -> Self {
        Self {
            inactive,
            active,
            events,
            pair_with,
            nxsv_sector: Vec::new(),
            desc: None,
            index: None,
            populated: Vec::new(),
            partial: Vec::new(),
            cur: None,
            active_index: None,
            scratch: vec![0u8; SCRATCH],
            journal: None,
        }
    }

    pub fn events(&self) -> &E {
        &self.events
    }

    pub fn events_mut(&mut self) -> &mut E {
        &mut self.events
    }

    /// Hands the inactive device back (tests inspect the assembled bytes).
    pub fn into_inactive(self) -> D {
        self.inactive
    }

    /// TASK-0035 P3: the ACTIVE volume window with digest `sha` as
    /// `(volume byte offset, length)` — the `bundle-delta` base locator.
    /// Absent → `delta-base` (before any write).
    pub fn active_window(&mut self, sha: &[u8; 32]) -> Result<(u64, u64), RejectReason> {
        let active = self.active.as_mut().ok_or(RejectReason::DeltaBase)?;
        if self.active_index.is_none() {
            self.active_index = Some(read_active_index(active)?);
        }
        let index = self.active_index.as_ref().ok_or(RejectReason::Io)?;
        let b = index.bundle_by_sha(sha).ok_or(RejectReason::DeltaBase)?;
        Ok((index.superblock.data_offset as u64 + b.data_offset, b.data_len))
    }

    fn body_budget(&self) -> u64 {
        self.inactive.block_count().saturating_sub(VOLUME_START_SECTOR) * SECTOR as u64
    }

    fn sector_of(byte: u64) -> u64 {
        VOLUME_START_SECTOR + byte / SECTOR as u64
    }

    /// Persists the journal (sector 1) — after every verified window, so a
    /// power cut right after loses at most the window in flight.
    fn persist_journal(&mut self) -> Result<(), RejectReason> {
        if let Some(j) = &self.journal {
            let sector = j.encode();
            self.inactive.write(JOURNAL_SECTOR, &sector)?;
            self.inactive.sync()?;
        }
        Ok(())
    }

    /// Marks `row` completed in the journal and persists it.
    fn journal_done(&mut self, row: usize) -> Result<(), RejectReason> {
        if let Some(j) = &mut self.journal {
            j.set_done(row, true);
        }
        self.persist_journal()
    }

    /// TASK-0035 P1: reads the journal for THIS target; every journalled
    /// window is readback-verified against the NEW index before it counts
    /// (a stale bit is cleared, never trusted). A journal for another target
    /// (or a corrupt one) is zeroed. Returns the resumed count.
    fn load_journal(&mut self, desc: &bootfmt::nxsv::Nxsv) -> Result<usize, RejectReason> {
        let index = self.index.as_ref().ok_or(RejectReason::Io)?;
        let n = index.bundles.len();
        let mut sector = [0u8; SECTOR];
        self.inactive.read(JOURNAL_SECTOR, &mut sector)?;
        let fresh = Journal::new(desc.volume_sha256, desc.index_sha256, n);
        let found = Journal::decode(&sector).ok();
        let mut journal = match (found, fresh) {
            (Some(j), _) if j.binds(&desc.volume_sha256, &desc.index_sha256, n) => j,
            (_, Some(f)) => {
                if sector.iter().any(|&b| b != 0) {
                    // Another target's (or a torn) journal: zero it first.
                    self.inactive.write(JOURNAL_SECTOR, &[0u8; SECTOR])?;
                    self.inactive.sync()?;
                }
                f
            }
            (_, None) => return Err(RejectReason::Bounds),
        };
        let mut resumed = 0;
        let rows: Vec<(u64, u64, [u8; 32])> = index
            .bundles
            .iter()
            .map(|b| (index.superblock.data_offset as u64 + b.data_offset, b.data_len, b.sha256))
            .collect();
        for (row, (start, len, sha)) in rows.into_iter().enumerate() {
            if !journal.is_done(row) {
                continue;
            }
            if self.hash_range(start, len)? == sha {
                self.populated[row] = true;
                resumed += 1;
            } else {
                journal.set_done(row, false);
            }
        }
        self.journal = Some(journal);
        Ok(resumed)
    }

    /// Flushes the sector carry, zero-padding up to `pad_to` (absolute
    /// volume byte, itself a multiple of the alignment) so every byte the
    /// host build wrote as padding is written here as zero, never stale.
    fn flush_to(&mut self, pad_to: u64) -> Result<(), RejectReason> {
        let cur = self.cur.as_mut().ok_or(RejectReason::Io)?;
        let end = cur.start + cur.written;
        if pad_to < end {
            return Err(RejectReason::Bounds);
        }
        let pad = (pad_to - end) as usize;
        let mut tail = core::mem::take(&mut self.partial);
        tail.resize(tail.len() + pad, 0);
        if !tail.is_empty() {
            if tail.len() % SECTOR != 0 {
                return Err(RejectReason::Bounds);
            }
            self.inactive.write(cur.next_sector, &tail)?;
            cur.next_sector += (tail.len() / SECTOR) as u64;
        }
        Ok(())
    }

    /// Streams `len` bytes from `byte` of the inactive volume into a hasher.
    fn hash_range(&mut self, byte: u64, len: u64) -> Result<[u8; 32], RejectReason> {
        let mut hasher = Sha256::new();
        let mut remaining = len;
        let mut lba = Self::sector_of(byte);
        if byte % SECTOR as u64 != 0 {
            return Err(RejectReason::Bounds);
        }
        let mut buf = core::mem::take(&mut self.scratch);
        while remaining > 0 {
            let take = remaining.min(buf.len() as u64) as usize;
            let padded = take.div_ceil(SECTOR) * SECTOR;
            if let Err(e) = self.inactive.read(lba, &mut buf[..padded]) {
                self.scratch = buf;
                return Err(e);
            }
            hasher.update(&buf[..take]);
            lba += (padded / SECTOR) as u64;
            remaining -= take as u64;
        }
        self.scratch = buf;
        let mut out = [0u8; 32];
        out.copy_from_slice(&hasher.finalize());
        Ok(out)
    }

    /// Copies one unchanged bundle window from the ACTIVE volume into the
    /// inactive one at the NEW index offset, hashing against the NEW
    /// index digest while copying (never trusting the old index).
    fn reuse_from_active(&mut self, row: usize) -> Result<(), RejectReason> {
        let index = self.index.as_ref().ok_or(RejectReason::Io)?;
        let target = &index.bundles[row];
        let new_start = index.superblock.data_offset as u64 + target.data_offset;
        let len = target.data_len;
        let want_sha = target.sha256;
        let (name, version) = (target.bundle.clone(), target.version.clone());
        let active = self.active.as_mut().ok_or(RejectReason::BundleNotInIndex)?;
        // Locate the window in the ACTIVE volume by digest (its index is a
        // locator only; every byte is re-hashed below). Read once per set.
        if self.active_index.is_none() {
            self.active_index = Some(read_active_index(active)?);
        }
        let active_index = self.active_index.as_ref().ok_or(RejectReason::Io)?;
        let src = active_index.bundle_by_sha(&want_sha).ok_or(RejectReason::BundleNotInIndex)?;
        if src.data_len != len {
            return Err(RejectReason::BundleNotInIndex);
        }
        let src_start = active_index.superblock.data_offset as u64 + src.data_offset;
        if src_start % SECTOR as u64 != 0 || new_start % SECTOR as u64 != 0 {
            return Err(RejectReason::Bounds);
        }
        let mut hasher = Sha256::new();
        let mut buf = core::mem::take(&mut self.scratch);
        let mut done = 0u64;
        let copied = (|| -> Result<(), RejectReason> {
            while done < len {
                let take = (len - done).min(buf.len() as u64) as usize;
                let padded = take.div_ceil(SECTOR) * SECTOR;
                active.read(Self::sector_of(src_start + done), &mut buf[..padded])?;
                hasher.update(&buf[..take]);
                // Zero the tail of the last sector (host padding is zero).
                for b in &mut buf[take..padded] {
                    *b = 0;
                }
                self.inactive.write(Self::sector_of(new_start + done), &buf[..padded])?;
                done += take as u64;
            }
            Ok(())
        })();
        // Zero-pad up to the window's 4 KiB boundary (stale bytes there
        // would break the whole-volume digest) — from the same buffer.
        let end = new_start + len;
        let pad_to = end.div_ceil(WINDOW_ALIGN) * WINDOW_ALIGN;
        let written_end = end.div_ceil(SECTOR as u64) * SECTOR as u64;
        let padded_ok = copied.and_then(|()| {
            if pad_to > written_end {
                let n = (pad_to - written_end) as usize;
                for b in &mut buf[..n] {
                    *b = 0;
                }
                self.inactive.write(Self::sector_of(written_end), &buf[..n])?;
            }
            Ok(())
        });
        self.scratch = buf;
        padded_ok?;
        let mut got = [0u8; 32];
        got.copy_from_slice(&hasher.finalize());
        if got != want_sha {
            return Err(RejectReason::VolumeDigest);
        }
        // Durable BEFORE the marker: a cut right after `bundle reused`
        // resumes past this window.
        self.journal_done(row)?;
        let mut sha8 = [0u8; 8];
        sha8.copy_from_slice(&want_sha[..8]);
        self.events.bundle_reused(&name, &version, &sha8);
        Ok(())
    }
}

/// Reads and parses the ACTIVE volume's superblock + index (bounded).
pub fn read_active_index<A: VolumeDev>(active: &mut A) -> Result<VolumeIndex, RejectReason> {
    let mut first = [0u8; SECTOR];
    active.read(VOLUME_START_SECTOR, &mut first)?;
    let sb = parse_superblock(&first, &PkgImgCaps::default())
        .map_err(|_| RejectReason::BundleNotInIndex)?;
    let index_end = sb.index_end();
    if index_end > MAX_INDEX_BYTES_V3 {
        return Err(RejectReason::Bounds);
    }
    let padded = index_end.div_ceil(SECTOR) * SECTOR;
    let mut head = vec![0u8; padded];
    active.read(VOLUME_START_SECTOR, &mut head)?;
    head.truncate(index_end);
    parse_index(&head, &PkgImgCaps::default()).map_err(|_| RejectReason::BundleNotInIndex)
}

impl<D: VolumeDev, A: VolumeDev, E: VolumeEvents> ComponentSink for VolumeAssembler<D, A, E> {
    fn begin(&mut self, meta: &ComponentMeta) -> Result<(), RejectReason> {
        self.partial.clear();
        match meta.kind {
            KIND_SYSTEM_VOLUME => {
                let (desc, _sig) = bootfmt::nxsv::decode(&meta.kind_data)
                    .map_err(|_| RejectReason::VolumeBinding)?;
                if let Some(pair) = self.pair_with {
                    if desc.boot_image_sha256 != pair {
                        return Err(RejectReason::VolumeBinding);
                    }
                }
                if desc.volume_size > self.body_budget()
                    || meta.size as usize > MAX_INDEX_BYTES_V3
                    || u64::from(desc.index_len) != meta.size
                {
                    return Err(RejectReason::Bounds);
                }
                // Invalidate the commit point FIRST (NXSV-last discipline).
                self.inactive.write(0, &[0u8; SECTOR])?;
                self.inactive.sync()?;
                self.nxsv_sector = meta.kind_data.clone();
                self.desc = Some(desc);
                self.index = None;
                self.populated.clear();
                self.journal = None;
                self.cur = Some(Current {
                    kind: KIND_SYSTEM_VOLUME,
                    next_sector: VOLUME_START_SECTOR,
                    written: 0,
                    row: None,
                    start: 0,
                    skip: false,
                });
                Ok(())
            }
            KIND_BUNDLE => {
                let index = self.index.as_ref().ok_or(RejectReason::Order)?;
                let row = index
                    .bundles
                    .iter()
                    .position(|b| b.sha256 == meta.sha256 && b.data_len == meta.size)
                    .ok_or(RejectReason::BundleNotInIndex)?;
                let start = index.superblock.data_offset as u64 + index.bundles[row].data_offset;
                if start % SECTOR as u64 != 0 {
                    return Err(RejectReason::Bounds);
                }
                // TASK-0035 P1: a journalled + readback-verified window is
                // streamed (the engine hashes every component) but not
                // rewritten.
                let skip = self.populated.get(row).copied().unwrap_or(false);
                self.cur = Some(Current {
                    kind: KIND_BUNDLE,
                    next_sector: Self::sector_of(start),
                    written: 0,
                    row: Some(row),
                    start,
                    skip,
                });
                Ok(())
            }
            _ => Err(RejectReason::KindUnsupported),
        }
    }

    fn chunk(&mut self, _offset: u64, bytes: &[u8]) -> Result<(), RejectReason> {
        let cur = self.cur.as_mut().ok_or(RejectReason::Io)?;
        if cur.skip {
            cur.written += bytes.len() as u64;
            return Ok(());
        }
        self.partial.extend_from_slice(bytes);
        let full = self.partial.len() / SECTOR * SECTOR;
        if full > 0 {
            self.inactive.write(cur.next_sector, &self.partial[..full])?;
            cur.next_sector += (full / SECTOR) as u64;
            self.partial.drain(..full);
        }
        cur.written += bytes.len() as u64;
        Ok(())
    }

    fn finish(&mut self, meta: &ComponentMeta) -> Result<(), RejectReason> {
        let (kind, start, written, row, skip) = {
            let cur = self.cur.as_ref().ok_or(RejectReason::Io)?;
            (cur.kind, cur.start, cur.written, cur.row, cur.skip)
        };
        if written != meta.size {
            return Err(RejectReason::Bounds);
        }
        match kind {
            KIND_SYSTEM_VOLUME => {
                // The data section starts 4 KiB-aligned: pad the index tail
                // with zeros exactly as the host build did.
                let pad_to = (start + written).div_ceil(WINDOW_ALIGN) * WINDOW_ALIGN;
                self.flush_to(pad_to)?;
                if self.hash_range(0, written)? != meta.sha256 {
                    return Err(RejectReason::Digest);
                }
                // Parse the index FROM THE DISK (what bundlemgrd will read).
                let padded = (written as usize).div_ceil(SECTOR) * SECTOR;
                let mut head = vec![0u8; padded];
                self.inactive.read(VOLUME_START_SECTOR, &mut head)?;
                head.truncate(written as usize);
                let index = parse_index(&head, &PkgImgCaps::default())
                    .map_err(|_| RejectReason::VolumeBinding)?;
                let desc = self.desc.ok_or(RejectReason::Io)?;
                if index.superblock.index_end() as u64 != written
                    || index.superblock.data_offset as u64 != pad_to
                {
                    return Err(RejectReason::VolumeBinding);
                }
                // Every window must lie inside the NXSV-declared volume.
                for b in &index.bundles {
                    let end = index.superblock.data_offset as u64 + b.data_offset + b.data_len;
                    if end > desc.volume_size || b.data_offset % WINDOW_ALIGN != 0 {
                        return Err(RejectReason::VolumeBinding);
                    }
                }
                let total = index.bundles.len();
                self.populated = vec![false; total];
                self.events.volume_verified(desc.build_id_str(), total);
                self.index = Some(index);
                self.cur = None;
                // TASK-0035 P1: resume from the stage journal (every journalled
                // window readback-verified against THIS index first).
                let resumed = self.load_journal(&desc)?;
                if resumed > 0 {
                    self.events.restage_resume(resumed, total);
                }
                Ok(())
            }
            KIND_BUNDLE => {
                let row = row.ok_or(RejectReason::Io)?;
                if skip {
                    // Journalled + verified on resume: streamed (the engine
                    // hashed it), not rewritten. The size still has to match.
                    self.partial.clear();
                    self.cur = None;
                    return Ok(());
                }
                let pad_to = (start + written).div_ceil(WINDOW_ALIGN) * WINDOW_ALIGN;
                self.flush_to(pad_to)?;
                if self.hash_range(start, written)? != meta.sha256 {
                    return Err(RejectReason::Digest);
                }
                self.populated[row] = true;
                let (name, version) = {
                    let b = &self.index.as_ref().ok_or(RejectReason::Io)?.bundles[row];
                    (b.bundle.clone(), b.version.clone())
                };
                self.journal_done(row)?;
                self.events.bundle_verified(&name, &version);
                self.cur = None;
                Ok(())
            }
            _ => Err(RejectReason::KindUnsupported),
        }
    }

    fn commit_set(&mut self) -> Result<(), RejectReason> {
        let desc = match self.desc {
            Some(d) if self.index.is_some() => d,
            // No volume in this set (boot-slot-only): nothing to commit.
            _ => return Ok(()),
        };
        let missing: Vec<usize> =
            self.populated.iter().enumerate().filter(|(_, p)| !**p).map(|(i, _)| i).collect();
        for row in missing {
            self.reuse_from_active(row)?;
            self.populated[row] = true;
        }
        // Whole-volume readback against the NXSV — the same bytes the host
        // build produced, whether shipped or reused.
        if self.hash_range(0, desc.volume_size)? != desc.volume_sha256 {
            return Err(RejectReason::VolumeDigest);
        }
        // Commit point: the signed NXSV lands VERBATIM, last.
        let sector = self.nxsv_sector.clone();
        self.inactive.write(0, &sector)?;
        self.inactive.sync()?;
        // A valid NXSV supersedes the journal — zero it (a later stage of
        // another target starts clean; a cut here leaves a harmless stale
        // journal that will not bind).
        self.inactive.write(JOURNAL_SECTOR, &[0u8; SECTOR])?;
        self.inactive.sync()?;
        self.journal = None;
        Ok(())
    }
}

/// Convenience for callers that need the bundle names of a verified set.
pub fn bundle_names(index: &VolumeIndex) -> Vec<String> {
    index.bundles.iter().map(|b| b.bundle.clone()).collect()
}
