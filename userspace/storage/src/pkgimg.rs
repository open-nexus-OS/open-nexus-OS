// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Deterministic PackageFS image v2 (`pkgimg`) builder/parser.
//! OWNERS: @runtime @storage
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Unit tests in this module (including `test_reject_*`).

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use sha2::{Digest, Sha256};

const MAGIC: &[u8; 8] = b"PKGIMGV2";
const VERSION: u16 = 2;
const SUPERBLOCK_LEN: usize = 8 + 2 + 2 + 8 + 8 + 8 + 8 + 32;
const ALIGNMENT: usize = 4096;

/// Hard caps used by parser/builder for deterministic rejection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PkgImgCaps {
    /// Maximum accepted image size.
    pub max_image_bytes: usize,
    /// Maximum accepted index byte length.
    pub max_index_bytes: usize,
    /// Maximum accepted index entry count.
    pub max_entry_count: usize,
    /// Maximum accepted UTF-8 path length.
    pub max_path_len: usize,
    /// Maximum accepted bundle/version length.
    pub max_name_len: usize,
    /// Maximum accepted file payload length.
    pub max_file_len: usize,
}

impl Default for PkgImgCaps {
    fn default() -> Self {
        Self {
            max_image_bytes: 128 * 1024 * 1024,
            max_index_bytes: 8 * 1024 * 1024,
            max_entry_count: 65_536,
            max_path_len: 1024,
            max_name_len: 256,
            max_file_len: 32 * 1024 * 1024,
        }
    }
}

/// Input spec used by the deterministic image builder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PkgImgFileSpec {
    /// Bundle name.
    pub bundle: String,
    /// Bundle version.
    pub version: String,
    /// File path within the bundle.
    pub path: String,
    /// File payload bytes.
    pub bytes: Vec<u8>,
}

impl PkgImgFileSpec {
    /// Creates a new file spec.
    pub fn new(bundle: &str, version: &str, path: &str, bytes: &[u8]) -> Self {
        Self {
            bundle: bundle.to_string(),
            version: version.to_string(),
            path: path.to_string(),
            bytes: bytes.to_vec(),
        }
    }
}

/// Parsed immutable file entry metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PkgImgEntry {
    /// Bundle name.
    pub bundle: String,
    /// Bundle version.
    pub version: String,
    /// File path.
    pub path: String,
    /// Data offset relative to the data section.
    pub data_offset: u64,
    /// Data length.
    pub data_len: u64,
}

/// Parsed package image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedPkgImg {
    data_section_offset: usize,
    image_bytes: Vec<u8>,
    entries: Vec<PkgImgEntry>,
    index: BTreeMap<(String, String, String), usize>,
}

impl ParsedPkgImg {
    /// Returns entries in deterministic sorted order.
    pub fn entries(&self) -> &[PkgImgEntry] {
        &self.entries
    }

    /// Looks up a file by `(bundle, version, path)` and returns its bytes.
    pub fn read(&self, bundle: &str, version: &str, path: &str) -> Option<&[u8]> {
        let key = (bundle.to_string(), version.to_string(), path.to_string());
        let idx = *self.index.get(&key)?;
        let entry = self.entries.get(idx)?;
        let begin = self.data_section_offset.checked_add(entry.data_offset as usize)?;
        let end = begin.checked_add(entry.data_len as usize)?;
        self.image_bytes.get(begin..end)
    }

    /// Reads a bounded range from a file entry (`offset`, `len`), clamped to file size.
    pub fn read_at(
        &self,
        bundle: &str,
        version: &str,
        path: &str,
        offset: usize,
        len: usize,
    ) -> Option<&[u8]> {
        let full = self.read(bundle, version, path)?;
        if offset > full.len() {
            return None;
        }
        let end = core::cmp::min(full.len(), offset.checked_add(len)?);
        full.get(offset..end)
    }
}

/// Errors emitted by `pkgimg` parser/builder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PkgImgError {
    /// Input image is malformed.
    Malformed(&'static str),
    /// Image magic or version is unsupported.
    BadMagicOrVersion,
    /// Index hash does not match the encoded value.
    IndexHashMismatch,
    /// Entry metadata points outside data section.
    EntryOutOfBounds,
    /// Path is invalid (traversal, empty segment, invalid UTF-8 rules).
    PathTraversalOrEmptySegment,
    /// Parser/builder cap exceeded.
    IndexCapExceeded,
    /// Duplicate `(bundle,version,path)` key.
    DuplicateEntry,
}

impl fmt::Display for PkgImgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(ctx) => write!(f, "malformed pkgimg: {ctx}"),
            Self::BadMagicOrVersion => write!(f, "bad magic or version"),
            Self::IndexHashMismatch => write!(f, "index hash mismatch"),
            Self::EntryOutOfBounds => write!(f, "entry out of bounds"),
            Self::PathTraversalOrEmptySegment => write!(f, "path traversal or empty segment"),
            Self::IndexCapExceeded => write!(f, "index cap exceeded"),
            Self::DuplicateEntry => write!(f, "duplicate entry"),
        }
    }
}

fn align_up(value: usize, alignment: usize) -> usize {
    if alignment == 0 {
        return value;
    }
    let rem = value % alignment;
    if rem == 0 { value } else { value + (alignment - rem) }
}

fn sanitize_component(label: &str, value: &str, caps: &PkgImgCaps) -> Result<String, PkgImgError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > caps.max_name_len {
        return Err(PkgImgError::IndexCapExceeded);
    }
    if trimmed.contains('/') || trimmed.contains('\0') {
        let _ = label;
        return Err(PkgImgError::Malformed("component"));
    }
    Ok(trimmed.to_string())
}

fn sanitize_path(path: &str, caps: &PkgImgCaps) -> Result<String, PkgImgError> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() || trimmed.len() > caps.max_path_len {
        return Err(PkgImgError::IndexCapExceeded);
    }
    let mut clean = Vec::new();
    for segment in trimmed.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(PkgImgError::PathTraversalOrEmptySegment);
        }
        clean.push(segment);
    }
    Ok(clean.join("/"))
}

fn write_u16(dst: &mut Vec<u8>, value: u16) {
    dst.extend_from_slice(&value.to_le_bytes());
}

fn write_u32(dst: &mut Vec<u8>, value: u32) {
    dst.extend_from_slice(&value.to_le_bytes());
}

fn write_u64(dst: &mut Vec<u8>, value: u64) {
    dst.extend_from_slice(&value.to_le_bytes());
}

fn read_exact<'a>(bytes: &'a [u8], off: &mut usize, len: usize) -> Result<&'a [u8], PkgImgError> {
    let end = off.checked_add(len).ok_or(PkgImgError::Malformed("offset overflow"))?;
    let out = bytes.get(*off..end).ok_or(PkgImgError::Malformed("truncated"))?;
    *off = end;
    Ok(out)
}

fn read_u16(bytes: &[u8], off: &mut usize) -> Result<u16, PkgImgError> {
    let data = read_exact(bytes, off, 2)?;
    Ok(u16::from_le_bytes([data[0], data[1]]))
}

fn read_u32(bytes: &[u8], off: &mut usize) -> Result<u32, PkgImgError> {
    let data = read_exact(bytes, off, 4)?;
    Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
}

fn read_u64(bytes: &[u8], off: &mut usize) -> Result<u64, PkgImgError> {
    let data = read_exact(bytes, off, 8)?;
    Ok(u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]))
}

/// Builds a deterministic `pkgimg` v2 image from file specs.
pub fn build_pkgimg(specs: &[PkgImgFileSpec], caps: PkgImgCaps) -> Result<Vec<u8>, PkgImgError> {
    if specs.len() > caps.max_entry_count {
        return Err(PkgImgError::IndexCapExceeded);
    }
    let mut sorted = Vec::with_capacity(specs.len());
    for spec in specs {
        let bundle = sanitize_component("bundle", &spec.bundle, &caps)?;
        let version = sanitize_component("version", &spec.version, &caps)?;
        let path = sanitize_path(&spec.path, &caps)?;
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

    let mut index = Vec::new();
    write_u32(&mut index, sorted.len() as u32);
    let mut data = Vec::new();
    for (bundle, version, path, bytes) in &sorted {
        let aligned = align_up(data.len(), ALIGNMENT);
        if aligned > data.len() {
            data.resize(aligned, 0);
        }
        let data_off = data.len() as u64;
        data.extend_from_slice(bytes);

        if bundle.len() > u16::MAX as usize || version.len() > u16::MAX as usize || path.len() > u16::MAX as usize {
            return Err(PkgImgError::IndexCapExceeded);
        }
        write_u16(&mut index, bundle.len() as u16);
        write_u16(&mut index, version.len() as u16);
        write_u16(&mut index, path.len() as u16);
        write_u16(&mut index, 0);
        write_u64(&mut index, data_off);
        write_u64(&mut index, bytes.len() as u64);
        index.extend_from_slice(bundle.as_bytes());
        index.extend_from_slice(version.as_bytes());
        index.extend_from_slice(path.as_bytes());
    }
    if index.len() > caps.max_index_bytes {
        return Err(PkgImgError::IndexCapExceeded);
    }

    let index_hash = Sha256::digest(&index);
    let index_offset = SUPERBLOCK_LEN as u64;
    let index_len = index.len() as u64;
    let data_offset = (SUPERBLOCK_LEN + index.len()) as u64;
    let data_len = data.len() as u64;

    let mut out = Vec::with_capacity(SUPERBLOCK_LEN + index.len() + data.len());
    out.extend_from_slice(MAGIC);
    write_u16(&mut out, VERSION);
    write_u16(&mut out, 0);
    write_u64(&mut out, index_offset);
    write_u64(&mut out, index_len);
    write_u64(&mut out, data_offset);
    write_u64(&mut out, data_len);
    out.extend_from_slice(&index_hash);
    out.extend_from_slice(&index);
    out.extend_from_slice(&data);
    if out.len() > caps.max_image_bytes {
        return Err(PkgImgError::IndexCapExceeded);
    }
    Ok(out)
}

/// Parses and validates a `pkgimg` v2 image.
pub fn parse_pkgimg(image: &[u8], caps: PkgImgCaps) -> Result<ParsedPkgImg, PkgImgError> {
    if image.len() < SUPERBLOCK_LEN || image.len() > caps.max_image_bytes {
        return Err(PkgImgError::Malformed("image length"));
    }
    if &image[..8] != MAGIC {
        return Err(PkgImgError::BadMagicOrVersion);
    }
    let mut off = 8;
    let version = read_u16(image, &mut off)?;
    if version != VERSION {
        return Err(PkgImgError::BadMagicOrVersion);
    }
    let _flags = read_u16(image, &mut off)?;
    let index_offset = read_u64(image, &mut off)? as usize;
    let index_len = read_u64(image, &mut off)? as usize;
    let data_offset = read_u64(image, &mut off)? as usize;
    let data_len = read_u64(image, &mut off)? as usize;
    let expected_hash = read_exact(image, &mut off, 32)?;
    if off != SUPERBLOCK_LEN {
        return Err(PkgImgError::Malformed("superblock layout"));
    }
    if index_len > caps.max_index_bytes {
        return Err(PkgImgError::IndexCapExceeded);
    }
    let index_end = index_offset
        .checked_add(index_len)
        .ok_or(PkgImgError::Malformed("index overflow"))?;
    let data_end = data_offset
        .checked_add(data_len)
        .ok_or(PkgImgError::Malformed("data overflow"))?;
    if index_offset < SUPERBLOCK_LEN || data_offset < SUPERBLOCK_LEN || index_end > image.len() || data_end > image.len() || index_end > data_offset {
        return Err(PkgImgError::EntryOutOfBounds);
    }
    let index_bytes = image.get(index_offset..index_end).ok_or(PkgImgError::Malformed("index slice"))?;
    let digest = Sha256::digest(index_bytes);
    if digest.as_slice() != expected_hash {
        return Err(PkgImgError::IndexHashMismatch);
    }

    let mut idx_off = 0usize;
    let entry_count = read_u32(index_bytes, &mut idx_off)? as usize;
    if entry_count > caps.max_entry_count {
        return Err(PkgImgError::IndexCapExceeded);
    }

    let mut entries = Vec::with_capacity(entry_count);
    let mut index = BTreeMap::new();
    for _ in 0..entry_count {
        let bundle_len = read_u16(index_bytes, &mut idx_off)? as usize;
        let version_len = read_u16(index_bytes, &mut idx_off)? as usize;
        let path_len = read_u16(index_bytes, &mut idx_off)? as usize;
        let _reserved = read_u16(index_bytes, &mut idx_off)?;
        let data_off = read_u64(index_bytes, &mut idx_off)?;
        let len = read_u64(index_bytes, &mut idx_off)?;

        let bundle_raw = read_exact(index_bytes, &mut idx_off, bundle_len)?;
        let version_raw = read_exact(index_bytes, &mut idx_off, version_len)?;
        let path_raw = read_exact(index_bytes, &mut idx_off, path_len)?;
        let bundle_str = core::str::from_utf8(bundle_raw).map_err(|_| PkgImgError::Malformed("bundle utf8"))?;
        let version_str = core::str::from_utf8(version_raw).map_err(|_| PkgImgError::Malformed("version utf8"))?;
        let path_str = core::str::from_utf8(path_raw).map_err(|_| PkgImgError::Malformed("path utf8"))?;
        let bundle = sanitize_component("bundle", bundle_str, &caps)?;
        let version = sanitize_component("version", version_str, &caps)?;
        let path = sanitize_path(path_str, &caps)?;

        if len as usize > caps.max_file_len {
            return Err(PkgImgError::IndexCapExceeded);
        }
        let data_limit = data_len as u64;
        let end = data_off.checked_add(len).ok_or(PkgImgError::EntryOutOfBounds)?;
        if end > data_limit {
            return Err(PkgImgError::EntryOutOfBounds);
        }
        let key = (bundle.clone(), version.clone(), path.clone());
        if index.contains_key(&key) {
            return Err(PkgImgError::DuplicateEntry);
        }
        let next_idx = entries.len();
        entries.push(PkgImgEntry { bundle: bundle.clone(), version: version.clone(), path: path.clone(), data_offset: data_off, data_len: len });
        index.insert(key, next_idx);
    }
    if idx_off != index_bytes.len() {
        return Err(PkgImgError::Malformed("trailing index bytes"));
    }

    Ok(ParsedPkgImg {
        data_section_offset: data_offset,
        image_bytes: image.to_vec(),
        entries,
        index,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn sample_specs() -> Vec<PkgImgFileSpec> {
        vec![
            PkgImgFileSpec::new("demo.hello", "1.0.0", "manifest.nxb", b"manifest"),
            PkgImgFileSpec::new("demo.hello", "1.0.0", "payload.elf", b"payload"),
            PkgImgFileSpec::new("system", "1.0.0", "build.prop", b"ro.nexus.build=dev\n"),
        ]
    }

    #[test]
    fn build_parse_roundtrip_and_read() {
        let img = build_pkgimg(&sample_specs(), PkgImgCaps::default()).expect("build");
        let parsed = parse_pkgimg(&img, PkgImgCaps::default()).expect("parse");
        let got = parsed.read("system", "1.0.0", "build.prop").expect("read");
        assert_eq!(got, b"ro.nexus.build=dev\n");
        assert!(parsed.entries().len() == 3);
    }

    #[test]
    fn deterministic_build_outputs_identical_bytes() {
        let a = build_pkgimg(&sample_specs(), PkgImgCaps::default()).expect("build a");
        let b = build_pkgimg(&sample_specs(), PkgImgCaps::default()).expect("build b");
        assert_eq!(a, b);
    }

    #[test]
    fn read_contract_random_seek_slices() {
        let img = build_pkgimg(&sample_specs(), PkgImgCaps::default()).expect("build");
        let parsed = parse_pkgimg(&img, PkgImgCaps::default()).expect("parse");
        let file = b"ro.nexus.build=dev\n";
        let probes = [(0usize, 2usize), (3, 5), (8, 64), (file.len(), 1)];
        for (off, len) in probes {
            let got = parsed
                .read_at("system", "1.0.0", "build.prop", off, len)
                .expect("slice");
            let expected_end = core::cmp::min(file.len(), off + len);
            assert_eq!(got, &file[off..expected_end]);
        }
        assert!(
            parsed
                .read_at("system", "1.0.0", "build.prop", file.len() + 1, 1)
                .is_none()
        );
    }

    #[test]
    fn test_reject_pkgimg_bad_magic_or_version() {
        let mut img = build_pkgimg(&sample_specs(), PkgImgCaps::default()).expect("build");
        img[0] = b'X';
        let err = parse_pkgimg(&img, PkgImgCaps::default()).expect_err("must reject");
        assert_eq!(err, PkgImgError::BadMagicOrVersion);
    }

    #[test]
    fn test_reject_pkgimg_bad_version() {
        let mut img = build_pkgimg(&sample_specs(), PkgImgCaps::default()).expect("build");
        img[8..10].copy_from_slice(&3u16.to_le_bytes());
        let err = parse_pkgimg(&img, PkgImgCaps::default()).expect_err("must reject");
        assert_eq!(err, PkgImgError::BadMagicOrVersion);
    }

    #[test]
    fn test_reject_pkgimg_index_hash_mismatch() {
        let mut img = build_pkgimg(&sample_specs(), PkgImgCaps::default()).expect("build");
        let index_byte = SUPERBLOCK_LEN + 4;
        img[index_byte] ^= 0x01;
        let err = parse_pkgimg(&img, PkgImgCaps::default()).expect_err("must reject");
        assert_eq!(err, PkgImgError::IndexHashMismatch);
    }

    #[test]
    fn test_reject_pkgimg_entry_out_of_bounds() {
        let mut img = build_pkgimg(&sample_specs(), PkgImgCaps::default()).expect("build");
        let data_len_off = 8 + 2 + 2 + 8 + 8 + 8;
        img[data_len_off..data_len_off + 8].copy_from_slice(&1u64.to_le_bytes());
        let err = parse_pkgimg(&img, PkgImgCaps::default()).expect_err("must reject");
        assert_eq!(err, PkgImgError::EntryOutOfBounds);
    }

    #[test]
    fn test_reject_pkgimg_path_traversal_or_empty_segment() {
        let bad = vec![PkgImgFileSpec::new("demo.hello", "1.0.0", "../escape", b"x")];
        let err = build_pkgimg(&bad, PkgImgCaps::default()).expect_err("must reject");
        assert_eq!(err, PkgImgError::PathTraversalOrEmptySegment);
    }

    #[test]
    fn test_reject_pkgimg_path_traversal_or_empty_segment_from_image() {
        let mut img = build_pkgimg(&sample_specs(), PkgImgCaps::default()).expect("build");
        let mut off = SUPERBLOCK_LEN;
        let entry_count =
            u32::from_le_bytes([img[off], img[off + 1], img[off + 2], img[off + 3]]) as usize;
        assert!(entry_count > 0);
        off += 4;
        let bundle_len = u16::from_le_bytes([img[off], img[off + 1]]) as usize;
        let version_len = u16::from_le_bytes([img[off + 2], img[off + 3]]) as usize;
        let path_len = u16::from_le_bytes([img[off + 4], img[off + 5]]) as usize;
        let path_start = off + 2 + 2 + 2 + 2 + 8 + 8 + bundle_len + version_len;
        assert!(path_len >= 4);
        img[path_start] = b'.';
        img[path_start + 1] = b'.';
        img[path_start + 2] = b'/';
        img[path_start + 3] = b'/';

        let index_len_off = 8 + 2 + 2 + 8;
        let index_len = u64::from_le_bytes([
            img[index_len_off],
            img[index_len_off + 1],
            img[index_len_off + 2],
            img[index_len_off + 3],
            img[index_len_off + 4],
            img[index_len_off + 5],
            img[index_len_off + 6],
            img[index_len_off + 7],
        ]) as usize;
        let index_bytes = &img[SUPERBLOCK_LEN..SUPERBLOCK_LEN + index_len];
        let new_hash = Sha256::digest(index_bytes);
        let hash_off = 8 + 2 + 2 + 8 + 8 + 8 + 8;
        img[hash_off..hash_off + 32].copy_from_slice(&new_hash);

        let err = parse_pkgimg(&img, PkgImgCaps::default()).expect_err("must reject");
        assert_eq!(err, PkgImgError::PathTraversalOrEmptySegment);
    }

    #[test]
    fn test_reject_pkgimg_index_cap_exceeded() {
        let caps = PkgImgCaps { max_entry_count: 1, ..PkgImgCaps::default() };
        let err = build_pkgimg(&sample_specs(), caps).expect_err("must reject");
        assert_eq!(err, PkgImgError::IndexCapExceeded);
    }
}
