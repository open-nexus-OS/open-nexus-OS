// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: THE payload-VMO header (RFC-0097). Bulk bytes never travel in an
//! IPC frame — they are written into a caller-provided VMO and released with
//! this 16-byte header, written LAST. A freshly created VMO is all-zero, so the
//! magic being present IS the release fence: a reader that sees it sees
//! complete data.
//!
//! WHY THIS FILE EXISTS: the same idea was declared twice — `NXVR` for the VFS
//! splice read (`OP_READ_VMO`, RFC-0072 Phase 3) and `NXPL` for bundlemgrd's
//! payload ops (`GET_PAYLOAD` / `GET_BUNDLE_ELF` / `GET_INDEX` /
//! `GET_FILE_VMO`). Both were 16 bytes with the magic at `[0..4]` and the
//! length at `[8..12]`; they differed only in the magic and in what a status
//! number meant. `NXPL`'s private `u8` space is the reason this is not merely
//! untidy: its values collided with bundlemgrd's generic reply statuses in the
//! same module (`STATUS_MALFORMED == PAYLOAD_STATUS_OK == 1`), so a malformed
//! request wrote a header that read as SUCCESS. One table, one meaning per
//! value (see [`crate::status`]) makes that unrepresentable.
//!
//! TWO RULES THE ZERO-VALUED OK MAKES EXPLICIT:
//!
//! 1. The header is written in ONE 16-byte write. [`encode_header`] returns the
//!    whole array for exactly that reason — magic and status must never become
//!    visible separately, because `CODE_OK` is `0` and a half-written header
//!    with a visible magic would read as success.
//! 2. A REUSED VMO is zeroed with [`ZEROED_HEADER`] before it is armed again,
//!    or a stale OK from the previous operation is a valid completion signal
//!    for this one.
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable byte layout (golden-locked below)
//! TEST_COVERAGE: roundtrip, magic/bounds negatives, capacity negatives
//! RFC: docs/rfcs/RFC-0097-payload-vmo-header-v2-pkg-passthrough.md

/// Marks a filled payload-VMO header — and nothing else does. The writer puts
/// the payload down first and this header last, so a reader that finds the
/// magic is looking at complete bytes.
pub const MAGIC: [u8; 4] = *b"NXVR";

/// Header length. Also 8-byte aligned, which the canonical `.nxir` capnp
/// contract requires of the payload that follows it.
pub const HEADER_LEN: usize = 16;

/// Offset within the VMO where the payload bytes begin.
pub const DATA_OFFSET: usize = HEADER_LEN;

/// The all-zero header. Write this over a REUSED VMO's header before arming it
/// again: without it, the previous operation's `OK` is still a valid release
/// signal for the next one.
pub const ZEROED_HEADER: [u8; HEADER_LEN] = [0u8; HEADER_LEN];

/// Encodes the header: `magic(4) | status u16 LE | rsv(2) | len u32 LE | rsv(4)`.
///
/// `status` is a [`crate::status`] code (`0` = OK); `len` is the payload byte
/// count that follows at [`DATA_OFFSET`]. The whole array is returned because
/// it must reach the VMO in a single write — see the module note.
#[must_use]
pub fn encode_header(status: u16, len: u32) -> [u8; HEADER_LEN] {
    let mut hdr = [0u8; HEADER_LEN];
    hdr[0..4].copy_from_slice(&MAGIC);
    hdr[4..6].copy_from_slice(&status.to_le_bytes());
    hdr[8..12].copy_from_slice(&len.to_le_bytes());
    hdr
}

/// Decodes a header into `(status, len)`. `None` when the magic is absent —
/// i.e. the writer has not finished, which is NOT an error and NOT a success.
#[must_use]
pub fn decode_header(buf: &[u8]) -> Option<(u16, u32)> {
    if buf.len() < HEADER_LEN || buf[0..4] != MAGIC {
        return None;
    }
    let status = u16::from_le_bytes([buf[4], buf[5]]);
    let len = u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
    Some((status, len))
}

/// Whether a payload of `payload_len` bytes fits a VMO of `vmo_capacity` bytes
/// once the header is reserved. A payload that does not fit is refused with
/// `TooBig` BEFORE any byte is written — a writer must never truncate.
#[must_use]
pub fn fits(payload_len: usize, vmo_capacity: usize) -> bool {
    vmo_capacity.checked_sub(DATA_OFFSET).is_some_and(|max_payload| payload_len <= max_payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::{VfsError, CODE_OK};

    #[test]
    fn golden_bytes() {
        // The byte layout is a contract: magic, status LE at 4, len LE at 8.
        let hdr = encode_header(CODE_OK, 0x0001_0203);
        assert_eq!(&hdr[0..4], b"NXVR");
        assert_eq!(hdr[4..8], [0, 0, 0, 0]);
        assert_eq!(hdr[8..12], [0x03, 0x02, 0x01, 0x00]);
        assert_eq!(hdr[12..16], [0, 0, 0, 0]);
    }

    #[test]
    fn roundtrip_ok_and_error() {
        assert_eq!(decode_header(&encode_header(CODE_OK, 4096)), Some((CODE_OK, 4096)));
        let integrity = VfsError::Integrity.code();
        assert_eq!(decode_header(&encode_header(integrity, 0)), Some((integrity, 0)));
    }

    #[test]
    fn test_reject_bad_magic() {
        let mut hdr = encode_header(CODE_OK, 16);
        hdr[0] = b'X';
        assert_eq!(decode_header(&hdr), None);
        // The retired NXPL magic must not decode either — a stale writer is a
        // protocol error, never a silently accepted header.
        let mut stale = encode_header(CODE_OK, 16);
        stale[0..4].copy_from_slice(b"NXPL");
        assert_eq!(decode_header(&stale), None);
    }

    #[test]
    fn test_reject_short_header() {
        let hdr = encode_header(CODE_OK, 16);
        for n in 0..HEADER_LEN {
            assert_eq!(decode_header(&hdr[..n]), None, "len {n}");
        }
    }

    #[test]
    fn test_reject_unwritten_header() {
        // A fresh VMO reads as all-zero. That is "not yet written", and it must
        // never be mistaken for the zero-valued OK status.
        assert_eq!(decode_header(&ZEROED_HEADER), None);
    }

    #[test]
    fn test_reject_oversize_for_vmo() {
        assert!(fits(0, HEADER_LEN));
        assert!(fits(4096, HEADER_LEN + 4096));
        assert!(!fits(4097, HEADER_LEN + 4096));
        // A VMO too small to even hold the header fits nothing.
        assert!(!fits(0, HEADER_LEN - 1));
    }

    #[test]
    fn test_reject_status_collision_is_unrepresentable() {
        // The defect this codec replaces: NXPL's PAYLOAD_STATUS_OK was 1 and
        // bundlemgrd's STATUS_MALFORMED was also 1, in the same module, both
        // written into the same header byte. In the one table, OK is 0 and
        // every error is non-zero — no error can read as success.
        assert_eq!(CODE_OK, 0);
        for err in [VfsError::Invalid, VfsError::NotFound, VfsError::TooBig, VfsError::Integrity] {
            assert_ne!(err.code(), CODE_OK, "{err} must not read as OK");
            let (status, len) = decode_header(&encode_header(err.code(), 0)).expect("decodes");
            assert_ne!(status, CODE_OK);
            assert_eq!(len, 0);
        }
    }
}
