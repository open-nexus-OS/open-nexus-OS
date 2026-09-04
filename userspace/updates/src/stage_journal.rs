// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: The `NXSJ` stage journal (RFC-0089 §12.2 sector 1 of the
//! INACTIVE system slot; TASK-0035 P1). One 512-byte sector recording which
//! bundle windows of the volume being assembled were already written AND
//! readback-verified, bound to the target volume's identity (the NXSV's
//! `volume_sha256` + `index_sha256` and the bundle count). A restage with
//! the same target readback-verifies every journalled window instead of
//! rewriting it — verification is never skipped (the engine still streams
//! and hashes every shipped component; the journal only avoids rewrites).
//! A journal that does not bind, or fails its CRC, is ignored and zeroed;
//! the journal is zeroed after the NXSV commit (a valid NXSV supersedes it).
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (roundtrip, `test_reject_journal_crc`,
//!   `test_reject_journal_magic`, bounds); resume matrix in
//!   tests/updates_host/tests/component_set_volume.rs
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

/// Journal magic (versioned).
pub const MAGIC: [u8; 8] = *b"NXSJ0001";
/// Sector index (partition-relative) of the journal on a system slot.
pub const JOURNAL_SECTOR: u64 = 1;
/// Journal sector size.
pub const SECTOR: usize = 512;
/// Largest bundle table the bitmap covers.
pub const MAX_BUNDLES: usize = 256;

const OFF_VOLUME_SHA: usize = 8;
const OFF_INDEX_SHA: usize = 40;
const OFF_BUNDLES: usize = 72;
const OFF_BITMAP: usize = 74;
const OFF_CRC: usize = 106;
const LEN: usize = 110;

/// Decode failures — the caller treats every one as "no journal".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalError {
    Magic,
    Crc,
    Bounds,
}

/// The journal record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Journal {
    pub volume_sha256: [u8; 32],
    pub index_sha256: [u8; 32],
    pub bundles: u16,
    completed: [u8; 32],
}

impl Journal {
    /// A fresh journal for a target volume (nothing completed).
    pub fn new(volume_sha256: [u8; 32], index_sha256: [u8; 32], bundles: usize) -> Option<Self> {
        if bundles == 0 || bundles > MAX_BUNDLES {
            return None;
        }
        Some(Self { volume_sha256, index_sha256, bundles: bundles as u16, completed: [0u8; 32] })
    }

    /// `true` iff this journal describes exactly that target.
    pub fn binds(&self, volume_sha256: &[u8; 32], index_sha256: &[u8; 32], bundles: usize) -> bool {
        &self.volume_sha256 == volume_sha256
            && &self.index_sha256 == index_sha256
            && usize::from(self.bundles) == bundles
    }

    pub fn is_done(&self, row: usize) -> bool {
        row < usize::from(self.bundles) && (self.completed[row / 8] >> (row % 8)) & 1 == 1
    }

    pub fn set_done(&mut self, row: usize, done: bool) {
        if row >= usize::from(self.bundles) {
            return;
        }
        if done {
            self.completed[row / 8] |= 1 << (row % 8);
        } else {
            self.completed[row / 8] &= !(1 << (row % 8));
        }
    }

    pub fn done_count(&self) -> usize {
        (0..usize::from(self.bundles)).filter(|&r| self.is_done(r)).count()
    }

    /// The sector image (CRC-32/IEEE over the record before it).
    pub fn encode(&self) -> [u8; SECTOR] {
        let mut out = [0u8; SECTOR];
        out[..8].copy_from_slice(&MAGIC);
        out[OFF_VOLUME_SHA..OFF_VOLUME_SHA + 32].copy_from_slice(&self.volume_sha256);
        out[OFF_INDEX_SHA..OFF_INDEX_SHA + 32].copy_from_slice(&self.index_sha256);
        out[OFF_BUNDLES..OFF_BUNDLES + 2].copy_from_slice(&self.bundles.to_le_bytes());
        out[OFF_BITMAP..OFF_BITMAP + 32].copy_from_slice(&self.completed);
        let crc = storage::gpt::crc32_ieee(&out[..OFF_CRC]);
        out[OFF_CRC..LEN].copy_from_slice(&crc.to_le_bytes());
        out
    }

    /// Parses a sector; magic, CRC and the bundle-count bound are checked.
    pub fn decode(sector: &[u8]) -> Result<Self, JournalError> {
        if sector.len() < LEN {
            return Err(JournalError::Bounds);
        }
        if sector[..8] != MAGIC {
            return Err(JournalError::Magic);
        }
        let crc = u32::from_le_bytes([
            sector[OFF_CRC],
            sector[OFF_CRC + 1],
            sector[OFF_CRC + 2],
            sector[OFF_CRC + 3],
        ]);
        if storage::gpt::crc32_ieee(&sector[..OFF_CRC]) != crc {
            return Err(JournalError::Crc);
        }
        let bundles = u16::from_le_bytes([sector[OFF_BUNDLES], sector[OFF_BUNDLES + 1]]);
        if bundles == 0 || usize::from(bundles) > MAX_BUNDLES {
            return Err(JournalError::Bounds);
        }
        let mut volume_sha256 = [0u8; 32];
        volume_sha256.copy_from_slice(&sector[OFF_VOLUME_SHA..OFF_VOLUME_SHA + 32]);
        let mut index_sha256 = [0u8; 32];
        index_sha256.copy_from_slice(&sector[OFF_INDEX_SHA..OFF_INDEX_SHA + 32]);
        let mut completed = [0u8; 32];
        completed.copy_from_slice(&sector[OFF_BITMAP..OFF_BITMAP + 32]);
        Ok(Self { volume_sha256, index_sha256, bundles, completed })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_bits() {
        let mut j = Journal::new([1; 32], [2; 32], 22).expect("journal");
        j.set_done(0, true);
        j.set_done(21, true);
        j.set_done(22, true); // out of range: ignored
        let back = Journal::decode(&j.encode()).expect("decode");
        assert_eq!(back, j);
        assert!(back.is_done(0) && back.is_done(21) && !back.is_done(1) && !back.is_done(22));
        assert_eq!(back.done_count(), 2);
        assert!(back.binds(&[1; 32], &[2; 32], 22));
        assert!(!back.binds(&[1; 32], &[2; 32], 21));
        assert!(!back.binds(&[9; 32], &[2; 32], 22));
    }

    #[test]
    fn test_reject_journal_crc() {
        let j = Journal::new([1; 32], [2; 32], 3).expect("journal");
        let mut s = j.encode();
        s[OFF_BITMAP] ^= 0x01;
        assert_eq!(Journal::decode(&s), Err(JournalError::Crc));
    }

    #[test]
    fn test_reject_journal_magic_and_bounds() {
        let j = Journal::new([1; 32], [2; 32], 3).expect("journal");
        let mut s = j.encode();
        s[0] = b'X';
        assert_eq!(Journal::decode(&s), Err(JournalError::Magic));
        assert_eq!(Journal::decode(&[0u8; SECTOR]), Err(JournalError::Magic));
        assert_eq!(Journal::decode(&[0u8; 10]), Err(JournalError::Bounds));
        assert!(Journal::new([0; 32], [0; 32], 0).is_none());
        assert!(Journal::new([0; 32], [0; 32], MAX_BUNDLES + 1).is_none());
    }
}
