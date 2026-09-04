// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Bundle manager (bundlemgrd) service frames used for OS bring-up.
//!
//! This is intentionally minimal and byte-oriented (no IDL) to keep early boot
//! deterministic.

/// First magic byte (`'B'`).
pub const MAGIC0: u8 = b'B';
/// Second magic byte (`'N'`).
pub const MAGIC1: u8 = b'N';
/// Protocol version.
pub const VERSION: u8 = 1;

/// List installed bundles (bring-up only).
pub const OP_LIST: u8 = 1;
/// Probe routing status of a target (bring-up only; used for policyd-gated denial proofs).
pub const OP_ROUTE_STATUS: u8 = 2;
/// Fetch a read-only bundle image containing one or more entries.
pub const OP_FETCH_IMAGE: u8 = 3;
/// Set the active slot for publication (`a` or `b`).
pub const OP_SET_ACTIVE_SLOT: u8 = 4;
/// List installed apps for the launcher / Apps menu (RFC-0065 dynamic apps menu).
pub const OP_LIST_APPS: u8 = 5;

/// Operation succeeded.
pub const STATUS_OK: u8 = 0;
/// Request frame was malformed.
pub const STATUS_MALFORMED: u8 = 1;
/// Operation is not supported by this build.
pub const STATUS_UNSUPPORTED: u8 = 2;

/// Byte offset where LIST_APPS response entries begin (after status + count).
pub const LIST_APPS_BODY_OFFSET: usize = 7;

/// Encodes a LIST request.
pub fn encode_list(out: &mut [u8; 4]) {
    *out = [MAGIC0, MAGIC1, VERSION, OP_LIST];
}

/// Encodes a FETCH_IMAGE request.
pub fn encode_fetch_image(out: &mut [u8; 4]) {
    *out = [MAGIC0, MAGIC1, VERSION, OP_FETCH_IMAGE];
}

/// Encodes a LIST_APPS request: `[B, N, ver, OP_LIST_APPS]`.
pub fn encode_list_apps(out: &mut [u8; 4]) {
    *out = [MAGIC0, MAGIC1, VERSION, OP_LIST_APPS];
}

/// Encodes a SET_ACTIVE_SLOT request.
///
/// Frame: `[B, N, ver, OP_SET_ACTIVE_SLOT, slot:u8]`
pub fn encode_set_active_slot_req(slot: u8, out: &mut [u8; 5]) {
    *out = [MAGIC0, MAGIC1, VERSION, OP_SET_ACTIVE_SLOT, slot];
}

/// Decodes the request opcode from a bundlemgrd v1 request frame.
pub fn decode_request_op(frame: &[u8]) -> Option<u8> {
    crate::codec::request_op(frame, MAGIC0, MAGIC1, VERSION)
}

/// Decodes the LIST_APPS response header → `(status, count)`.
///
/// Response frame:
/// `[B, N, ver, OP_LIST_APPS|0x80, status:u8, count:u16le, entries...]`
/// where each entry is `[id_len:u8, id..., label_len:u8, label...]` (UTF-8).
/// Entry parsing (which needs allocation) lives in the consumer — trailing
/// entry bytes are deliberately NOT length-checked here.
pub fn decode_list_apps_header(frame: &[u8]) -> Option<(u8, u16)> {
    let mut r = crate::codec::Reader::new(frame);
    crate::codec::check_hdr(&mut r, MAGIC0, MAGIC1, VERSION, OP_LIST_APPS | 0x80)?;
    let status = r.take_u8()?;
    let count = r.take_u16le()?;
    Some((status, count))
}

crate::frames! {
    protocol(magic0 = MAGIC0, magic1 = MAGIC1, version = VERSION);

    /// LIST response → `(status, count)` (one reserved trailing byte).
    reply decode decode_list_rsp (op = OP_LIST) {
        status: u8,
        count: u16le,
        _r: pad(1),
    }
    /// FETCH_IMAGE response → `(status, image_bytes)`:
    /// `[B,N,ver,op|0x80,status,len:u32le, payload...]`.
    reply decode decode_fetch_image_rsp (op = OP_FETCH_IMAGE) {
        status: u8,
        image: bytes32(min = 0, max = u32::MAX as usize),
    }
    /// SET_ACTIVE_SLOT response → `(status, slot)` (two reserved trailing bytes).
    reply decode decode_set_active_slot_rsp (op = OP_SET_ACTIVE_SLOT) {
        status: u8,
        slot: u8,
        _r: pad(2),
    }
    /// GET_PAYLOAD request: `[B, N, ver, OP_GET_PAYLOAD, id_len:u8, id...]`.
    request encode_get_payload / decode_get_payload (op = OP_GET_PAYLOAD) {
        app_id: bytes8(min = 1, max = 48),
    }
}

/// Fetch an app's UI-program payload into a caller-provided VMO
/// (TASK-0080D GET_PAYLOAD). Request:
/// `[B, N, ver, OP_GET_PAYLOAD, id_len:u8, id...]` with the payload VMO
/// capability MOVED alongside the message (CAP_MOVE — the gpud-attach /
/// ADR-0042 SURFACE_CREATE pattern; the message's single cap slot carries
/// the VMO, so there is no reply frame). bundlemgrd writes the payload
/// bytes at [`PAYLOAD_DATA_OFFSET`], then the header LAST (header-last =
/// release ordering for the single writer); the consumer polls the header.
pub const OP_GET_PAYLOAD: u8 = 6;

/// Payload-VMO header magic (`"NXPL"`), written after the payload bytes.
pub const PAYLOAD_MAGIC: [u8; 4] = *b"NXPL";
/// Header length; the payload bytes start here (8-byte aligned for the
/// canonical `.nxir` capnp contract).
pub const PAYLOAD_DATA_OFFSET: usize = 16;
/// Header status: payload written completely.
pub const PAYLOAD_STATUS_OK: u8 = 1;
/// Header status: the app id has no ui-program payload.
pub const PAYLOAD_STATUS_UNKNOWN: u8 = 2;
/// Header status: the payload exceeds the provided VMO.
pub const PAYLOAD_STATUS_TOO_LARGE: u8 = 3;

/// Encodes the 16-byte payload-VMO header (`magic, status, len:u32le`).
///
/// Not a request/reply frame (no magic0/magic1/version/op prefix) — this is
/// the shared-memory poll header the GET_PAYLOAD contract writes last.
pub fn encode_payload_header(status: u8, len: u32) -> [u8; PAYLOAD_DATA_OFFSET] {
    let mut hdr = [0u8; PAYLOAD_DATA_OFFSET];
    hdr[..4].copy_from_slice(&PAYLOAD_MAGIC);
    hdr[4] = status;
    hdr[8..12].copy_from_slice(&len.to_le_bytes());
    hdr
}

/// Decodes a payload-VMO header → `(status, len)`; `None` while the
/// header has not been written yet (or is not a payload header).
pub fn decode_payload_header(hdr: &[u8]) -> Option<(u8, u32)> {
    if hdr.len() < PAYLOAD_DATA_OFFSET || hdr[..4] != PAYLOAD_MAGIC {
        return None;
    }
    Some((hdr[4], u32::from_le_bytes([hdr[8], hdr[9], hdr[10], hdr[11]])))
}

// ---------------------------------------------------------------------------
// TASK-0321 P2 (RFC-0089 §12.3, ADR-0060): the verified SYSTEM VOLUME.
// bundlemgrd verifies the NXSV + index of the system slot paired with the
// measured boot slot and serves bundle ELFs out of it to init (the sole
// spawner) over the same header-last VMO discipline as GET_PAYLOAD.
// ---------------------------------------------------------------------------

/// QUERY_BUNDLE request `[B, N, ver, OP_QUERY_BUNDLE, name_len:u8, name…]`
/// → reply `[…|0x80, status, size:u32le, stack_pages:u32le, gp:u64le,
/// sha8_len:u8, sha8[..8], ver_len:u8, version…]` — the bundle's
/// `payload.elf` size, the launch params from the bundle table, the first
/// 8 bytes of its window digest and its version string.
pub const OP_QUERY_BUNDLE: u8 = 7;
/// GET_BUNDLE_ELF request `[B, N, ver, OP_GET_BUNDLE_ELF, name_len:u8,
/// name…]` with the destination VMO capability MOVED alongside; bundlemgrd
/// writes the bundle's `payload.elf` bytes at [`PAYLOAD_DATA_OFFSET`] after
/// verifying their index digest, then the payload header LAST.
pub const OP_GET_BUNDLE_ELF: u8 = 8;
/// VOLUME_STATUS request `[B, N, ver, OP_VOLUME_STATUS]` → reply
/// `[…|0x80, status, slot:u8, verified:u8, bundles:u16le, build8_len:u8, build8[..8]]`.
pub const OP_VOLUME_STATUS: u8 = 9;

/// Volume-op status: the bundle is not in the (verified) index.
pub const STATUS_NOT_FOUND: u8 = 4;
/// Volume-op status: the system volume is not verified (absent, unpaired,
/// signature/digest failure, block plane down) — deterministic, never a
/// half answer.
pub const STATUS_UNAVAILABLE: u8 = 5;
/// Header status (GET_BUNDLE_ELF): the payload bytes did not hash to the
/// index digest — nothing usable was written.
pub const PAYLOAD_STATUS_DIGEST: u8 = 4;
/// TASK-0321 P5: GET_INDEX request `[B, N, ver, OP_GET_INDEX]` with the
/// destination VMO MOVED alongside; bundlemgrd writes the NXSV-verified
/// volume index bytes (superblock + index, ≤ 256 KiB) at
/// [`PAYLOAD_DATA_OFFSET`], then the payload header LAST. packagefsd builds
/// its `pkg:/` view from exactly these bytes (one bundle authority).
pub const OP_GET_INDEX: u8 = 10;
/// TASK-0321 P5: GET_FILE_VMO request `[B, N, ver, OP_GET_FILE_VMO,
/// bundle_len:u8, bundle…, path_len:u8, path…]` with the destination VMO
/// MOVED alongside; bundlemgrd streams the entry's bytes from the volume
/// hashed against its index digest, header LAST (`PAYLOAD_STATUS_DIGEST`
/// on mismatch — never an OK header over unverified bytes).
pub const OP_GET_FILE_VMO: u8 = 11;

crate::frames! {
    protocol(magic0 = MAGIC0, magic1 = MAGIC1, version = VERSION);

    /// QUERY_BUNDLE request.
    request encode_query_bundle / decode_query_bundle (op = OP_QUERY_BUNDLE) {
        name: bytes8(min = 1, max = 48),
    }
    /// QUERY_BUNDLE reply → `(status, size, stack_pages, global_pointer, sha8, version)`.
    reply encode_query_bundle_rsp / decode_query_bundle_rsp (op = OP_QUERY_BUNDLE) {
        status: u8,
        size: u32le,
        stack_pages: u32le,
        global_pointer: u64le,
        sha8: bytes8(min = 0, max = 8),
        version: bytes8(min = 0, max = 32),
    }
    /// GET_BUNDLE_ELF request (VMO moved alongside).
    request encode_get_bundle_elf / decode_get_bundle_elf (op = OP_GET_BUNDLE_ELF) {
        name: bytes8(min = 1, max = 48),
    }
    /// VOLUME_STATUS request.
    request encode_volume_status / decode_volume_status (op = OP_VOLUME_STATUS) {}
    /// GET_INDEX request (VMO moved alongside).
    request encode_get_index / decode_get_index (op = OP_GET_INDEX) {}
    /// GET_FILE_VMO request (VMO moved alongside) → `(bundle, path)`.
    request encode_get_file_vmo / decode_get_file_vmo (op = OP_GET_FILE_VMO) {
        bundle: bytes8(min = 1, max = 48),
        path: bytes8(min = 1, max = 96),
    }
    /// VOLUME_STATUS reply → `(status, slot, verified, bundles, build8)`.
    reply encode_volume_status_rsp / decode_volume_status_rsp (op = OP_VOLUME_STATUS) {
        status: u8,
        slot: u8,
        verified: u8,
        bundles: u16le,
        build8: bytes8(min = 0, max = 8),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_ops_round_trip() {
        // TASK-0321: QUERY_BUNDLE reply carries launch params + sha8 + version.
        let mut buf = [0u8; 96];
        let n =
            encode_query_bundle_rsp(STATUS_OK, 81_760, 1, 0x22368, &[0xd0; 8], b"1.0.0", &mut buf)
                .expect("encode");
        let (status, size, stack_pages, gp, sha8, version) =
            decode_query_bundle_rsp(&buf[..n]).expect("decode");
        assert_eq!((status, size, stack_pages, gp), (STATUS_OK, 81_760, 1, 0x22368));
        assert_eq!(sha8, &[0xd0; 8]);
        assert_eq!(version, b"1.0.0");
        let mut req = [0u8; 64];
        let n = encode_get_bundle_elf(b"metricsd", &mut req).expect("encode req");
        assert_eq!(decode_request_op(&req[..n]), Some(OP_GET_BUNDLE_ELF));
        assert_eq!(decode_get_bundle_elf(&req[..n]), Some(&b"metricsd"[..]));
        let n = encode_volume_status_rsp(STATUS_OK, b'a', 1, 3, b"dev-6569", &mut buf).expect("st");
        assert_eq!(
            decode_volume_status_rsp(&buf[..n]),
            Some((STATUS_OK, b'a', 1, 3, &b"dev-6569"[..]))
        );
        // A truncated reply never decodes half a record.
        assert!(decode_volume_status_rsp(&buf[..n - 3]).is_none());
    }

    #[test]
    fn file_ops_round_trip() {
        // TASK-0321 P5: GET_INDEX carries no body; GET_FILE_VMO names the
        // bundle + entry path; a truncated frame never half-decodes.
        let mut req = [0u8; 4];
        let n = encode_get_index(&mut req).expect("encode");
        assert_eq!(decode_request_op(&req[..n]), Some(OP_GET_INDEX));
        let mut req = [0u8; 160];
        let n = encode_get_file_vmo(b"calculator", b"payload.elf", &mut req).expect("encode");
        assert_eq!(decode_get_file_vmo(&req[..n]), Some((&b"calculator"[..], &b"payload.elf"[..])));
        assert!(decode_get_file_vmo(&req[..n - 1]).is_none());
        assert!(encode_get_file_vmo(b"", b"x", &mut req).is_none());
    }

    #[test]
    fn get_payload_round_trip() {
        let mut buf = [0u8; 64];
        let n = encode_get_payload(b"counter", &mut buf).unwrap();
        assert_eq!(
            &buf[..n],
            &[b'B', b'N', 1, OP_GET_PAYLOAD, 7, b'c', b'o', b'u', b'n', b't', b'e', b'r']
        );
        assert_eq!(decode_get_payload(&buf[..n]).unwrap(), b"counter");
        // Truncated / empty ids rejected.
        assert!(decode_get_payload(&buf[..n - 1]).is_none());
        assert!(encode_get_payload(b"", &mut buf).is_none());
    }

    #[test]
    fn payload_header_round_trip() {
        let hdr = encode_payload_header(PAYLOAD_STATUS_OK, 4096);
        assert_eq!(decode_payload_header(&hdr), Some((PAYLOAD_STATUS_OK, 4096)));
        // An unwritten (zeroed) header decodes to None — the poll contract.
        assert_eq!(decode_payload_header(&[0u8; PAYLOAD_DATA_OFFSET]), None);
    }

    #[test]
    fn list_req_golden() {
        let mut req = [0u8; 4];
        encode_list(&mut req);
        const GOLDEN: [u8; 4] = [b'B', b'N', 1, 1];
        assert_eq!(req, GOLDEN);
        assert_eq!(decode_request_op(&req).unwrap(), OP_LIST);
    }

    #[test]
    fn fetch_image_req_golden() {
        let mut req = [0u8; 4];
        encode_fetch_image(&mut req);
        const GOLDEN: [u8; 4] = [b'B', b'N', 1, 3];
        assert_eq!(req, GOLDEN);
        assert_eq!(decode_request_op(&req).unwrap(), OP_FETCH_IMAGE);
    }

    #[test]
    fn list_apps_req_and_header_golden() {
        let mut req = [0u8; 4];
        encode_list_apps(&mut req);
        assert_eq!(req, [b'B', b'N', 1, OP_LIST_APPS]);
        assert_eq!(decode_request_op(&req).unwrap(), OP_LIST_APPS);

        // A response header for 2 apps decodes to (OK, 2).
        let rsp = [b'B', b'N', 1, OP_LIST_APPS | 0x80, STATUS_OK, 2, 0];
        assert_eq!(decode_list_apps_header(&rsp), Some((STATUS_OK, 2)));
        // Wrong opcode rejected.
        let bad = [b'B', b'N', 1, OP_LIST | 0x80, STATUS_OK, 2, 0];
        assert_eq!(decode_list_apps_header(&bad), None);
    }

    #[test]
    fn set_active_slot_req_golden() {
        let mut req = [0u8; 5];
        encode_set_active_slot_req(1, &mut req);
        const GOLDEN: [u8; 5] = [b'B', b'N', 1, 4, 1];
        assert_eq!(req, GOLDEN);
        assert_eq!(decode_request_op(&req).unwrap(), OP_SET_ACTIVE_SLOT);
    }

    #[test]
    fn fixed_rsp_decoders_ignore_reserved_bytes() {
        let rsp = [b'B', b'N', 1, OP_LIST | 0x80, STATUS_OK, 7, 0, 0xEE];
        assert_eq!(decode_list_rsp(&rsp), Some((STATUS_OK, 7)));
        assert_eq!(decode_list_rsp(&rsp[..7]), None);
        let rsp = [b'B', b'N', 1, OP_SET_ACTIVE_SLOT | 0x80, STATUS_OK, 1, 0xAA, 0xBB];
        assert_eq!(decode_set_active_slot_rsp(&rsp), Some((STATUS_OK, 1)));
    }

    #[test]
    fn fetch_image_rsp_roundtrip() {
        let rsp = [b'B', b'N', 1, OP_FETCH_IMAGE | 0x80, STATUS_OK, 2, 0, 0, 0, b'h', b'i'];
        assert_eq!(decode_fetch_image_rsp(&rsp), Some((STATUS_OK, &b"hi"[..])));
        // Length mismatch rejected.
        assert_eq!(decode_fetch_image_rsp(&rsp[..rsp.len() - 1]), None);
    }
}
