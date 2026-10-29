// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the seal of a record that leaves the kernel (RFC-0100 §5, TASK-0333 6f-2, ADR-0088) —
//! ChaCha20-Poly1305 under the boot's swap key. The nonce is the kernel's swap counter (one per
//! export, never reused under one key); the associated data is the page's identity (object,
//! page index), so a sealed record opens only as the page it was sealed for, and only under the
//! counter the kernel recorded for it — an older record of the same page never opens again. The
//! pager (memd) and the disk see ciphertext alone; a record that does not open is refused, never
//! entered. In place, allocation-free. Pure, host-proven.
//!
//! NOT target-gated (a child of `lazy`).
//!
//! OWNERS: @kernel-mm-team
//! STATUS: Functional (TASK-0333 6f-2)
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below — a round trip, the ciphertext unlike the record
//!   (`test_reject_*`: a flipped byte, a flipped tag byte, another page's identity, another
//!   counter, a short buffer)
//! INVARIANTS: a record is entered only after its tag verified under the page's own counter and
//!   identity; the key never leaves this type.

use chacha20poly1305::aead::{AeadInPlace, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce, Tag};

/// The key's bytes.
pub const KEY_LEN: usize = 32;
/// The tag a sealed record carries after its bytes.
pub const TAG_LEN: usize = 16;
/// The largest sealed record: the largest record the store takes and its tag.
pub const MAX_SEALED: usize = super::store::MAX_RECORD + TAG_LEN;

/// Why a seal or an open failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealError {
    /// The buffer has no room for the tag (seal), or is shorter than a tag (open).
    Short,
    /// The tag did not verify: another key, counter or page, or the bytes changed.
    Forged,
}

/// The boot's sealer: the swap key, kept inside the cipher.
pub struct Sealer(ChaCha20Poly1305);

impl Sealer {
    /// A sealer under `key`.
    #[must_use]
    pub fn new(key: &[u8; KEY_LEN]) -> Self {
        Self(ChaCha20Poly1305::new(Key::from_slice(key)))
    }

    /// Seal the record in `buf[..len]` in place and put the tag after it, for page `index` of
    /// object `object` under counter `ctr`. Answers the sealed length (`len + TAG_LEN`).
    pub fn seal(
        &self,
        ctr: u64,
        object: u32,
        index: u32,
        buf: &mut [u8],
        len: usize,
    ) -> Result<usize, SealError> {
        if len.checked_add(TAG_LEN).is_none_or(|end| end > buf.len()) {
            return Err(SealError::Short);
        }
        let (record, rest) = buf.split_at_mut(len);
        let tag = self
            .0
            .encrypt_in_place_detached(&nonce(ctr), &identity(object, index), record)
            .map_err(|_| SealError::Short)?;
        rest[..TAG_LEN].copy_from_slice(tag.as_slice());
        Ok(len + TAG_LEN)
    }

    /// Open the sealed record in `buf` in place, as page `index` of object `object` under
    /// counter `ctr`. Answers the record's length; on `Forged` the bytes are not a record.
    pub fn open(
        &self,
        ctr: u64,
        object: u32,
        index: u32,
        buf: &mut [u8],
    ) -> Result<usize, SealError> {
        let len = buf.len().checked_sub(TAG_LEN).ok_or(SealError::Short)?;
        let mut tag = [0u8; TAG_LEN];
        tag.copy_from_slice(&buf[len..]);
        self.0
            .decrypt_in_place_detached(
                &nonce(ctr),
                &identity(object, index),
                &mut buf[..len],
                Tag::from_slice(&tag),
            )
            .map_err(|_| SealError::Forged)?;
        Ok(len)
    }
}

/// The nonce: the counter in the first eight bytes, zero after.
fn nonce(ctr: u64) -> Nonce {
    let mut bytes = [0u8; 12];
    bytes[..8].copy_from_slice(&ctr.to_le_bytes());
    *Nonce::from_slice(&bytes)
}

/// The associated data: the page's identity.
fn identity(object: u32, index: u32) -> [u8; 8] {
    let mut bytes = [0u8; 8];
    bytes[..4].copy_from_slice(&object.to_le_bytes());
    bytes[4..].copy_from_slice(&index.to_le_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; KEY_LEN] = [0x5a; KEY_LEN];

    fn record() -> Vec<u8> {
        (0..700u32).map(|i| (i * 13 % 251) as u8).collect()
    }

    fn sealed(sealer: &Sealer, ctr: u64, object: u32, index: u32) -> Vec<u8> {
        let mut buf = record();
        buf.resize(buf.len() + TAG_LEN, 0);
        let n = sealer.seal(ctr, object, index, &mut buf, 700).unwrap();
        assert_eq!(n, 700 + TAG_LEN);
        buf
    }

    #[test]
    fn a_sealed_record_opens_as_its_own_page_only() {
        let sealer = Sealer::new(&KEY);
        let mut buf = sealed(&sealer, 7, 3, 12);
        assert_ne!(&buf[..700], &record()[..], "the pager sees ciphertext");
        assert_eq!(sealer.open(7, 3, 12, &mut buf), Ok(700));
        assert_eq!(&buf[..700], &record()[..]);
    }

    #[test]
    fn test_reject_a_changed_byte_or_tag() {
        let sealer = Sealer::new(&KEY);
        let mut body = sealed(&sealer, 7, 3, 12);
        body[100] ^= 1;
        assert_eq!(sealer.open(7, 3, 12, &mut body), Err(SealError::Forged));
        let mut tag = sealed(&sealer, 7, 3, 12);
        let last = tag.len() - 1;
        tag[last] ^= 0x80;
        assert_eq!(sealer.open(7, 3, 12, &mut tag), Err(SealError::Forged));
    }

    /// Another page's record, or this page's under an older counter (a replay), never opens.
    #[test]
    fn test_reject_another_page_or_counter() {
        let sealer = Sealer::new(&KEY);
        let mut other_page = sealed(&sealer, 7, 3, 13);
        assert_eq!(sealer.open(7, 3, 12, &mut other_page), Err(SealError::Forged));
        let mut other_object = sealed(&sealer, 7, 4, 12);
        assert_eq!(sealer.open(7, 3, 12, &mut other_object), Err(SealError::Forged));
        let mut replayed = sealed(&sealer, 6, 3, 12);
        assert_eq!(sealer.open(7, 3, 12, &mut replayed), Err(SealError::Forged));
        let mut other_key = sealed(&Sealer::new(&[1; KEY_LEN]), 7, 3, 12);
        assert_eq!(sealer.open(7, 3, 12, &mut other_key), Err(SealError::Forged));
    }

    #[test]
    fn test_reject_a_short_buffer() {
        let sealer = Sealer::new(&KEY);
        let mut tight = record();
        assert_eq!(
            sealer.seal(1, 1, 1, &mut tight, 700),
            Err(SealError::Short),
            "no room for the tag"
        );
        let mut tiny = [0u8; TAG_LEN - 1];
        assert_eq!(sealer.open(1, 1, 1, &mut tiny), Err(SealError::Short));
        let mut bare_tag = [0u8; TAG_LEN];
        assert_eq!(sealer.open(1, 1, 1, &mut bare_tag), Err(SealError::Forged), "an empty record");
    }
}
