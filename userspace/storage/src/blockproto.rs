// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The partition-scoped block IPC protocol (ADR-0044/RFC-0089):
//! virtioblkd is the single virtio-queue owner and serves these frames;
//! statefsd/nxfsd consume them through `RemoteBlockDevice`. ONE codec for
//! both ends — the wire cannot drift. Bounded: at most
//! [`MAX_BLOCKS_PER_REQ`] sectors per request so every frame stays far
//! below the 8 KiB IPC cap. Every frame carries magic + a u32 nonce
//! (RFC-0019 discipline): client reply inboxes are SHARED with other
//! targets (statefsd's inbox also carries policyd replies), so replies
//! must be self-identifying and correlated, never inferred from order.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0293)
//! TEST_COVERAGE: roundtrip + reject tests below

use alloc::vec::Vec;

/// Frame magic + version (requests `[B,K,1,op,nonce…]`, replies echo the
/// header with `op | 0x80`).
pub const MAGIC0: u8 = b'B';
pub const MAGIC1: u8 = b'K';
pub const VERSION: u8 = 1;
/// Request header length: magic(2) + ver + op + nonce(4).
pub const HDR_LEN: usize = 8;

/// Protocol opcodes (frame byte 3).
pub const OP_INFO: u8 = 1;
pub const OP_READ: u8 = 2;
pub const OP_WRITE: u8 = 3;
pub const OP_SYNC: u8 = 4;

/// Partition selectors (stable across the boot; GPT-derived). 0/1 are the
/// TASK-0293-era selectors; 2..6 joined with the RFC-0089 §2 layout.
pub const PART_STATE: u8 = 0;
pub const PART_DATA: u8 = 1;
pub const PART_BSB: u8 = 2;
pub const PART_BOOT_A: u8 = 3;
pub const PART_BOOT_B: u8 = 4;
pub const PART_SYSTEM_A: u8 = 5;
pub const PART_SYSTEM_B: u8 = 6;
/// Number of addressable partitions.
pub const PART_COUNT: u8 = 7;

/// FIXED client wiring slots (init transfers these at spawn-time
/// distribution — BEFORE any request can flow, so the statefsd pristine
/// upgrade window can never be beaten by an early mutating op the way a
/// route-get could). High in the 256-slot table, far above the kernel's
/// sequential picks. Same numbers in every client's table.
pub const CLIENT_REQ_SLOT: u32 = 0xF0;
pub const CLIENT_REPLY_RECV_SLOT: u32 = 0xF1;
pub const CLIENT_REPLY_SEND_SLOT: u32 = 0xF2;

/// GPT partition NAME for a selector (layout authority: `crate::layout`).
pub fn part_layout_name(part: u8) -> Option<&'static str> {
    match part {
        PART_STATE => Some("state"),
        PART_DATA => Some("data"),
        PART_BSB => Some("bsb"),
        PART_BOOT_A => Some("boot-a"),
        PART_BOOT_B => Some("boot-b"),
        PART_SYSTEM_A => Some("system-a"),
        PART_SYSTEM_B => Some("system-b"),
        _ => None,
    }
}

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
/// Caller identity holds no grant for this partition/op (deny-by-default).
pub const STATUS_DENIED: u8 = 5;

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

/// Writes a request header into `buf[0..HDR_LEN]`.
fn write_req_header(buf: &mut [u8], op: u8, nonce: u32) {
    buf[0] = MAGIC0;
    buf[1] = MAGIC1;
    buf[2] = VERSION;
    buf[3] = op;
    buf[4..8].copy_from_slice(&nonce.to_le_bytes());
}

/// No-alloc READ request encoder (bump-allocator services MUST use the
/// `_into` family — per-request `Vec`s never free on the OS heap).
pub fn encode_read_into(buf: &mut [u8], nonce: u32, part: u8, lba: u64, count: u16) -> usize {
    write_req_header(buf, OP_READ, nonce);
    buf[8] = part;
    buf[9..17].copy_from_slice(&lba.to_le_bytes());
    buf[17..19].copy_from_slice(&count.to_le_bytes());
    19
}

/// No-alloc WRITE request encoder.
pub fn encode_write_into(buf: &mut [u8], nonce: u32, part: u8, lba: u64, data: &[u8]) -> usize {
    write_req_header(buf, OP_WRITE, nonce);
    buf[8] = part;
    buf[9..17].copy_from_slice(&lba.to_le_bytes());
    buf[17..17 + data.len()].copy_from_slice(data);
    17 + data.len()
}

/// No-alloc INFO request encoder.
pub fn encode_info_into(buf: &mut [u8], nonce: u32, part: u8) -> usize {
    write_req_header(buf, OP_INFO, nonce);
    buf[8] = part;
    9
}

/// No-alloc SYNC request encoder.
pub fn encode_sync_into(buf: &mut [u8], nonce: u32, part: u8) -> usize {
    write_req_header(buf, OP_SYNC, nonce);
    buf[8] = part;
    9
}

/// Writes a reply header (+status) into `buf[0..HDR_LEN + 1]`.
pub fn write_rsp_header(buf: &mut [u8], op: u8, nonce: u32, status: u8) -> usize {
    buf[0] = MAGIC0;
    buf[1] = MAGIC1;
    buf[2] = VERSION;
    buf[3] = op | 0x80;
    buf[4..8].copy_from_slice(&nonce.to_le_bytes());
    buf[8] = status;
    HDR_LEN + 1
}

/// No-alloc INFO reply encoder.
pub fn encode_info_reply_into(
    buf: &mut [u8],
    nonce: u32,
    block_size: u32,
    block_count: u64,
) -> usize {
    let base = write_rsp_header(buf, OP_INFO, nonce, STATUS_OK);
    buf[base..base + 4].copy_from_slice(&block_size.to_le_bytes());
    buf[base + 4..base + 12].copy_from_slice(&block_count.to_le_bytes());
    base + 12
}

fn req_header(op: u8, nonce: u32, cap: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(HDR_LEN + cap);
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(op);
    out.extend_from_slice(&nonce.to_le_bytes());
    out
}

/// Encodes an INFO request.
#[must_use]
pub fn encode_info(nonce: u32, part: u8) -> Vec<u8> {
    let mut out = req_header(OP_INFO, nonce, 1);
    out.push(part);
    out
}

/// Encodes a READ request.
#[must_use]
pub fn encode_read(nonce: u32, part: u8, lba: u64, count: u16) -> Vec<u8> {
    let mut out = req_header(OP_READ, nonce, 11);
    out.push(part);
    out.extend_from_slice(&lba.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out
}

/// Encodes a WRITE request (`data` must be whole sectors, bounded).
#[must_use]
pub fn encode_write(nonce: u32, part: u8, lba: u64, data: &[u8]) -> Vec<u8> {
    let mut out = req_header(OP_WRITE, nonce, 9 + data.len());
    out.push(part);
    out.extend_from_slice(&lba.to_le_bytes());
    out.extend_from_slice(data);
    out
}

/// Encodes a SYNC request.
#[must_use]
pub fn encode_sync(nonce: u32, part: u8) -> Vec<u8> {
    let mut out = req_header(OP_SYNC, nonce, 1);
    out.push(part);
    out
}

/// Decodes any request frame (server side) → `(nonce, request)`.
/// Fail-closed on malformed input.
pub fn decode_request(frame: &[u8]) -> Option<(u32, BlockRequest<'_>)> {
    if frame.len() < HDR_LEN + 1 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION
    {
        return None;
    }
    let nonce = u32::from_le_bytes(frame[4..8].try_into().ok()?);
    let body = &frame[HDR_LEN..];
    let req = match frame[3] {
        OP_INFO if body.len() == 1 => BlockRequest::Info { part: body[0] },
        OP_READ if body.len() == 11 => {
            let lba = u64::from_le_bytes(body[1..9].try_into().ok()?);
            let count = u16::from_le_bytes(body[9..11].try_into().ok()?);
            if count == 0 || count > MAX_BLOCKS_PER_REQ {
                return None;
            }
            BlockRequest::Read { part: body[0], lba, count }
        }
        OP_WRITE if body.len() > 9 => {
            let lba = u64::from_le_bytes(body[1..9].try_into().ok()?);
            let data = &body[9..];
            // `%` not `is_multiple_of`: the OS toolchain (nightly-2025-01-15)
            // predates the `unsigned_is_multiple_of` stabilization (stable 1.87).
            #[allow(unknown_lints, clippy::manual_is_multiple_of)]
            if data.is_empty()
                || data.len() % SECTOR_SIZE != 0
                || data.len() / SECTOR_SIZE > MAX_BLOCKS_PER_REQ as usize
            {
                return None;
            }
            BlockRequest::Write { part: body[0], lba, data }
        }
        OP_SYNC if body.len() == 1 => BlockRequest::Sync { part: body[0] },
        _ => return None,
    };
    Some((nonce, req))
}

fn rsp_header(op: u8, nonce: u32, status: u8, cap: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(HDR_LEN + 1 + cap);
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(op | 0x80);
    out.extend_from_slice(&nonce.to_le_bytes());
    out.push(status);
    out
}

/// Encodes a status-only reply (WRITE/SYNC, and every error).
#[must_use]
pub fn encode_status(op: u8, nonce: u32, status: u8) -> Vec<u8> {
    rsp_header(op, nonce, status, 0)
}

/// Encodes an INFO reply.
#[must_use]
pub fn encode_info_reply(nonce: u32, block_size: u32, block_count: u64) -> Vec<u8> {
    let mut out = rsp_header(OP_INFO, nonce, STATUS_OK, 12);
    out.extend_from_slice(&block_size.to_le_bytes());
    out.extend_from_slice(&block_count.to_le_bytes());
    out
}

/// Validates a reply header against `(op, nonce)` → the status + body.
/// `None` = not ours (shared-inbox stranger) or malformed — the caller
/// keeps draining.
pub fn open_reply(frame: &[u8], op: u8, nonce: u32) -> Option<(u8, &[u8])> {
    if frame.len() < HDR_LEN + 1
        || frame[0] != MAGIC0
        || frame[1] != MAGIC1
        || frame[2] != VERSION
        || frame[3] != (op | 0x80)
    {
        return None;
    }
    if u32::from_le_bytes(frame[4..8].try_into().ok()?) != nonce {
        return None;
    }
    Some((frame[8], &frame[HDR_LEN + 1..]))
}

/// Decodes an INFO reply body → `(block_size, block_count)`.
pub fn decode_info_reply(nonce: u32, frame: &[u8]) -> Option<(u32, u64)> {
    let (status, body) = open_reply(frame, OP_INFO, nonce)?;
    if status != STATUS_OK || body.len() != 12 {
        return None;
    }
    let block_size = u32::from_le_bytes(body[0..4].try_into().ok()?);
    let block_count = u64::from_le_bytes(body[4..12].try_into().ok()?);
    Some((block_size, block_count))
}

/// Encodes a READ reply (`data` = the sectors).
#[must_use]
pub fn encode_read_reply(nonce: u32, data: &[u8]) -> Vec<u8> {
    let mut out = rsp_header(OP_READ, nonce, STATUS_OK, data.len());
    out.extend_from_slice(data);
    out
}

/// Decodes a READ reply into the sector payload.
pub fn decode_read_reply(nonce: u32, frame: &[u8], expect_sectors: u16) -> Option<&[u8]> {
    let (status, body) = open_reply(frame, OP_READ, nonce)?;
    if status != STATUS_OK {
        return None;
    }
    (body.len() == expect_sectors as usize * SECTOR_SIZE).then_some(body)
}

/// Decodes a status-only reply.
pub fn decode_status(op: u8, nonce: u32, frame: &[u8]) -> Option<u8> {
    let (status, body) = open_reply(frame, op, nonce)?;
    body.is_empty().then_some(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    const N: u32 = 0xC0FF_EE01;

    #[test]
    fn request_roundtrips() {
        let read = encode_read(N, PART_DATA, 42, 3);
        assert_eq!(
            decode_request(&read),
            Some((N, BlockRequest::Read { part: PART_DATA, lba: 42, count: 3 }))
        );
        let payload = [7u8; 2 * SECTOR_SIZE];
        let write = encode_write(N, PART_STATE, 9, &payload);
        match decode_request(&write) {
            Some((nonce, BlockRequest::Write { part, lba, data })) => {
                assert_eq!((nonce, part, lba), (N, PART_STATE, 9));
                assert_eq!(data, payload);
            }
            other => panic!("bad decode {other:?}"),
        }
        assert_eq!(
            decode_request(&encode_info(N, PART_BOOT_A)),
            Some((N, BlockRequest::Info { part: PART_BOOT_A }))
        );
        assert_eq!(
            decode_request(&encode_sync(N, PART_BSB)),
            Some((N, BlockRequest::Sync { part: PART_BSB }))
        );
    }

    #[test]
    fn reply_roundtrips_and_nonce_correlation() {
        let (bs, count) = decode_info_reply(N, &encode_info_reply(N, 512, 131072)).expect("info");
        assert_eq!((bs, count), (512, 131072));
        let sectors = [0xEE; SECTOR_SIZE];
        assert_eq!(decode_read_reply(N, &encode_read_reply(N, &sectors), 1), Some(&sectors[..]));
        assert_eq!(
            decode_status(OP_SYNC, N, &encode_status(OP_SYNC, N, STATUS_IO)),
            Some(STATUS_IO)
        );
        // Shared-inbox discipline: a stranger nonce or foreign op is NOT ours.
        assert_eq!(decode_status(OP_SYNC, N + 1, &encode_status(OP_SYNC, N, STATUS_OK)), None);
        assert_eq!(decode_status(OP_WRITE, N, &encode_status(OP_SYNC, N, STATUS_OK)), None);
        // A statefs-shaped stranger frame is ignored, never misparsed.
        assert_eq!(open_reply(&[b'S', b'F', 2, 0x81, 0, 0, 0, 0, 0], OP_INFO, 0), None);
    }

    #[test]
    fn part_selectors_map_to_layout_names() {
        for part in 0..PART_COUNT {
            assert!(part_layout_name(part).is_some(), "selector {part}");
        }
        assert_eq!(part_layout_name(PART_STATE), Some("state"));
        assert_eq!(part_layout_name(PART_BOOT_B), Some("boot-b"));
        assert_eq!(part_layout_name(PART_COUNT), None);
    }

    #[test]
    fn test_reject_malformed_and_oversize() {
        assert_eq!(decode_request(&[]), None);
        assert_eq!(decode_request(&[MAGIC0, MAGIC1, VERSION, OP_READ, 0, 0, 0, 0, 1]), None);
        assert_eq!(decode_request(&encode_read(N, 0, 0, 0)), None); // zero count
        assert_eq!(decode_request(&encode_read(N, 0, 0, MAX_BLOCKS_PER_REQ + 1)), None);
        let oversize = alloc::vec![0u8; (MAX_BLOCKS_PER_REQ as usize + 1) * SECTOR_SIZE];
        assert_eq!(decode_request(&encode_write(N, 0, 0, &oversize)), None);
        let ragged = alloc::vec![0u8; SECTOR_SIZE + 1];
        assert_eq!(decode_request(&encode_write(N, 0, 0, &ragged)), None);
        // Read reply with wrong sector count fails closed.
        let sectors = [0u8; SECTOR_SIZE];
        assert_eq!(decode_read_reply(N, &encode_read_reply(N, &sectors), 2), None);
    }
}
