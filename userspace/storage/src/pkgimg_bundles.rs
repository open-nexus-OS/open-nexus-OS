// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `pkgimg` **v3** — the system-volume image (RFC-0089 §12.1,
//! TASK-0321). Same superblock and per-file index as v2 (`pkgimg.rs`),
//! evolved additively: every file entry carries its sha256, and a
//! **bundle table** follows the entries — one row per `(bundle, version)`
//! with the bundle's contiguous data window, its window sha256 (THE reuse
//! unit for TASK-0035 and the digest a kind-2 `bundle` component binds),
//! and the launch parameters (`stack_pages`, `global_pointer`) init needs
//! to spawn a service from the volume without parsing ELF at runtime.
//! Deterministic builder (sorted, 4 KiB-aligned windows, byte-identical
//! for identical inputs) + bounded parser that can run on the SUPERBLOCK +
//! INDEX ALONE (`parse_index`) — bundlemgrd verifies the index at boot and
//! bundle digests lazily per served bundle (§12.3), so boot cost stays
//! O(index).
//! OWNERS: @runtime @storage @reliability
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `userspace/storage/tests/pkgimg_v3.rs` (roundtrip,
//!   determinism, windows, `test_reject_*` for entry/bundle digests and
//!   out-of-bounds tables)

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use sha2::{Digest, Sha256};

use crate::pkgimg::{
    align_up, read_exact, read_u16, read_u32, read_u64, sanitize_component, sanitize_path,
    write_u16, write_u32, write_u64, PkgImgCaps, PkgImgError, PkgImgFileSpec, ALIGNMENT,
    SUPERBLOCK_LEN,
};

/// v3 magic (the superblock layout is identical to v2).
pub const MAGIC_V3: &[u8; 8] = b"PKGIMGV3";
/// v3 version field.
pub const VERSION_V3: u16 = 3;
/// Hard cap on the superblock + index bytes a verifier reads at boot
/// (RFC-0089 §12.4: the `system-volume` component payload).
pub const MAX_INDEX_BYTES_V3: usize = 256 * 1024;

/// Launch parameters for one `(bundle, version)` — how init spawns it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BundleLaunch {
    pub bundle: String,
    pub version: String,
    /// Stack pages for the spawned task (0 = data-only bundle, not spawnable).
    pub stack_pages: u32,
    /// RISC-V `gp` for the payload ELF (0 = data-only bundle).
    pub global_pointer: u64,
}

/// Parsed superblock (both versions share the layout).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Superblock {
    pub version: u16,
    pub index_offset: usize,
    pub index_len: usize,
    pub data_offset: usize,
    pub data_len: usize,
    pub index_sha256: [u8; 32],
}

impl Superblock {
    /// Byte length of superblock + index — the boot-time read.
    pub fn index_end(&self) -> usize {
        self.index_offset + self.index_len
    }
}

/// One file entry of a v3 index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeEntry {
    pub bundle: String,
    pub version: String,
    pub path: String,
    /// Offset relative to the data section.
    pub data_offset: u64,
    pub data_len: u64,
    pub sha256: [u8; 32],
}

/// One bundle-table row of a v3 index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeBundle {
    pub bundle: String,
    pub version: String,
    /// Window start relative to the data section (4 KiB aligned).
    pub data_offset: u64,
    /// Window length (first file start → last file end, padding included).
    pub data_len: u64,
    pub stack_pages: u32,
    pub global_pointer: u64,
    /// sha256 over the window bytes — the reuse unit / kind-2 digest.
    pub sha256: [u8; 32],
}

/// Parsed superblock + index (no data bytes required).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeIndex {
    pub superblock: Superblock,
    pub entries: Vec<VolumeEntry>,
    pub bundles: Vec<VolumeBundle>,
}

impl VolumeIndex {
    /// Bundle-table lookup by `(bundle, version)`.
    pub fn bundle(&self, bundle: &str, version: &str) -> Option<&VolumeBundle> {
        self.bundles.iter().find(|b| b.bundle == bundle && b.version == version)
    }

    /// Bundle-table lookup by window digest (the reuse index key).
    pub fn bundle_by_sha(&self, sha256: &[u8; 32]) -> Option<&VolumeBundle> {
        self.bundles.iter().find(|b| &b.sha256 == sha256)
    }
}

/// Parses the 76-byte superblock of a v2 or v3 image (bounded, no data
/// access beyond the header).
pub fn parse_superblock(image: &[u8], caps: &PkgImgCaps) -> Result<Superblock, PkgImgError> {
    if image.len() < SUPERBLOCK_LEN {
        return Err(PkgImgError::Malformed("image length"));
    }
    let version = match &image[..8] {
        m if m == MAGIC_V3 => VERSION_V3,
        m if m == crate::pkgimg::MAGIC => crate::pkgimg::VERSION,
        _ => return Err(PkgImgError::BadMagicOrVersion),
    };
    let mut off = 8;
    if read_u16(image, &mut off)? != version {
        return Err(PkgImgError::BadMagicOrVersion);
    }
    let _flags = read_u16(image, &mut off)?;
    let index_offset = read_u64(image, &mut off)? as usize;
    let index_len = read_u64(image, &mut off)? as usize;
    let data_offset = read_u64(image, &mut off)? as usize;
    let data_len = read_u64(image, &mut off)? as usize;
    let mut index_sha256 = [0u8; 32];
    index_sha256.copy_from_slice(read_exact(image, &mut off, 32)?);
    if index_len > caps.max_index_bytes {
        return Err(PkgImgError::IndexCapExceeded);
    }
    let index_end =
        index_offset.checked_add(index_len).ok_or(PkgImgError::Malformed("index overflow"))?;
    let data_end =
        data_offset.checked_add(data_len).ok_or(PkgImgError::Malformed("data overflow"))?;
    if index_offset < SUPERBLOCK_LEN || data_offset < SUPERBLOCK_LEN || index_end > data_offset {
        return Err(PkgImgError::EntryOutOfBounds);
    }
    let _ = data_end;
    Ok(Superblock { version, index_offset, index_len, data_offset, data_len, index_sha256 })
}

/// Parses a v3 superblock + index from `head` (which may be the whole
/// image or just its first `index_end` bytes). Verifies the index digest;
/// does NOT touch data bytes — bundle/entry digests are verified lazily via
/// [`verify_bundle`] / [`verify_entry`].
pub fn parse_index(head: &[u8], caps: &PkgImgCaps) -> Result<VolumeIndex, PkgImgError> {
    let sb = parse_superblock(head, caps)?;
    if sb.version != VERSION_V3 {
        return Err(PkgImgError::BadMagicOrVersion);
    }
    if sb.index_end() > MAX_INDEX_BYTES_V3 {
        return Err(PkgImgError::IndexCapExceeded);
    }
    let index_bytes =
        head.get(sb.index_offset..sb.index_end()).ok_or(PkgImgError::Malformed("index slice"))?;
    if Sha256::digest(index_bytes).as_slice() != sb.index_sha256 {
        return Err(PkgImgError::IndexHashMismatch);
    }
    let mut off = 0usize;
    let entry_count = read_u32(index_bytes, &mut off)? as usize;
    if entry_count > caps.max_entry_count {
        return Err(PkgImgError::IndexCapExceeded);
    }
    let data_limit = sb.data_len as u64;
    let mut entries = Vec::with_capacity(entry_count);
    for _ in 0..entry_count {
        let bundle_len = read_u16(index_bytes, &mut off)? as usize;
        let version_len = read_u16(index_bytes, &mut off)? as usize;
        let path_len = read_u16(index_bytes, &mut off)? as usize;
        let _reserved = read_u16(index_bytes, &mut off)?;
        let data_offset = read_u64(index_bytes, &mut off)?;
        let data_len = read_u64(index_bytes, &mut off)?;
        let bundle = utf8(read_exact(index_bytes, &mut off, bundle_len)?, "bundle utf8")?;
        let version = utf8(read_exact(index_bytes, &mut off, version_len)?, "version utf8")?;
        let path = utf8(read_exact(index_bytes, &mut off, path_len)?, "path utf8")?;
        let mut sha256 = [0u8; 32];
        sha256.copy_from_slice(read_exact(index_bytes, &mut off, 32)?);
        let bundle = sanitize_component("bundle", &bundle, caps)?;
        let version = sanitize_component("version", &version, caps)?;
        let path = sanitize_path(&path, caps)?;
        if data_len as usize > caps.max_file_len {
            return Err(PkgImgError::IndexCapExceeded);
        }
        let end = data_offset.checked_add(data_len).ok_or(PkgImgError::EntryOutOfBounds)?;
        if end > data_limit {
            return Err(PkgImgError::EntryOutOfBounds);
        }
        if entries
            .iter()
            .any(|e: &VolumeEntry| e.bundle == bundle && e.version == version && e.path == path)
        {
            return Err(PkgImgError::DuplicateEntry);
        }
        entries.push(VolumeEntry { bundle, version, path, data_offset, data_len, sha256 });
    }
    let bundle_count = read_u32(index_bytes, &mut off)? as usize;
    if bundle_count > caps.max_entry_count {
        return Err(PkgImgError::IndexCapExceeded);
    }
    let mut bundles = Vec::with_capacity(bundle_count);
    for _ in 0..bundle_count {
        let bundle_len = read_u16(index_bytes, &mut off)? as usize;
        let version_len = read_u16(index_bytes, &mut off)? as usize;
        let _reserved = read_u32(index_bytes, &mut off)?;
        let data_offset = read_u64(index_bytes, &mut off)?;
        let data_len = read_u64(index_bytes, &mut off)?;
        let stack_pages = read_u32(index_bytes, &mut off)?;
        let global_pointer = read_u64(index_bytes, &mut off)?;
        let mut sha256 = [0u8; 32];
        sha256.copy_from_slice(read_exact(index_bytes, &mut off, 32)?);
        let bundle = utf8(read_exact(index_bytes, &mut off, bundle_len)?, "bundle utf8")?;
        let version = utf8(read_exact(index_bytes, &mut off, version_len)?, "version utf8")?;
        let bundle = sanitize_component("bundle", &bundle, caps)?;
        let version = sanitize_component("version", &version, caps)?;
        let end = data_offset.checked_add(data_len).ok_or(PkgImgError::EntryOutOfBounds)?;
        if end > data_limit {
            return Err(PkgImgError::EntryOutOfBounds);
        }
        if bundles.iter().any(|b: &VolumeBundle| b.bundle == bundle && b.version == version) {
            return Err(PkgImgError::DuplicateEntry);
        }
        bundles.push(VolumeBundle {
            bundle,
            version,
            data_offset,
            data_len,
            stack_pages,
            global_pointer,
            sha256,
        });
    }
    if off != index_bytes.len() {
        return Err(PkgImgError::Malformed("trailing index bytes"));
    }
    Ok(VolumeIndex { superblock: sb, entries, bundles })
}

fn utf8(raw: &[u8], ctx: &'static str) -> Result<String, PkgImgError> {
    core::str::from_utf8(raw).map(|s| s.to_string()).map_err(|_| PkgImgError::Malformed(ctx))
}

/// The bundle's data window from a full image.
pub fn bundle_window<'a>(image: &'a [u8], sb: &Superblock, b: &VolumeBundle) -> Option<&'a [u8]> {
    let begin = sb.data_offset.checked_add(b.data_offset as usize)?;
    let end = begin.checked_add(b.data_len as usize)?;
    image.get(begin..end)
}

/// A file's bytes from a full image.
pub fn entry_bytes<'a>(image: &'a [u8], sb: &Superblock, e: &VolumeEntry) -> Option<&'a [u8]> {
    let begin = sb.data_offset.checked_add(e.data_offset as usize)?;
    let end = begin.checked_add(e.data_len as usize)?;
    image.get(begin..end)
}

/// Verifies one bundle window against its index digest (lazy per-bundle
/// verification, RFC-0089 §12.3).
pub fn verify_bundle(image: &[u8], sb: &Superblock, b: &VolumeBundle) -> Result<(), PkgImgError> {
    let window = bundle_window(image, sb, b).ok_or(PkgImgError::EntryOutOfBounds)?;
    if Sha256::digest(window).as_slice() != b.sha256 {
        return Err(PkgImgError::BundleDigestMismatch);
    }
    Ok(())
}

/// Verifies one file entry against its index digest.
pub fn verify_entry(image: &[u8], sb: &Superblock, e: &VolumeEntry) -> Result<(), PkgImgError> {
    let bytes = entry_bytes(image, sb, e).ok_or(PkgImgError::EntryOutOfBounds)?;
    if Sha256::digest(bytes).as_slice() != e.sha256 {
        return Err(PkgImgError::EntryDigestMismatch);
    }
    Ok(())
}

/// Builds a deterministic v3 image. Files are sorted by `(bundle, version,
/// path)` and 4 KiB-aligned, so every bundle occupies one contiguous
/// window; `launch` rows (matched by `(bundle, version)`) fill the launch
/// parameters, absent rows mean a data-only bundle (0/0).
pub fn build_volume(
    specs: &[PkgImgFileSpec],
    launch: &[BundleLaunch],
    caps: &PkgImgCaps,
) -> Result<Vec<u8>, PkgImgError> {
    if specs.len() > caps.max_entry_count {
        return Err(PkgImgError::IndexCapExceeded);
    }
    let mut sorted = Vec::with_capacity(specs.len());
    for spec in specs {
        let bundle = sanitize_component("bundle", &spec.bundle, caps)?;
        let version = sanitize_component("version", &spec.version, caps)?;
        let path = sanitize_path(&spec.path, caps)?;
        if spec.bytes.len() > caps.max_file_len {
            return Err(PkgImgError::IndexCapExceeded);
        }
        sorted.push((bundle, version, path, spec.bytes.clone()));
    }
    sorted.sort_by(|a, b| (&a.0, &a.1, &a.2).cmp(&(&b.0, &b.1, &b.2)));
    for pair in sorted.windows(2) {
        if pair[0].0 == pair[1].0 && pair[0].1 == pair[1].1 && pair[0].2 == pair[1].2 {
            return Err(PkgImgError::DuplicateEntry);
        }
    }
    for l in launch {
        if !sorted.iter().any(|(b, v, _, _)| *b == l.bundle && *v == l.version) {
            return Err(PkgImgError::Malformed("launch row without bundle"));
        }
    }

    let mut entries = Vec::new();
    let mut data = Vec::new();
    // (bundle, version, window_start, window_end)
    let mut windows: Vec<(String, String, usize, usize)> = Vec::new();
    for (bundle, version, path, bytes) in &sorted {
        let aligned = align_up(data.len(), ALIGNMENT);
        if aligned > data.len() {
            data.resize(aligned, 0);
        }
        let data_off = data.len();
        data.extend_from_slice(bytes);
        match windows.last_mut() {
            Some(w) if w.0 == *bundle && w.1 == *version => w.3 = data.len(),
            _ => windows.push((bundle.clone(), version.clone(), data_off, data.len())),
        }
        if bundle.len() > u16::MAX as usize
            || version.len() > u16::MAX as usize
            || path.len() > u16::MAX as usize
        {
            return Err(PkgImgError::IndexCapExceeded);
        }
        write_u16(&mut entries, bundle.len() as u16);
        write_u16(&mut entries, version.len() as u16);
        write_u16(&mut entries, path.len() as u16);
        write_u16(&mut entries, 0);
        write_u64(&mut entries, data_off as u64);
        write_u64(&mut entries, bytes.len() as u64);
        entries.extend_from_slice(bundle.as_bytes());
        entries.extend_from_slice(version.as_bytes());
        entries.extend_from_slice(path.as_bytes());
        entries.extend_from_slice(&Sha256::digest(bytes));
    }

    let mut index = Vec::new();
    write_u32(&mut index, sorted.len() as u32);
    index.extend_from_slice(&entries);
    write_u32(&mut index, windows.len() as u32);
    for (bundle, version, start, end) in &windows {
        let l = launch.iter().find(|l| l.bundle == *bundle && l.version == *version);
        write_u16(&mut index, bundle.len() as u16);
        write_u16(&mut index, version.len() as u16);
        write_u32(&mut index, 0);
        write_u64(&mut index, *start as u64);
        write_u64(&mut index, (end - start) as u64);
        write_u32(&mut index, l.map(|l| l.stack_pages).unwrap_or(0));
        write_u64(&mut index, l.map(|l| l.global_pointer).unwrap_or(0));
        index.extend_from_slice(&Sha256::digest(&data[*start..*end]));
        index.extend_from_slice(bundle.as_bytes());
        index.extend_from_slice(version.as_bytes());
    }
    if index.len() > caps.max_index_bytes || SUPERBLOCK_LEN + index.len() > MAX_INDEX_BYTES_V3 {
        return Err(PkgImgError::IndexCapExceeded);
    }

    let index_hash = Sha256::digest(&index);
    let mut out = Vec::with_capacity(SUPERBLOCK_LEN + index.len() + data.len());
    out.extend_from_slice(MAGIC_V3);
    write_u16(&mut out, VERSION_V3);
    write_u16(&mut out, 0);
    write_u64(&mut out, SUPERBLOCK_LEN as u64);
    write_u64(&mut out, index.len() as u64);
    write_u64(&mut out, (SUPERBLOCK_LEN + index.len()) as u64);
    write_u64(&mut out, data.len() as u64);
    out.extend_from_slice(&index_hash);
    out.extend_from_slice(&index);
    out.extend_from_slice(&data);
    if out.len() > caps.max_image_bytes {
        return Err(PkgImgError::IndexCapExceeded);
    }
    Ok(out)
}
