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

#[cfg(test)]
mod tests {
    use super::*;

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
