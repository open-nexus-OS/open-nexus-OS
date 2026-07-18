// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The partition-scoped block IPC protocol (ADR-0044): virtioblkd is
//! the single virtio-queue owner and serves these frames; statefsd/nxfsd
//! consume them through `RemoteBlockDevice`. ONE codec for both ends — the
//! wire cannot drift. Bounded: at most [`MAX_BLOCKS_PER_REQ`] sectors per
//! request so every frame stays far below the 8 KiB IPC cap.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0293)
//! TEST_COVERAGE: roundtrip + reject tests below

use alloc::vec::Vec;

/// Protocol opcodes (frame byte 0).
pub const OP_INFO: u8 = 1;
pub const OP_READ: u8 = 2;
pub const OP_WRITE: u8 = 3;
pub const OP_SYNC: u8 = 4;

/// Partition selectors (stable across the boot; GPT-derived).
pub const PART_STATE: u8 = 0;
pub const PART_DATA: u8 = 1;

/// Sector size the protocol speaks (virtio-blk native).
pub const SECTOR_SIZE: usize = 512;
/// Bounded sectors per request/response (12 × 512 = 6 KiB payload).
pub const MAX_BLOCKS_PER_REQ: u16 = 12;

/// Reply status byte.
pub const STATUS_OK: u8 = 0;
pub const STATUS_OUT_OF_RANGE: u8 = 1;
pub const STATUS_IO: u8 = 2;
pub const STATUS_MALFORMED: u8 = 3;
pub const STATUS_UNKNOWN_PART: u8 = 4;

/// A decoded request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockRequest<'a> {
    /// Partition geometry query.
    Info { part: u8 },
    /// Read `count` sectors at `lba` (partition-relative).
    Read { part: u8, lba: u64, count: u16 },
    /// Write sectors at `lba`.
    Write { part: u8, lba: u64, data: &'a [u8] },
    /// Durability barrier.
    Sync { part: u8 },
}

/// Encodes an INFO request.
#[must_use]
pub fn encode_info(part: u8) -> Vec<u8> {
    alloc::vec![OP_INFO, part]
}

/// Encodes a READ request.
#[must_use]
pub fn encode_read(part: u8, lba: u64, count: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(12);
    out.push(OP_READ);
    out.push(part);
    out.extend_from_slice(&lba.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out
}

/// Encodes a WRITE request (`data` must be whole sectors, bounded).
#[must_use]
pub fn encode_write(part: u8, lba: u64, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(10 + data.len());
    out.push(OP_WRITE);
    out.push(part);
    out.extend_from_slice(&lba.to_le_bytes());
    out.extend_from_slice(data);
    out
}

/// Encodes a SYNC request.
#[must_use]
pub fn encode_sync(part: u8) -> Vec<u8> {
    alloc::vec![OP_SYNC, part]
}

/// Decodes any request frame (server side). Fail-closed on malformed input.
pub fn decode_request(frame: &[u8]) -> Option<BlockRequest<'_>> {
    match *frame.first()? {
        OP_INFO if frame.len() == 2 => Some(BlockRequest::Info { part: frame[1] }),
        OP_READ if frame.len() == 12 => {
            let lba = u64::from_le_bytes(frame[2..10].try_into().ok()?);
            let count = u16::from_le_bytes(frame[10..12].try_into().ok()?);
            if count == 0 || count > MAX_BLOCKS_PER_REQ {
                return None;
            }
            Some(BlockRequest::Read { part: frame[1], lba, count })
        }
        OP_WRITE if frame.len() > 10 => {
            let lba = u64::from_le_bytes(frame[2..10].try_into().ok()?);
            let data = &frame[10..];
            // `%` not `is_multiple_of`: the OS toolchain (nightly-2025-01-15)
            // predates the `unsigned_is_multiple_of` stabilization (stable 1.87).
            #[allow(clippy::manual_is_multiple_of)]
            if data.is_empty()
                || data.len() % SECTOR_SIZE != 0
                || data.len() / SECTOR_SIZE > MAX_BLOCKS_PER_REQ as usize
            {
                return None;
            }
            Some(BlockRequest::Write { part: frame[1], lba, data })
        }
        OP_SYNC if frame.len() == 2 => Some(BlockRequest::Sync { part: frame[1] }),
        _ => None,
    }
}

/// Encodes a status-only reply (WRITE/SYNC, and every error).
#[must_use]
pub fn encode_status(status: u8) -> Vec<u8> {
    alloc::vec![status]
}

/// Encodes an INFO reply.
#[must_use]
pub fn encode_info_reply(block_size: u32, block_count: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(13);
    out.push(STATUS_OK);
    out.extend_from_slice(&block_size.to_le_bytes());
    out.extend_from_slice(&block_count.to_le_bytes());
    out
}

/// Decodes an INFO reply → `(block_size, block_count)`.
pub fn decode_info_reply(frame: &[u8]) -> Option<(u32, u64)> {
    if frame.len() != 13 || frame[0] != STATUS_OK {
        return None;
    }
    let block_size = u32::from_le_bytes(frame[1..5].try_into().ok()?);
    let block_count = u64::from_le_bytes(frame[5..13].try_into().ok()?);
    Some((block_size, block_count))
}

/// Encodes a READ reply (`data` = the sectors).
#[must_use]
pub fn encode_read_reply(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + data.len());
    out.push(STATUS_OK);
    out.extend_from_slice(data);
    out
}

/// Decodes a READ reply into the sector payload.
pub fn decode_read_reply(frame: &[u8], expect_sectors: u16) -> Option<&[u8]> {
    if frame.first() != Some(&STATUS_OK) {
        return None;
    }
    let data = &frame[1..];
    (data.len() == expect_sectors as usize * SECTOR_SIZE).then_some(data)
}

/// Decodes a status-only reply.
pub fn decode_status(frame: &[u8]) -> Option<u8> {
    (frame.len() == 1).then(|| frame[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_roundtrips() {
        let read = encode_read(PART_DATA, 42, 3);
        assert_eq!(
            decode_request(&read),
            Some(BlockRequest::Read { part: PART_DATA, lba: 42, count: 3 })
        );
        let payload = [7u8; 2 * SECTOR_SIZE];
        let write = encode_write(PART_STATE, 9, &payload);
        match decode_request(&write) {
            Some(BlockRequest::Write { part, lba, data }) => {
                assert_eq!((part, lba), (PART_STATE, 9));
                assert_eq!(data, payload);
            }
            other => panic!("bad decode {other:?}"),
        }
        assert_eq!(decode_request(&encode_info(1)), Some(BlockRequest::Info { part: 1 }));
        assert_eq!(decode_request(&encode_sync(0)), Some(BlockRequest::Sync { part: 0 }));
    }

    #[test]
    fn reply_roundtrips() {
        let (bs, count) = decode_info_reply(&encode_info_reply(512, 131072)).expect("info");
        assert_eq!((bs, count), (512, 131072));
        let sectors = [0xEE; SECTOR_SIZE];
        assert_eq!(decode_read_reply(&encode_read_reply(&sectors), 1), Some(&sectors[..]));
        assert_eq!(decode_status(&encode_status(STATUS_IO)), Some(STATUS_IO));
    }

    #[test]
    fn test_reject_malformed_and_oversize() {
        assert_eq!(decode_request(&[]), None);
        assert_eq!(decode_request(&[OP_READ, 0, 1, 2]), None); // short
        assert_eq!(decode_request(&encode_read(0, 0, 0)), None); // zero count
        assert_eq!(decode_request(&encode_read(0, 0, MAX_BLOCKS_PER_REQ + 1)), None);
        let oversize = alloc::vec![0u8; (MAX_BLOCKS_PER_REQ as usize + 1) * SECTOR_SIZE];
        assert_eq!(decode_request(&encode_write(0, 0, &oversize)), None);
        let ragged = alloc::vec![0u8; SECTOR_SIZE + 1];
        assert_eq!(decode_request(&encode_write(0, 0, &ragged)), None);
        // Read reply with wrong sector count fails closed.
        let sectors = [0u8; SECTOR_SIZE];
        assert_eq!(decode_read_reply(&encode_read_reply(&sectors), 2), None);
    }
}
