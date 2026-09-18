// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The zero-copy read data plane (RFC-0072 Phase 3, RFC-0040 VMO
//! transfer). Bulk file bytes above [`INLINE_IO_MAX`] move as a VMO handle
//! instead of streaming through IPC frames: the client creates a VMO, CAP_MOVEs
//! it to vfsd with an [`OP_READ_VMO`] request, and the provider fills it —
//! writing the payload FIRST and the header LAST (release ordering), so a
//! client that sees the magic sees complete data. Inline reads/writes above the
//! cap are `E2BIG`, never a silent slow path.
//!
//! The header itself is NOT declared here any more. It is the one payload-VMO
//! header (`nexus_wire::payload_vmo`, RFC-0097) that bundlemgrd's payload ops
//! use as well — the same 16 bytes served two protocols under two magics until
//! TASK-0033 P1. The names below are re-exports, so every VFS client keeps its
//! `nexus_vfs_types::` paths.
//!
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0295, unified TASK-0033)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: request codec bounds + the provider-fill/consumer-read contract
//!   (the header codec's own tests live with the codec)
//! RFC: docs/rfcs/RFC-0097-payload-vmo-header-v2-pkg-passthrough.md

use alloc::string::String;
use alloc::vec::Vec;

use crate::entry::MAX_PATH_LEN;

/// THE payload-VMO header codec (RFC-0097). Re-exported under the historical
/// splice names so the VFS surface reads the way RFC-0072 describes it.
pub use nexus_wire::payload_vmo::{
    decode_header as decode_splice_header, encode_header as encode_splice_header,
    fits as splice_fits, DATA_OFFSET as SPLICE_DATA_OFFSET, HEADER_LEN as SPLICE_HEADER_LEN,
    MAGIC as SPLICE_MAGIC,
};

/// Frame opcode for a VMO-splice read (byte 0 of the request). Sits between
/// `OP_READDIR = 6` and `OP_MKDIR = 8` in the shared opcode space.
pub const OP_READ_VMO: u8 = 7;

/// Bytes at or below this move inline in the IPC payload; above it the data
/// plane is a VMO handle (RFC-0071/0072). An inline read/write above the cap is
/// a protocol error (`E2BIG`), announced in RFC-0072 and enforced here.
pub const INLINE_IO_MAX: usize = 4096;

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
    fn request_roundtrips_and_bounds() {
        let payload = encode_read_vmo_request("pkg:/system/build.prop").expect("encode");
        assert_eq!(decode_read_vmo_request(&payload).as_deref(), Some("pkg:/system/build.prop"));
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
        assert_eq!(decode_splice_header(&hdr), Some((crate::VfsError::TooBig.code(), 0)));
    }
}
