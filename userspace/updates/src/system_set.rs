// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: System-set parsing and signature verification (v1.0)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable (v1.0)
//! TEST_COVERAGE: 8 tests (via tests/updates_host/ota_flow.rs)
//!   - signature verification (valid + invalid)
//!   - digest mismatch rejection
//!   - missing signature rejection
//!   - oversized archive rejection
//!   - path-traversal ../ rejection (security)
//!   - absolute path rejection (security)
//!
//! ADR: docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md

#[cfg(all(feature = "os-lite", not(feature = "std")))]
use alloc::string::ToString;
#[cfg(all(feature = "os-lite", not(feature = "std")))]
use alloc::{collections::BTreeMap, string::String, vec::Vec};
#[cfg(feature = "std")]
use std::{collections::BTreeMap, string::String, vec::Vec};

use capnp::message::ReaderOptions;
use capnp::serialize;
use sha2::{Digest, Sha256};

use crate::system_set_capnp::system_set_index;

const MAX_NXS_ARCHIVE_BYTES: usize = 100 * 1024 * 1024;
const MAX_SYSTEM_NXSINDEX_BYTES: usize = 1024 * 1024;
const MAX_MANIFEST_NXB_BYTES: usize = 256 * 1024;
const MAX_PAYLOAD_ELF_BYTES: usize = 50 * 1024 * 1024;
const MAX_BUNDLES_PER_SET: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemSetIndex {
    pub system_version: String,
    pub publisher: [u8; 32],
    pub timestamp_unix_ms: u64,
    pub bundles: Vec<BundleIndexEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleIndexEntry {
    pub name: String,
    pub version: String,
    pub manifest_sha256: [u8; 32],
    pub payload_sha256: [u8; 32],
    pub payload_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleRecord {
    pub name: String,
    pub version: String,
    pub manifest: Vec<u8>,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemSet {
    pub index: SystemSetIndex,
    pub bundles: Vec<BundleRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemSetError {
    ArchiveTooLarge { actual: usize, max: usize },
    ArchiveMalformed(&'static str),
    MissingEntry(&'static str),
    UnexpectedEntry { name: String },
    OversizedEntry { name: String, actual: usize, max: usize },
    InvalidSignature(&'static str),
    InvalidIndex(&'static str),
    DigestMismatch { name: String, field: &'static str },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    InvalidSignature,
    InvalidKey,
    Backend(&'static str),
}

pub trait SignatureVerifier {
    fn verify_ed25519(
        &self,
        public_key: &[u8; 32],
        message: &[u8],
        signature: &[u8; 64],
    ) -> Result<(), VerifyError>;
}

#[cfg(feature = "std")]
pub struct Ed25519Verifier;

#[cfg(feature = "std")]
impl SignatureVerifier for Ed25519Verifier {
    fn verify_ed25519(
        &self,
        public_key: &[u8; 32],
        message: &[u8],
        signature: &[u8; 64],
    ) -> Result<(), VerifyError> {
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};

        let key = VerifyingKey::from_bytes(public_key).map_err(|_| VerifyError::InvalidKey)?;
        let sig = Signature::from_bytes(signature);
        key.verify(message, &sig).map_err(|_| VerifyError::InvalidSignature)
    }
}

impl SystemSet {
    pub fn parse(bytes: &[u8], verifier: &dyn SignatureVerifier) -> Result<Self, SystemSetError> {
        let mut noop = || {};
        Self::parse_inner(bytes, verifier, &mut noop)
    }

    /// Parses a system-set and calls `yield_hook` periodically during heavy work (e.g. hashing).
    ///
    /// This is intended for cooperative schedulers (OS bring-up under QEMU) to avoid starvation.
    pub fn parse_with_yield(
        bytes: &[u8],
        verifier: &dyn SignatureVerifier,
        mut yield_hook: impl FnMut(),
    ) -> Result<Self, SystemSetError> {
        Self::parse_inner(bytes, verifier, &mut yield_hook)
    }

    fn parse_inner(
        bytes: &[u8],
        verifier: &dyn SignatureVerifier,
        yield_hook: &mut impl FnMut(),
    ) -> Result<Self, SystemSetError> {
        if bytes.len() > MAX_NXS_ARCHIVE_BYTES {
            return Err(SystemSetError::ArchiveTooLarge {
                actual: bytes.len(),
                max: MAX_NXS_ARCHIVE_BYTES,
            });
        }

        let entries = parse_tar_entries(bytes)?;
        if entries.len() < 2 {
            return Err(SystemSetError::ArchiveMalformed("missing system entries"));
        }

        let index_entry = &entries[0];
        let sig_entry = &entries[1];
        if index_entry.name != "system.nxsindex" {
            return Err(SystemSetError::MissingEntry("system.nxsindex"));
        }
        if sig_entry.name != "system.sig.ed25519" {
            return Err(SystemSetError::MissingEntry("system.sig.ed25519"));
        }
        if index_entry.data.len() > MAX_SYSTEM_NXSINDEX_BYTES {
            return Err(SystemSetError::OversizedEntry {
                name: index_entry.name.clone(),
                actual: index_entry.data.len(),
                max: MAX_SYSTEM_NXSINDEX_BYTES,
            });
        }
        if sig_entry.data.len() != 64 {
            return Err(SystemSetError::InvalidSignature("signature must be 64 bytes"));
        }

        let index = parse_index(&index_entry.data)?;
        let signature = array_64(&sig_entry.data)?;
        verifier
            .verify_ed25519(&index.publisher, &index_entry.data, &signature)
            .map_err(|_| SystemSetError::InvalidSignature("signature verify failed"))?;

        if index.bundles.len() > MAX_BUNDLES_PER_SET {
            return Err(SystemSetError::ArchiveMalformed("too many bundles"));
        }

        let mut file_map = BTreeMap::new();
        for entry in entries.iter().skip(2) {
            if entry.is_dir {
                continue;
            }
            file_map.insert(entry.name.clone(), entry.data.clone());
        }

        let mut bundles = Vec::with_capacity(index.bundles.len());
        for bundle in &index.bundles {
            let mut manifest_path = String::with_capacity(bundle.name.len() + 20);
            manifest_path.push_str(&bundle.name);
            manifest_path.push_str(".nxb/manifest.nxb");
            let mut payload_path = String::with_capacity(bundle.name.len() + 16);
            payload_path.push_str(&bundle.name);
            payload_path.push_str(".nxb/payload.elf");

            let manifest = file_map
                .remove(&manifest_path)
                .ok_or(SystemSetError::MissingEntry("manifest.nxb"))?;
            let payload = file_map
                .remove(&payload_path)
                .ok_or(SystemSetError::MissingEntry("payload.elf"))?;

            if manifest.len() > MAX_MANIFEST_NXB_BYTES {
                return Err(SystemSetError::OversizedEntry {
                    name: manifest_path,
                    actual: manifest.len(),
                    max: MAX_MANIFEST_NXB_BYTES,
                });
            }
            if payload.len() > MAX_PAYLOAD_ELF_BYTES {
                return Err(SystemSetError::OversizedEntry {
                    name: payload_path,
                    actual: payload.len(),
                    max: MAX_PAYLOAD_ELF_BYTES,
                });
            }
            if payload.len() as u64 != bundle.payload_size {
                return Err(SystemSetError::DigestMismatch {
                    name: bundle.name.clone(),
                    field: "payloadSize",
                });
            }
            if sha256_with_yield(&manifest, yield_hook) != bundle.manifest_sha256 {
                return Err(SystemSetError::DigestMismatch {
                    name: bundle.name.clone(),
                    field: "manifestSha256",
                });
            }
            if sha256_with_yield(&payload, yield_hook) != bundle.payload_sha256 {
                return Err(SystemSetError::DigestMismatch {
                    name: bundle.name.clone(),
                    field: "payloadSha256",
                });
            }

            bundles.push(BundleRecord {
                name: bundle.name.clone(),
                version: bundle.version.clone(),
                manifest,
                payload,
            });
        }

        if let Some(extra) = file_map.keys().next() {
            return Err(SystemSetError::UnexpectedEntry { name: extra.clone() });
        }

        Ok(SystemSet { index, bundles })
    }
}

fn parse_index(bytes: &[u8]) -> Result<SystemSetIndex, SystemSetError> {
    let mut slice = bytes;
    let message =
        serialize::read_message_from_flat_slice_no_alloc(&mut slice, ReaderOptions::new())
            .map_err(|_| SystemSetError::InvalidIndex("capnp decode failed"))?;

    let root = message
        .get_root::<system_set_index::Reader<'_>>()
        .map_err(|_| SystemSetError::InvalidIndex("capnp root missing"))?;

    let system_version = root
        .get_system_version()
        .map_err(|_| SystemSetError::InvalidIndex("systemVersion missing"))?;
    let system_version = system_version
        .to_str()
        .map_err(|_| SystemSetError::InvalidIndex("systemVersion invalid utf-8"))?
        .trim()
        .to_string();
    if system_version.is_empty() {
        return Err(SystemSetError::InvalidIndex("systemVersion empty"));
    }

    let publisher =
        root.get_publisher().map_err(|_| SystemSetError::InvalidIndex("publisher missing"))?;
    if publisher.len() != 32 {
        return Err(SystemSetError::InvalidIndex("publisher must be 32 bytes"));
    }
    let mut publisher_bytes = [0u8; 32];
    publisher_bytes.copy_from_slice(publisher);

    let timestamp_unix_ms = root.get_timestamp_unix_ms();

    let bundles_reader =
        root.get_bundles().map_err(|_| SystemSetError::InvalidIndex("bundles missing"))?;
    let mut bundles = Vec::with_capacity(bundles_reader.len() as usize);
    for i in 0..bundles_reader.len() {
        let entry = bundles_reader.get(i);
        let name =
            entry.get_name().map_err(|_| SystemSetError::InvalidIndex("bundle name missing"))?;
        let name = name
            .to_str()
            .map_err(|_| SystemSetError::InvalidIndex("bundle name invalid utf-8"))?
            .trim()
            .to_string();
        if name.is_empty() {
            return Err(SystemSetError::InvalidIndex("bundle name empty"));
        }
        let version = entry
            .get_version()
            .map_err(|_| SystemSetError::InvalidIndex("bundle version missing"))?;
        let version = version
            .to_str()
            .map_err(|_| SystemSetError::InvalidIndex("bundle version invalid utf-8"))?
            .trim()
            .to_string();
        if version.is_empty() {
            return Err(SystemSetError::InvalidIndex("bundle version empty"));
        }

        let manifest_sha256 = entry
            .get_manifest_sha256()
            .map_err(|_| SystemSetError::InvalidIndex("manifestSha256 missing"))?;
        let payload_sha256 = entry
            .get_payload_sha256()
            .map_err(|_| SystemSetError::InvalidIndex("payloadSha256 missing"))?;
        if manifest_sha256.len() != 32 || payload_sha256.len() != 32 {
            return Err(SystemSetError::InvalidIndex("digest must be 32 bytes"));
        }
        let mut manifest_bytes = [0u8; 32];
        let mut payload_bytes = [0u8; 32];
        manifest_bytes.copy_from_slice(manifest_sha256);
        payload_bytes.copy_from_slice(payload_sha256);

        bundles.push(BundleIndexEntry {
            name,
            version,
            manifest_sha256: manifest_bytes,
            payload_sha256: payload_bytes,
            payload_size: entry.get_payload_size(),
        });
    }

    Ok(SystemSetIndex { system_version, publisher: publisher_bytes, timestamp_unix_ms, bundles })
}

struct TarEntry {
    name: String,
    data: Vec<u8>,
    is_dir: bool,
}

fn parse_tar_entries(bytes: &[u8]) -> Result<Vec<TarEntry>, SystemSetError> {
    let mut entries = Vec::new();
    let mut offset = 0usize;
    while offset + 512 <= bytes.len() {
        let header = &bytes[offset..offset + 512];
        if header.iter().all(|b| *b == 0) {
            break;
        }
        let name = parse_name(header)?;
        if !is_safe_path(&name) {
            return Err(SystemSetError::ArchiveMalformed("unsafe path"));
        }
        let size = parse_octal(&header[124..136])?;
        let typeflag = header[156];
        let data_start = offset + 512;
        let data_end = data_start
            .checked_add(size)
            .ok_or(SystemSetError::ArchiveMalformed("tar size overflow"))?;
        if data_end > bytes.len() {
            return Err(SystemSetError::ArchiveMalformed("truncated tar entry"));
        }
        let data = bytes[data_start..data_end].to_vec();
        let is_dir = typeflag == b'5';
        entries.push(TarEntry { name, data, is_dir });

        let padded = size.div_ceil(512) * 512;
        offset = data_start + padded;
    }
    Ok(entries)
}

fn parse_name(header: &[u8]) -> Result<String, SystemSetError> {
    let name_bytes = &header[0..100];
    let name_len = name_bytes.iter().position(|b| *b == 0).unwrap_or(100);
    let name = core::str::from_utf8(&name_bytes[..name_len])
        .map_err(|_| SystemSetError::ArchiveMalformed("tar name invalid utf-8"))?
        .trim()
        .to_string();
    if name.is_empty() {
        return Err(SystemSetError::ArchiveMalformed("empty tar name"));
    }
    Ok(name)
}

fn parse_octal(bytes: &[u8]) -> Result<usize, SystemSetError> {
    let mut out: usize = 0;
    let mut saw_digit = false;
    for b in bytes.iter() {
        if *b == 0 || *b == b' ' {
            continue;
        }
        if *b < b'0' || *b > b'7' {
            return Err(SystemSetError::ArchiveMalformed("invalid tar size"));
        }
        saw_digit = true;
        out = out
            .checked_mul(8)
            .and_then(|v| v.checked_add((b - b'0') as usize))
            .ok_or(SystemSetError::ArchiveMalformed("tar size overflow"))?;
    }
    if !saw_digit {
        return Err(SystemSetError::ArchiveMalformed("missing tar size"));
    }
    Ok(out)
}

fn is_safe_path(path: &str) -> bool {
    if path.starts_with('/') || path.contains('\0') {
        return false;
    }
    for part in path.split('/') {
        if part == ".." {
            return false;
        }
    }
    true
}

fn sha256_with_yield(bytes: &[u8], yield_hook: &mut impl FnMut()) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for (i, chunk) in bytes.chunks(4096).enumerate() {
        hasher.update(chunk);
        if (i & 0x3f) == 0 {
            yield_hook();
        }
    }
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

fn array_64(bytes: &[u8]) -> Result<[u8; 64], SystemSetError> {
    if bytes.len() != 64 {
        return Err(SystemSetError::InvalidSignature("signature must be 64 bytes"));
    }
    let mut out = [0u8; 64];
    out.copy_from_slice(bytes);
    Ok(out)
}
