// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The zero-copy read data plane (RFC-0072 Phase 3, RFC-0040 VMO
//! transfer). Bulk file bytes above [`INLINE_IO_MAX`] move as a VMO handle
//! instead of streaming through IPC frames: the client creates a VMO, CAP_MOVEs
//! it to vfsd with an [`OP_READ_VMO`] request, and the provider fills it —
//! writing the payload FIRST and the [`SPLICE_MAGIC`] header LAST (release
//! ordering), so a client that sees the magic sees complete data. This is the
//! same header-last handoff execd/bundlemgrd use for payloads; the codec lives
//! here so vfsd and every client share one byte layout. Inline reads/writes
//! above the cap are `E2BIG`, never a silent slow path.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0295)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: header roundtrip + magic/bounds negatives + request codec

use alloc::string::String;
use alloc::vec::Vec;

use crate::entry::MAX_PATH_LEN;

/// Frame opcode for a VMO-splice read (byte 0 of the request). Sits between
/// `OP_READDIR = 6` and `OP_MKDIR = 8` in the shared opcode space.
pub const OP_READ_VMO: u8 = 7;

/// Bytes at or below this move inline in the IPC payload; above it the data
/// plane is a VMO handle (RFC-0071/0072). An inline read/write above the cap is
/// a protocol error (`E2BIG`), announced in RFC-0072 and enforced here.
pub const INLINE_IO_MAX: usize = 4096;

/// Marks a filled splice header. A freshly created VMO is all-zero, so the
/// magic being present is the signal that the provider finished writing (it
/// writes the payload first, then this header — release ordering).
pub const SPLICE_MAGIC: [u8; 4] = *b"NXVR";

/// Length of the splice header written at VMO offset 0.
pub const SPLICE_HEADER_LEN: usize = 16;

/// Offset within the VMO where the payload bytes begin (after the header).
pub const SPLICE_DATA_OFFSET: usize = SPLICE_HEADER_LEN;

/// Encodes the 16-byte splice header: `magic(4) | status u16 LE | rsv(2) |
/// len u32 LE | rsv(4)`. `status` is an RFC-0072 error code (0 = OK); `len` is
/// the payload byte count that follows at [`SPLICE_DATA_OFFSET`].
#[must_use]
pub fn encode_splice_header(status: u16, len: u32) -> [u8; SPLICE_HEADER_LEN] {
    let mut hdr = [0u8; SPLICE_HEADER_LEN];
    hdr[0..4].copy_from_slice(&SPLICE_MAGIC);
    hdr[4..6].copy_from_slice(&status.to_le_bytes());
    hdr[8..12].copy_from_slice(&len.to_le_bytes());
    hdr
}

/// Decodes a splice header into `(status, len)`. Returns `None` when the magic
/// is absent — i.e. the provider has not finished writing (poll again).
#[must_use]
pub fn decode_splice_header(buf: &[u8]) -> Option<(u16, u32)> {
    if buf.len() < SPLICE_HEADER_LEN || buf[0..4] != SPLICE_MAGIC {
        return None;
    }
    let status = u16::from_le_bytes([buf[4], buf[5]]);
    let len = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
    Some((status, len))
}

/// Whether a payload of `payload_len` bytes fits in a caller VMO of
/// `vmo_capacity` bytes (after reserving the [`SPLICE_DATA_OFFSET`] header).
/// A payload that does not fit is an `E2BIG` — the provider must NOT truncate.
#[must_use]
pub fn splice_fits(payload_len: usize, vmo_capacity: usize) -> bool {
    vmo_capacity
        .checked_sub(SPLICE_DATA_OFFSET)
        .is_some_and(|max_payload| payload_len <= max_payload)
}

/// Encodes an `OP_READ_VMO` request payload (the path bytes; the opcode byte is
/// prepended by the caller). The read starts at offset 0 and fills up to the
/// VMO's capacity minus the header.
#[must_use]
pub fn encode_read_vmo_request(path: &str) -> Option<Vec<u8>> {
    if path.is_empty() || path.len() > MAX_PATH_LEN {
        return None;
    }
    Some(path.as_bytes().to_vec())
}

/// Decodes an `OP_READ_VMO` request payload into the path.
#[must_use]
pub fn decode_read_vmo_request(payload: &[u8]) -> Option<String> {
    if payload.is_empty() || payload.len() > MAX_PATH_LEN {
        return None;
    }
    core::str::from_utf8(payload).ok().map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrips() {
        let hdr = encode_splice_header(0, 12345);
        assert_eq!(decode_splice_header(&hdr), Some((0, 12345)));
        let err = encode_splice_header(8, 0); // E2BIG, no payload
        assert_eq!(decode_splice_header(&err), Some((8, 0)));
    }

    #[test]
    fn unwritten_header_is_pending() {
        // A fresh VMO is all-zero: no magic → still pending.
        let zero = [0u8; SPLICE_HEADER_LEN];
        assert_eq!(decode_splice_header(&zero), None);
        // Short buffers never falsely decode.
        assert_eq!(decode_splice_header(&[b'N', b'X', b'V']), None);
    }

    #[test]
    fn request_roundtrips_and_bounds() {
        let payload = encode_read_vmo_request("pkg:/system/build.prop").expect("encode");
        assert_eq!(
            decode_read_vmo_request(&payload).as_deref(),
            Some("pkg:/system/build.prop")
        );
        assert!(encode_read_vmo_request("").is_none());
        let long = "x".repeat(MAX_PATH_LEN + 1);
        assert!(encode_read_vmo_request(&long).is_none());
    }

    #[test]
    fn inline_cap_is_the_shared_constant() {
        assert_eq!(INLINE_IO_MAX, crate::fileops::MAX_INLINE_TEXT);
    }

    /// Simulates the provider fill (payload-first, header-last) and the consumer
    /// read: the bytes the consumer extracts must equal the original — the
    /// byte-equality contract vfsd and the client implement over a real VMO.
    #[test]
    fn fill_then_read_is_byte_identical() {
        let payload = b"ro.nexus.build=dev\nro.nexus.channel=stable\n";
        let cap = 4096usize;
        let mut vmo = alloc::vec![0u8; cap];
        assert!(splice_fits(payload.len(), cap));
        // Payload FIRST at the data offset.
        vmo[SPLICE_DATA_OFFSET..SPLICE_DATA_OFFSET + payload.len()].copy_from_slice(payload);
        // A consumer polling now sees no magic → still pending.
        assert_eq!(decode_splice_header(&vmo[..SPLICE_HEADER_LEN]), None);
        // Header LAST (release).
        let hdr = encode_splice_header(crate::CODE_OK, payload.len() as u32);
        vmo[..SPLICE_HEADER_LEN].copy_from_slice(&hdr);
        // Consumer read-back.
        let (status, len) = decode_splice_header(&vmo[..SPLICE_HEADER_LEN]).expect("ready");
        assert_eq!(status, crate::CODE_OK);
        let got = &vmo[SPLICE_DATA_OFFSET..SPLICE_DATA_OFFSET + len as usize];
        assert_eq!(got, payload, "spliced bytes match the source");
    }

    #[test]
    fn oversize_payload_is_e2big_not_truncated() {
        let cap = 64usize; // 16-byte header leaves 48 bytes of payload room
        assert!(splice_fits(48, cap));
        assert!(!splice_fits(49, cap));
        // The provider signals E2BIG with a zero-length payload, never a partial.
        let hdr = encode_splice_header(crate::VfsError::TooBig.code(), 0);
        assert_eq!(
            decode_splice_header(&hdr),
            Some((crate::VfsError::TooBig.code(), 0))
        );
    }

    #[test]
    fn splice_fits_rejects_capacity_below_header() {
        assert!(!splice_fits(0, SPLICE_HEADER_LEN - 1));
        assert!(splice_fits(0, SPLICE_HEADER_LEN));
    }
}
