// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: nxfs on-disk format v1 (RFC-0071 §Contract): superblock with dual
//! checkpoint slots, crc32c discipline, and the global bounds. Every
//! structure is length-explicit and validated before use — malformed bytes
//! are a deterministic error, never UB.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0292)
//! TEST_COVERAGE: superblock roundtrip/reject tests below

use crate::{NxfsError, Result};

/// nxfs logical block size (bytes). Devices with smaller sectors are adapted
/// by `dev::Dev`.
pub const LOGICAL_BLOCK_SIZE: usize = 4096;
/// On-disk format version.
pub const NXFS_VERSION: u16 = 1;
/// Superblock magic.
pub const MAGIC: [u8; 4] = *b"NXFS";
/// Journal record magic.
pub const JOURNAL_MAGIC: [u8; 4] = *b"NXJL";

/// Maximum entry-name length in bytes (RFC-0071).
pub const MAX_NAME_LEN: usize = 255;
/// Maximum path depth (RFC-0071).
pub const MAX_DEPTH: usize = 32;
/// Root directory object id.
pub const ROOT_OBJECT: u64 = 1;

/// Object kinds (on-disk).
pub const KIND_FILE: u8 = 0;
pub const KIND_DIR: u8 = 1;

/// Container UUID (opaque 16 bytes, injected by mkfs — no RNG in the engine).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Uuid(pub [u8; 16]);

/// One checkpoint pointer slot (A/B) inside the superblock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CheckpointSlot {
    /// Monotonic generation; 0 = slot empty/invalid.
    pub generation: u64,
    /// First logical block of the serialized checkpoint.
    pub root_lb: u64,
    /// Serialized checkpoint length in bytes.
    pub len_bytes: u64,
    /// crc32c over the serialized checkpoint bytes.
    pub crc: u32,
}

/// The superblock (logical block 0, mirrored in the LAST logical block).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Superblock {
    pub uuid: Uuid,
    /// Total logical blocks in the container.
    pub total_blocks: u64,
    /// First logical block of the journal region.
    pub journal_start: u64,
    /// Journal region length in logical blocks.
    pub journal_blocks: u64,
    /// Checkpoint slots; mount picks the newest VALID one.
    pub slots: [CheckpointSlot; 2],
    /// Encryption mode (0 = off; RFC-0071 Phase 4 flips this).
    pub enc_mode: u8,
}

const SB_ENCODED_LEN: usize = 4 + 2 + 16 + 8 + 8 + 8 + 2 * (8 + 8 + 8 + 4) + 1 + 4;

impl Superblock {
    /// Serializes into one logical block (zero-padded, crc32c-terminated).
    pub fn encode(&self) -> [u8; LOGICAL_BLOCK_SIZE] {
        let mut out = [0u8; LOGICAL_BLOCK_SIZE];
        let mut off = 0usize;
        out[off..off + 4].copy_from_slice(&MAGIC);
        off += 4;
        out[off..off + 2].copy_from_slice(&NXFS_VERSION.to_le_bytes());
        off += 2;
        out[off..off + 16].copy_from_slice(&self.uuid.0);
        off += 16;
        out[off..off + 8].copy_from_slice(&self.total_blocks.to_le_bytes());
        off += 8;
        out[off..off + 8].copy_from_slice(&self.journal_start.to_le_bytes());
        off += 8;
        out[off..off + 8].copy_from_slice(&self.journal_blocks.to_le_bytes());
        off += 8;
        for slot in &self.slots {
            out[off..off + 8].copy_from_slice(&slot.generation.to_le_bytes());
            off += 8;
            out[off..off + 8].copy_from_slice(&slot.root_lb.to_le_bytes());
            off += 8;
            out[off..off + 8].copy_from_slice(&slot.len_bytes.to_le_bytes());
            off += 8;
            out[off..off + 4].copy_from_slice(&slot.crc.to_le_bytes());
            off += 4;
        }
        out[off] = self.enc_mode;
        off += 1;
        let crc = crc32c(&out[..off]);
        out[off..off + 4].copy_from_slice(&crc.to_le_bytes());
        out
    }

    /// Parses + validates one logical block. Fail-closed on any mismatch.
    pub fn decode(block: &[u8]) -> Result<Self> {
        if block.len() < SB_ENCODED_LEN {
            return Err(NxfsError::Integrity);
        }
        if block[0..4] != MAGIC {
            return Err(NxfsError::Integrity);
        }
        let version = u16::from_le_bytes([block[4], block[5]]);
        if version != NXFS_VERSION {
            return Err(NxfsError::Integrity);
        }
        let stored_crc = u32::from_le_bytes([
            block[SB_ENCODED_LEN - 4],
            block[SB_ENCODED_LEN - 3],
            block[SB_ENCODED_LEN - 2],
            block[SB_ENCODED_LEN - 1],
        ]);
        if crc32c(&block[..SB_ENCODED_LEN - 4]) != stored_crc {
            return Err(NxfsError::Integrity);
        }
        let mut off = 6usize;
        let mut uuid = [0u8; 16];
        uuid.copy_from_slice(&block[off..off + 16]);
        off += 16;
        let total_blocks = read_u64(block, &mut off);
        let journal_start = read_u64(block, &mut off);
        let journal_blocks = read_u64(block, &mut off);
        let mut slots = [CheckpointSlot::default(); 2];
        for slot in &mut slots {
            slot.generation = read_u64(block, &mut off);
            slot.root_lb = read_u64(block, &mut off);
            slot.len_bytes = read_u64(block, &mut off);
            slot.crc = read_u32(block, &mut off);
        }
        let enc_mode = block[off];
        // Structural sanity: the journal must live inside the container.
        if journal_start == 0
            || journal_blocks == 0
            || journal_start.saturating_add(journal_blocks) >= total_blocks
        {
            return Err(NxfsError::Integrity);
        }
        Ok(Self {
            uuid: Uuid(uuid),
            total_blocks,
            journal_start,
            journal_blocks,
            slots,
            enc_mode,
        })
    }

    /// The slot index the next checkpoint should overwrite (the OLDER one).
    #[must_use]
    pub fn older_slot(&self) -> usize {
        usize::from(self.slots[0].generation > self.slots[1].generation)
    }

    /// The newest valid slot index by generation (validity = generation > 0;
    /// checkpoint bytes are verified separately against `crc`).
    #[must_use]
    pub fn newest_slot(&self) -> Option<usize> {
        match (self.slots[0].generation, self.slots[1].generation) {
            (0, 0) => None,
            (a, b) if a >= b => Some(0),
            _ => Some(1),
        }
    }
}

fn read_u64(buf: &[u8], off: &mut usize) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&buf[*off..*off + 8]);
    *off += 8;
    u64::from_le_bytes(bytes)
}

fn read_u32(buf: &[u8], off: &mut usize) -> u32 {
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&buf[*off..*off + 4]);
    *off += 4;
    u32::from_le_bytes(bytes)
}

/// crc32c (Castagnoli), bitwise implementation — no tables, no_std, and the
/// same polynomial statefs uses (shared discipline, independent code).
#[must_use]
pub fn crc32c(data: &[u8]) -> u32 {
    let mut crc: u32 = !0;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0x82F6_3B78 & mask);
        }
    }
    !crc
}

/// Validates one path segment (entry name) against the format bounds.
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        return Err(NxfsError::TooBig);
    }
    if name == "." || name == ".." || name.contains('/') || name.contains('\0') {
        return Err(NxfsError::Invalid);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Superblock {
        Superblock {
            uuid: Uuid([7; 16]),
            total_blocks: 1024,
            journal_start: 2,
            journal_blocks: 64,
            slots: [
                CheckpointSlot { generation: 3, root_lb: 100, len_bytes: 900, crc: 0xABCD },
                CheckpointSlot { generation: 2, root_lb: 200, len_bytes: 800, crc: 0x1234 },
            ],
            enc_mode: 0,
        }
    }

    #[test]
    fn superblock_roundtrip() {
        let sb = sample();
        let decoded = Superblock::decode(&sb.encode()).expect("decode");
        assert_eq!(decoded, sb);
        assert_eq!(decoded.newest_slot(), Some(0));
        assert_eq!(decoded.older_slot(), 1);
    }

    #[test]
    fn test_reject_corrupt_superblock() {
        let sb = sample();
        let mut block = sb.encode();
        block[10] ^= 0xFF;
        assert_eq!(Superblock::decode(&block), Err(NxfsError::Integrity));
        // Bad magic
        let mut block = sb.encode();
        block[0] = b'X';
        assert_eq!(Superblock::decode(&block), Err(NxfsError::Integrity));
        // Journal outside container
        let mut bad = sample();
        bad.journal_start = 2000;
        assert_eq!(Superblock::decode(&bad.encode()), Err(NxfsError::Integrity));
    }

    #[test]
    fn test_reject_bad_names() {
        assert!(validate_name("ok.txt").is_ok());
        assert_eq!(validate_name(""), Err(NxfsError::TooBig));
        assert_eq!(validate_name(&"x".repeat(256)), Err(NxfsError::TooBig));
        assert_eq!(validate_name(".."), Err(NxfsError::Invalid));
        assert_eq!(validate_name("a/b"), Err(NxfsError::Invalid));
    }
}
