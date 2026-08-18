// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: StateFS record encryption v2b (TASK-0027) — opt-in AEAD
//!   (XChaCha20-Poly1305) of value payloads for enrolled non-boot-critical
//!   prefixes. Values are sealed at write time and stay sealed in memory
//!   and through compaction (snapshots copy ciphertext); `get` opens on
//!   demand. Keys/paths stay plaintext. Contract:
//!   docs/storage/statefs.md §Record encryption v2b + RFC-0071 key
//!   hierarchy (one derivation discipline platform-wide).
//! OWNERS: @runtime
//! STATUS: Functional (host-first)
//! API_STABILITY: Unstable (v2b)
//! TEST_COVERAGE: Unit tests below (roundtrip, tamper, splice, enrollment
//!   guard, meta codec) + engine-level tests in tests/crash_injection.rs
//!
//! SEALED VALUE LAYOUT (`NXR1` v1, overhead = 36 bytes):
//!   magic(4) | ver(1) | class(1) | reserved(2=0) | txn_id u64 LE |
//!   chunk_idx u32 LE | ciphertext | tag(16)
//!
//! NONCE (24 bytes, deterministic, never sampled — RFC-0009: no getrandom):
//!   salt(12) || txn_id LE(8) || chunk_idx LE(4). Uniqueness holds because
//!   txn ids are monotonic and never reused: plain puts consume an id from
//!   the same counter, and replay re-seeds the counter above every id seen
//!   in PREPARE records AND sealed value headers.
//!
//! AAD binds the sealed header AND the key path — ciphertext spliced onto
//! another key (or another txn/chunk) fails to open.
//!
//! KEY DERIVATION mirrors `statefs::derive` (TASK-0025): deterministic
//! Ed25519 device-key signature over `statefs.record.v1.<class>` as ikm,
//! then HKDF-SHA256 with the same label as salt + info. Only statefsd ever
//! derives record keys (locally, from the persisted seed) — unlike envelope
//! MAC keys there is no writer-side oracle, so `device_sign_allowed` stays
//! untouched.
//!
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{Tag, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use sha2::Sha256;

use crate::StatefsError;

/// Derivation-label prefix; full label = `statefs.record.v1.<class>`.
pub const RECORD_KEY_LABEL_PREFIX: &[u8] = b"statefs.record.v1.";
/// Sealed-value magic (distinct from `NXEV` envelopes and `NXS2`/`NXSF`).
pub const SEALED_MAGIC: [u8; 4] = *b"NXR1";
const SEALED_VERSION: u8 = 1;
/// Header (20) + Poly1305 tag (16).
pub const SEALED_OVERHEAD: usize = SEALED_HEADER_LEN + TAG_LEN;
const SEALED_HEADER_LEN: usize = 20;
const TAG_LEN: usize = 16;
/// Per-store nonce diversifier length (public, entropy-sourced at enable).
pub const SALT_LEN: usize = 12;
/// Enablement meta record (plaintext by construction — the enc chicken-egg).
pub const META_KEY: &str = "/state/statefsd/enc.v1";
const META_MAGIC: [u8; 4] = *b"NXEM";
const META_VERSION: u8 = 1;
/// magic(4) + ver(1) + salt(12) + crc32c(4).
pub const META_LEN: usize = 4 + 1 + SALT_LEN + 4;
/// Hard cap on classes (class byte indexes into the context table).
pub const MAX_CLASSES: usize = 4;
/// Hard cap on enrolled prefixes (const service tables stay tiny).
pub const MAX_ENROLLED: usize = 8;
/// Class names are ASCII, short, and bounded (label buffer sizing).
pub const MAX_CLASS_LEN: usize = 24;
/// `RECORD_KEY_LABEL_PREFIX` + class, bounded.
pub const MAX_LABEL_LEN: usize = 42;

/// Prefixes that can NEVER be enrolled: records the boot chain needs before
/// any key material exists (TASK-0027 chicken-egg rule), plus the meta key's
/// own prefix (the switch must stay readable without keys).
const FORBIDDEN_ENROLL: [&str; 3] = ["/state/keystore/", "/state/boot/", "/state/statefsd/"];

/// A derived 32-byte AEAD key for one prefix class.
pub struct RecordKey(pub(crate) [u8; 32]);

impl RecordKey {
    /// Wrap raw key bytes (offline tooling: fsck-statefs takes the key as
    /// an explicit CLI input — it never derives).
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

/// Build the full derivation label for `class` into `out`; returns its
/// length. Rejects empty/oversized/non-ASCII class names.
pub fn record_label(class: &str, out: &mut [u8; MAX_LABEL_LEN]) -> Result<usize, StatefsError> {
    if class.is_empty()
        || class.len() > MAX_CLASS_LEN
        || !class.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(StatefsError::InvalidKey);
    }
    let len = RECORD_KEY_LABEL_PREFIX.len() + class.len();
    out[..RECORD_KEY_LABEL_PREFIX.len()].copy_from_slice(RECORD_KEY_LABEL_PREFIX);
    out[RECORD_KEY_LABEL_PREFIX.len()..len].copy_from_slice(class.as_bytes());
    Ok(len)
}

/// HKDF-SHA256(salt = info = label, ikm) → 32-byte record key. Fails closed
/// (`IntegrityViolation`) instead of yielding a degenerate key — the same
/// discipline as `derive::envelope_key_from_ikm`.
pub fn record_key_from_ikm(ikm: &[u8], class: &str) -> Result<RecordKey, StatefsError> {
    let mut label = [0u8; MAX_LABEL_LEN];
    let len = record_label(class, &mut label)?;
    let hk = Hkdf::<Sha256>::new(Some(&label[..len]), ikm);
    let mut okm = [0u8; 32];
    hk.expand(&label[..len], &mut okm).map_err(|_| StatefsError::IntegrityViolation)?;
    Ok(RecordKey(okm))
}

/// One enrolled prefix → class-index row.
struct Enrolled {
    prefix: String,
    class: u8,
}

/// The live encryption context: per-class keys, the store salt, and the
/// enrolled-prefix table. Owned by `JournalEngine` once enabled.
pub struct EncContext {
    salt: [u8; SALT_LEN],
    ciphers: Vec<XChaCha20Poly1305>,
    class_names: Vec<&'static str>,
    enrolled: Vec<Enrolled>,
}

impl EncContext {
    /// New context around the persisted per-store salt.
    #[must_use]
    pub fn new(salt: [u8; SALT_LEN]) -> Self {
        Self { salt, ciphers: Vec::new(), class_names: Vec::new(), enrolled: Vec::new() }
    }

    /// Register a class key; returns the class index used in sealed headers.
    pub fn add_class(&mut self, name: &'static str, key: &RecordKey) -> Result<u8, StatefsError> {
        if self.ciphers.len() >= MAX_CLASSES {
            return Err(StatefsError::ValueTooLarge);
        }
        // Validate the name through the label builder (bounded, ascii).
        let mut label = [0u8; MAX_LABEL_LEN];
        record_label(name, &mut label)?;
        self.ciphers.push(XChaCha20Poly1305::new((&key.0).into()));
        self.class_names.push(name);
        Ok((self.ciphers.len() - 1) as u8)
    }

    /// Enroll `prefix` under class index `class`. Boot-critical prefixes and
    /// the meta key's own prefix are rejected deterministically (chicken-egg
    /// rule — see `test_reject_boot_critical_prefix_enrollment`).
    pub fn enroll(&mut self, prefix: &str, class: u8) -> Result<(), StatefsError> {
        if !prefix.starts_with("/state/") || !prefix.ends_with('/') {
            return Err(StatefsError::InvalidKey);
        }
        for forbidden in FORBIDDEN_ENROLL {
            if prefix.starts_with(forbidden) || forbidden.starts_with(prefix) {
                return Err(StatefsError::AccessDenied);
            }
        }
        if usize::from(class) >= self.ciphers.len() || self.enrolled.len() >= MAX_ENROLLED {
            return Err(StatefsError::ValueTooLarge);
        }
        self.enrolled.push(Enrolled { prefix: String::from(prefix), class });
        Ok(())
    }

    /// Class index for `key`, if it lives under an enrolled prefix.
    #[must_use]
    pub fn class_for(&self, key: &str) -> Option<u8> {
        self.enrolled.iter().find(|e| key.starts_with(e.prefix.as_str())).map(|e| e.class)
    }

    fn cipher(&self, class: u8) -> Result<&XChaCha20Poly1305, StatefsError> {
        self.ciphers.get(usize::from(class)).ok_or(StatefsError::IntegrityViolation)
    }

    fn nonce(&self, txn_id: u64, chunk_idx: u32) -> XNonce {
        let mut nonce = [0u8; 24];
        nonce[..SALT_LEN].copy_from_slice(&self.salt);
        nonce[SALT_LEN..SALT_LEN + 8].copy_from_slice(&txn_id.to_le_bytes());
        nonce[SALT_LEN + 8..].copy_from_slice(&chunk_idx.to_le_bytes());
        XNonce::from(nonce)
    }
}

/// True if `value` carries the sealed-record magic.
#[must_use]
pub fn is_sealed(value: &[u8]) -> bool {
    value.len() >= SEALED_OVERHEAD && value[..4] == SEALED_MAGIC && value[4] == SEALED_VERSION
}

/// The nonce-counter id a sealed value consumed (for replay re-seeding).
#[must_use]
pub fn sealed_txn_id(value: &[u8]) -> Option<u64> {
    if !is_sealed(value) {
        return None;
    }
    let mut id = [0u8; 8];
    id.copy_from_slice(&value[8..16]);
    Some(u64::from_le_bytes(id))
}

/// AAD = header bytes || key path (bounded: MAX_KEY_LEN).
fn build_aad(header: &[u8; SEALED_HEADER_LEN], key_path: &str, out: &mut [u8]) -> usize {
    let len = SEALED_HEADER_LEN + key_path.len();
    out[..SEALED_HEADER_LEN].copy_from_slice(header);
    out[SEALED_HEADER_LEN..len].copy_from_slice(key_path.as_bytes());
    len
}

const AAD_BUF: usize = SEALED_HEADER_LEN + crate::MAX_KEY_LEN;

fn header(class: u8, txn_id: u64, chunk_idx: u32) -> [u8; SEALED_HEADER_LEN] {
    let mut h = [0u8; SEALED_HEADER_LEN];
    h[..4].copy_from_slice(&SEALED_MAGIC);
    h[4] = SEALED_VERSION;
    h[5] = class;
    // h[6..8] reserved = 0
    h[8..16].copy_from_slice(&txn_id.to_le_bytes());
    h[16..20].copy_from_slice(&chunk_idx.to_le_bytes());
    h
}

/// Seal `plaintext` for `key_path` into `out` (cleared first; capacity is
/// reused — bump-allocator hot path). Nonce = salt || txn_id || chunk_idx.
pub fn seal_into(
    ctx: &EncContext,
    class: u8,
    key_path: &str,
    txn_id: u64,
    chunk_idx: u32,
    plaintext: &[u8],
    out: &mut Vec<u8>,
) -> Result<(), StatefsError> {
    let cipher = ctx.cipher(class)?;
    let hdr = header(class, txn_id, chunk_idx);
    let mut aad = [0u8; AAD_BUF];
    let aad_len = build_aad(&hdr, key_path, &mut aad);
    out.clear();
    out.extend_from_slice(&hdr);
    out.extend_from_slice(plaintext);
    let tag: Tag = cipher
        .encrypt_in_place_detached(
            &ctx.nonce(txn_id, chunk_idx),
            &aad[..aad_len],
            &mut out[SEALED_HEADER_LEN..],
        )
        .map_err(|_| StatefsError::IntegrityViolation)?;
    out.extend_from_slice(&tag);
    Ok(())
}

/// Open a sealed value; any failure (structure, class, MAC, splice) is
/// `IntegrityViolation` — tampered ciphertext is never served as plaintext.
pub fn open(ctx: &EncContext, key_path: &str, sealed: &[u8]) -> Result<Vec<u8>, StatefsError> {
    if !is_sealed(sealed) || sealed[6] != 0 || sealed[7] != 0 {
        return Err(StatefsError::IntegrityViolation);
    }
    let class = sealed[5];
    let cipher = ctx.cipher(class)?;
    let mut hdr = [0u8; SEALED_HEADER_LEN];
    hdr.copy_from_slice(&sealed[..SEALED_HEADER_LEN]);
    let mut id = [0u8; 8];
    id.copy_from_slice(&hdr[8..16]);
    let txn_id = u64::from_le_bytes(id);
    let mut idx = [0u8; 4];
    idx.copy_from_slice(&hdr[16..20]);
    let chunk_idx = u32::from_le_bytes(idx);
    let mut aad = [0u8; AAD_BUF];
    let aad_len = build_aad(&hdr, key_path, &mut aad);
    let ct_end = sealed.len() - TAG_LEN;
    let mut buf = Vec::with_capacity(ct_end - SEALED_HEADER_LEN);
    buf.extend_from_slice(&sealed[SEALED_HEADER_LEN..ct_end]);
    let tag = Tag::clone_from_slice(&sealed[ct_end..]);
    cipher
        .decrypt_in_place_detached(&ctx.nonce(txn_id, chunk_idx), &aad[..aad_len], &mut buf, &tag)
        .map_err(|_| StatefsError::IntegrityViolation)?;
    Ok(buf)
}

/// Verify a sealed value without keeping the plaintext (replay path).
#[must_use]
pub fn verify(ctx: &EncContext, key_path: &str, sealed: &[u8]) -> bool {
    open(ctx, key_path, sealed).is_ok()
}

/// Enablement meta record (`META_KEY`, plaintext by construction).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncMeta {
    pub salt: [u8; SALT_LEN],
}

/// Encode the meta record (fixed length, CRC-terminated).
#[must_use]
pub fn encode_meta(meta: &EncMeta) -> [u8; META_LEN] {
    let mut buf = [0u8; META_LEN];
    buf[..4].copy_from_slice(&META_MAGIC);
    buf[4] = META_VERSION;
    buf[5..5 + SALT_LEN].copy_from_slice(&meta.salt);
    let crc = crate::crc32c(&buf[..META_LEN - 4]);
    buf[META_LEN - 4..].copy_from_slice(&crc.to_le_bytes());
    buf
}

/// Decode + validate a meta record. A degenerate all-zero salt is rejected
/// (entropy honesty — RED rule: never claim secure encryption without a
/// real salt provenance).
pub fn decode_meta(bytes: &[u8]) -> Result<EncMeta, StatefsError> {
    if bytes.len() != META_LEN || bytes[..4] != META_MAGIC || bytes[4] != META_VERSION {
        return Err(StatefsError::IntegrityViolation);
    }
    let mut crc = [0u8; 4];
    crc.copy_from_slice(&bytes[META_LEN - 4..]);
    if crate::crc32c(&bytes[..META_LEN - 4]) != u32::from_le_bytes(crc) {
        return Err(StatefsError::IntegrityViolation);
    }
    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&bytes[5..5 + SALT_LEN]);
    if salt == [0u8; SALT_LEN] {
        return Err(StatefsError::IntegrityViolation);
    }
    Ok(EncMeta { salt })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> EncContext {
        let mut ctx = EncContext::new([7u8; SALT_LEN]);
        let key = record_key_from_ikm(b"test-ikm", "app").unwrap();
        let class = ctx.add_class("app", &key).unwrap();
        ctx.enroll("/state/app/", class).unwrap();
        ctx
    }

    #[test]
    fn seal_open_roundtrip_and_header() {
        let ctx = ctx();
        let mut sealed = Vec::new();
        seal_into(&ctx, 0, "/state/app/x", 42, 3, b"payload", &mut sealed).unwrap();
        assert!(is_sealed(&sealed));
        assert_eq!(sealed_txn_id(&sealed), Some(42));
        assert_eq!(sealed.len(), b"payload".len() + SEALED_OVERHEAD);
        assert_eq!(open(&ctx, "/state/app/x", &sealed).unwrap(), b"payload");
    }

    #[test]
    fn test_reject_tampered_ciphertext_any_region() {
        let ctx = ctx();
        let mut sealed = Vec::new();
        seal_into(&ctx, 0, "/state/app/x", 1, 0, b"secret-value", &mut sealed).unwrap();
        for i in 0..sealed.len() {
            let mut bad = sealed.clone();
            bad[i] ^= 0x01;
            assert!(open(&ctx, "/state/app/x", &bad).is_err(), "flip at byte {i} must not open");
        }
    }

    #[test]
    fn test_reject_ciphertext_splice_across_keys() {
        // AAD binds the key path: ciphertext moved to another key fails.
        let ctx = ctx();
        let mut sealed = Vec::new();
        seal_into(&ctx, 0, "/state/app/a", 1, 0, b"value-a", &mut sealed).unwrap();
        assert!(open(&ctx, "/state/app/b", &sealed).is_err());
        assert!(open(&ctx, "/state/app/a", &sealed).is_ok());
    }

    #[test]
    fn test_reject_truncated_and_unsealed() {
        let ctx = ctx();
        assert!(open(&ctx, "/state/app/x", b"plain").is_err());
        let mut sealed = Vec::new();
        seal_into(&ctx, 0, "/state/app/x", 1, 0, b"v", &mut sealed).unwrap();
        assert!(open(&ctx, "/state/app/x", &sealed[..SEALED_OVERHEAD - 1]).is_err());
    }

    #[test]
    fn test_reject_boot_critical_prefix_enrollment() {
        let mut ctx = EncContext::new([7u8; SALT_LEN]);
        let key = record_key_from_ikm(b"ikm", "app").unwrap();
        let class = ctx.add_class("app", &key).unwrap();
        // The chicken-egg rule: records the boot chain needs before key
        // material exists can never be enrolled — nor the meta switch.
        for p in ["/state/keystore/", "/state/boot/", "/state/statefsd/", "/state/"] {
            assert_eq!(ctx.enroll(p, class), Err(StatefsError::AccessDenied), "{p}");
        }
        assert_eq!(ctx.enroll("/data/x/", class), Err(StatefsError::InvalidKey));
        assert_eq!(ctx.enroll("/state/app", class), Err(StatefsError::InvalidKey));
        assert!(ctx.enroll("/state/app/", class).is_ok());
    }

    #[test]
    fn nonce_differs_per_txn_and_chunk() {
        let ctx = ctx();
        let mut a = Vec::new();
        let mut b = Vec::new();
        let mut c = Vec::new();
        seal_into(&ctx, 0, "/state/app/x", 1, 0, b"same", &mut a).unwrap();
        seal_into(&ctx, 0, "/state/app/x", 2, 0, b"same", &mut b).unwrap();
        seal_into(&ctx, 0, "/state/app/x", 1, 1, b"same", &mut c).unwrap();
        // Distinct nonces ⇒ distinct ciphertexts for identical plaintext.
        assert_ne!(a[SEALED_HEADER_LEN..], b[SEALED_HEADER_LEN..]);
        assert_ne!(a[SEALED_HEADER_LEN..], c[SEALED_HEADER_LEN..]);
    }

    #[test]
    fn record_keys_are_domain_separated() {
        let a = record_key_from_ikm(b"ikm", "app").unwrap();
        let b = record_key_from_ikm(b"ikm", "app2").unwrap();
        assert_ne!(a.0, b.0);
        // And never equal the raw ikm.
        let ikm = [0x5au8; 32];
        assert_ne!(record_key_from_ikm(&ikm, "app").unwrap().0, ikm);
    }

    #[test]
    fn test_reject_bad_class_names() {
        let mut label = [0u8; MAX_LABEL_LEN];
        assert!(record_label("", &mut label).is_err());
        assert!(record_label("UPPER", &mut label).is_err());
        assert!(record_label("has space", &mut label).is_err());
        assert!(record_label("waaaaaaaaaaaaaaaaaaaaaay-too-long", &mut label).is_err());
        assert!(record_label("app-1", &mut label).is_ok());
    }

    #[test]
    fn meta_roundtrip_and_rejects() {
        let meta = EncMeta { salt: [9u8; SALT_LEN] };
        let bytes = encode_meta(&meta);
        assert_eq!(decode_meta(&bytes).unwrap(), meta);
        // CRC damage.
        let mut bad = bytes;
        bad[6] ^= 0xFF;
        assert!(decode_meta(&bad).is_err());
        // Truncation, magic, zero salt.
        assert!(decode_meta(&bytes[..META_LEN - 1]).is_err());
        let mut wrong_magic = bytes;
        wrong_magic[0] = b'X';
        assert!(decode_meta(&wrong_magic).is_err());
        assert!(decode_meta(&encode_meta(&EncMeta { salt: [0u8; SALT_LEN] })).is_err());
    }
}
