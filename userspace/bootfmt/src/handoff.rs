// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Measured-boot handoff page codec (ADR-0059 ABI v1). `nxboot`
//! writes ONE 4 KiB page at `HANDOFF_ADDR` before jumping to the verified
//! boot image; the kernel probes the magic + CRC and exposes the record
//! read-only (bootctld is the userland surface owner). The record carries
//! MEASUREMENT (what booted), never policy, and is immutable after handoff.
//! OWNERS: @reliability @security @kernel-team
//! PUBLIC API: Handoff, encode_page, decode
//! TEST_COVERAGE: golden layout, roundtrip, CRC tamper, absent magic
//! ADR: docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md

use super::bsb::Slot;
use super::{crc32_ieee, FmtError};

pub const MAGIC: &[u8; 8] = b"NXHO0001";
pub const VERSION: u16 = 1;
/// The handoff record occupies one page; bytes [60..4096) are reserved (0).
pub const PAGE: usize = 4096;
/// Fixed guest-RAM address of the page (ADR-0059 memory contract, frozen
/// after the TASK-0289-A memory-map audit: above EVERY kernel-managed
/// range — user VMO arena ends 0x9180_0000, RAM ends 0x9400_0000 — so the
/// record can never be recycled into a user VMO; the kernel asserts this
/// at consumption time).
pub const ADDR: usize = 0x9300_0000;
/// CRC32 covers bytes [0..56); the CRC itself sits at [56..60).
const CRC_OFF: usize = 56;

/// Decoded measured-boot record (ADR-0059 v1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handoff {
    /// Slot the loader verified and jumped into.
    pub boot_slot: Slot,
    /// Whether this boot consumed a trial try (BSB actuator write happened).
    pub tries_decremented: bool,
    /// `rollback_index` of the booted NXBD.
    pub rollback_index: u32,
    /// Streamed sha256 of the booted image payload.
    pub image_sha256: [u8; 32],
    /// `seq` of the BSB block the selection was read from.
    pub bsb_seq: u64,
}

/// Encodes the record into a full zero-padded page.
pub fn encode_page(h: &Handoff) -> [u8; PAGE] {
    let mut out = [0u8; PAGE];
    out[0..8].copy_from_slice(MAGIC);
    out[8..10].copy_from_slice(&VERSION.to_le_bytes());
    out[10] = match h.boot_slot {
        Slot::A => 0,
        Slot::B => 1,
    };
    out[11] = u8::from(h.tries_decremented);
    out[12..16].copy_from_slice(&h.rollback_index.to_le_bytes());
    out[16..48].copy_from_slice(&h.image_sha256);
    out[48..56].copy_from_slice(&h.bsb_seq.to_le_bytes());
    let crc = crc32_ieee(&out[..CRC_OFF]);
    out[CRC_OFF..CRC_OFF + 4].copy_from_slice(&crc.to_le_bytes());
    out
}

/// Bounded decode: needs at least the first 60 bytes of the page. A missing
/// magic is `Malformed` (the honest "direct kernel boot, no loader" case);
/// a present magic with a bad CRC is `Crc` (torn or corrupt record).
pub fn decode(bytes: &[u8]) -> Result<Handoff, FmtError> {
    if bytes.len() < CRC_OFF + 4 || &bytes[0..8] != MAGIC {
        return Err(FmtError::Malformed);
    }
    let stored = u32::from_le_bytes([bytes[56], bytes[57], bytes[58], bytes[59]]);
    if crc32_ieee(&bytes[..CRC_OFF]) != stored {
        return Err(FmtError::Crc);
    }
    if u16::from_le_bytes([bytes[8], bytes[9]]) != VERSION {
        return Err(FmtError::Malformed);
    }
    let boot_slot = match bytes[10] {
        0 => Slot::A,
        1 => Slot::B,
        _ => return Err(FmtError::Malformed),
    };
    if bytes[11] > 1 {
        return Err(FmtError::Malformed);
    }
    let mut sha = [0u8; 32];
    sha.copy_from_slice(&bytes[16..48]);
    let mut seq = [0u8; 8];
    seq.copy_from_slice(&bytes[48..56]);
    Ok(Handoff {
        boot_slot,
        tries_decremented: bytes[11] == 1,
        rollback_index: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
        image_sha256: sha,
        bsb_seq: u64::from_le_bytes(seq),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Handoff {
        Handoff {
            boot_slot: Slot::B,
            tries_decremented: true,
            rollback_index: 3,
            image_sha256: [0xCD; 32],
            bsb_seq: 42,
        }
    }

    #[test]
    fn golden_layout_and_roundtrip() {
        let page = encode_page(&sample());
        assert_eq!(&page[0..8], MAGIC);
        assert_eq!(page[10], 1, "slot b wire byte");
        assert_eq!(page[11], 1, "tries_decremented wire byte");
        assert_eq!(&page[12..16], &3u32.to_le_bytes());
        assert_eq!(&page[48..56], &42u64.to_le_bytes());
        assert!(page[60..].iter().all(|&b| b == 0), "reserved tail must be zero");
        assert_eq!(decode(&page).expect("decode"), sample());
        // Decoding the minimal 60-byte prefix must work (kernel probe path).
        assert_eq!(decode(&page[..60]).expect("prefix decode"), sample());
    }

    #[test]
    fn rejects_tamper_and_absence() {
        let mut page = encode_page(&sample());
        page[12] ^= 1;
        assert_eq!(decode(&page), Err(FmtError::Crc));
        // Absent magic = direct kernel boot: Malformed, never a fake record.
        let zeroed = [0u8; PAGE];
        assert_eq!(decode(&zeroed), Err(FmtError::Malformed));
        assert_eq!(decode(&[0u8; 12]), Err(FmtError::Malformed));
    }

    #[test]
    fn rejects_bad_version_slot_and_bool() {
        let good = encode_page(&sample());
        // Version bump must be rejected (fail closed on unknown ABI).
        let mut page = good;
        page[8] = 2;
        let crc = super::crc32_ieee(&page[..56]);
        page[56..60].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(decode(&page), Err(FmtError::Malformed));
        // Slot byte outside {0,1}.
        let mut page = good;
        page[10] = 7;
        let crc = super::crc32_ieee(&page[..56]);
        page[56..60].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(decode(&page), Err(FmtError::Malformed));
        // tries_decremented outside {0,1}.
        let mut page = good;
        page[11] = 2;
        let crc = super::crc32_ieee(&page[..56]);
        page[56..60].copy_from_slice(&crc.to_le_bytes());
        assert_eq!(decode(&page), Err(FmtError::Malformed));
    }
}
