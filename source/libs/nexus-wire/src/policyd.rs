// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Policyd service frames (v1/v2/v3) shared between init-lite, policyd, and
//! clients. v1 is the legacy bring-up wire (no correlation), v2 adds
//! nonce-correlated requests/responses (RFC-0019), v3 switches
//! requester/target to stable u64 service ids.

/// First magic byte (`'P'`) — private: only touched via the codecs.
const MAGIC0: u8 = b'P';
/// Second magic byte (`'O'`).
const MAGIC1: u8 = b'O';

/// Policyd protocol version 1 (legacy bring-up, no correlation).
pub const VERSION_V1: u8 = 1;
/// Policyd protocol version 2 (nonce-correlated requests/responses).
pub const VERSION_V2: u8 = 2;
/// Policyd protocol version 3 (nonce-correlated, ID-based requester/target).
pub const VERSION_V3: u8 = 3;

/// Policy check opcode (bring-up).
pub const OP_CHECK: u8 = 1;
/// Route authorization check opcode.
pub const OP_ROUTE: u8 = 2;
/// Exec authorization check opcode.
pub const OP_EXEC: u8 = 3;
/// Capability check opcode (bring-up, service-id bound).
pub const OP_CHECK_CAP: u8 = 4;
/// ABI syscall profile fetch opcode (nonce-correlated, v2).
pub const OP_ABI_PROFILE_GET: u8 = 6;

/// Status: allowed.
pub const STATUS_ALLOW: u8 = 0;
/// Status: denied.
pub const STATUS_DENY: u8 = 1;
/// Status: malformed.
pub const STATUS_MALFORMED: u8 = 2;
/// Status: unsupported op/version.
pub const STATUS_UNSUPPORTED: u8 = 3;

/// Maximum encoded ABI-profile bytes carried in an ABI_PROFILE_GET response —
/// the wire bound (`nexus-abi`'s `abi_filter` decoder re-sources this value).
pub const MAX_PROFILE_BYTES: usize = 512;

/// Nonce used to correlate requests and responses (v2).
pub type Nonce = u32;

crate::frames! {
    protocol(magic0 = MAGIC0, magic1 = MAGIC1, version = VERSION_V2);

    /// v2 ROUTE request:
    /// `[P,O,ver=2,OP_ROUTE, nonce:u32le, req_len:u8, req..., tgt_len:u8, tgt...]`.
    request encode_route_v2 / decode_route_v2 (op = OP_ROUTE) {
        nonce: u32le,
        requester: bytes8(min = 1, max = 48),
        target: bytes8(min = 1, max = 48),
    }
    /// v2 EXEC request:
    /// `[P,O,ver=2,OP_EXEC, nonce:u32le, req_len:u8, req..., image_id:u8]`.
    request encode_exec_v2 / decode_exec_v2 (op = OP_EXEC) {
        nonce: u32le,
        requester: bytes8(min = 1, max = 48),
        image_id: u8,
    }
    /// v3 ROUTE request:
    /// `[P,O,ver=3,OP_ROUTE, nonce:u32le, requester_id:u64le, target_id:u64le]`.
    request encode_route_v3_id / decode_route_v3_id (op = OP_ROUTE, version = VERSION_V3) {
        nonce: u32le,
        requester_id: u64le,
        target_id: u64le,
    }
    /// v3 EXEC request:
    /// `[P,O,ver=3,OP_EXEC, nonce:u32le, requester_id:u64le, image_id:u8]`.
    request encode_exec_v3_id / decode_exec_v3_id (op = OP_EXEC, version = VERSION_V3) {
        nonce: u32le,
        requester_id: u64le,
        image_id: u8,
    }
    /// v2 ABI profile fetch request:
    /// `[P,O,ver=2,OP_ABI_PROFILE_GET, nonce:u32le, subject_id:u64le]`.
    #[must_use = "encoded/decoded profile requests must be checked before use"]
    request encode_abi_profile_get_v2 / decode_abi_profile_get_v2 (op = OP_ABI_PROFILE_GET) {
        nonce: u32le,
        subject_id: u64le,
    }
    /// v2 ABI profile fetch response:
    /// `[P,O,ver=2,OP_ABI_PROFILE_GET|0x80,nonce:u32le,status:u8,_reserved:u8,profile_len:u16le,profile...]`.
    #[must_use = "encoded/decoded profile responses must be checked before use"]
    reply encode_abi_profile_rsp_v2 / decode_abi_profile_rsp_v2 (op = OP_ABI_PROFILE_GET) {
        nonce: u32le,
        status: u8,
        _reserved: pad(1),
        profile: bytes16(min = 0, max = MAX_PROFILE_BYTES),
    }
    /// v2 response: `[P,O,ver=2,op|0x80, nonce:u32le, status:u8, _reserved:u8]`.
    reply fixed encode encode_rsp_v2 (op = caller) {
        nonce: u32le,
        status: u8,
        _reserved: pad(1),
    }
    /// v3 response: `[P,O,ver=3,op|0x80, nonce:u32le, status:u8, _reserved:u8]`.
    reply fixed encode encode_rsp_v3 (op = caller, version = VERSION_V3) {
        nonce: u32le,
        status: u8,
        _reserved: pad(1),
    }
}

/// Decodes a v2/v3 response and returns (ver, op, nonce, status).
pub fn decode_rsp_v2_or_v3(frame: &[u8]) -> Option<(u8, u8, Nonce, u8)> {
    let mut r = crate::codec::Reader::new(frame);
    r.expect_u8(MAGIC0)?;
    r.expect_u8(MAGIC1)?;
    let ver = r.take_u8()?;
    if ver != VERSION_V2 && ver != VERSION_V3 {
        return None;
    }
    let op_byte = r.take_u8()?;
    if (op_byte & 0x80) == 0 {
        return None;
    }
    let nonce = r.take_u32le()?;
    let status = r.take_u8()?;
    r.skip(1)?;
    r.finish_exact()?;
    Some((ver, op_byte & !0x80, nonce, status))
}

/// Decodes a v2 response and returns (op, nonce, status).
pub fn decode_rsp_v2(frame: &[u8]) -> Option<(u8, Nonce, u8)> {
    match decode_rsp_v2_or_v3(frame)? {
        (VERSION_V2, op, nonce, status) => Some((op, nonce, status)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_v3_id_golden() {
        let mut buf = [0u8; 32];
        let n =
            encode_route_v3_id(0x11223344, 0x0102_0304_0506_0708, 0xA0A1_A2A3_A4A5_A6A7, &mut buf)
                .unwrap();
        const GOLDEN: [u8; 24] = [
            b'P', b'O', 3, 2, // magic + ver + OP_ROUTE
            0x44, 0x33, 0x22, 0x11, // nonce LE
            0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01, // requester_id LE
            0xA7, 0xA6, 0xA5, 0xA4, 0xA3, 0xA2, 0xA1, 0xA0, // target_id LE
        ];
        assert_eq!(&buf[..n], &GOLDEN);
        let (nonce, req, tgt) = decode_route_v3_id(&buf[..n]).unwrap();
        assert_eq!(nonce, 0x11223344);
        assert_eq!(req, 0x0102_0304_0506_0708);
        assert_eq!(tgt, 0xA0A1_A2A3_A4A5_A6A7);
    }

    #[test]
    fn exec_v3_id_golden() {
        let mut buf = [0u8; 32];
        let n = encode_exec_v3_id(0x01020304, 0x1122_3344_5566_7788, 9, &mut buf).unwrap();
        const GOLDEN: [u8; 17] = [
            b'P', b'O', 3, 3, // magic + ver + OP_EXEC
            0x04, 0x03, 0x02, 0x01, // nonce LE
            0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, // requester_id LE
            9,    // image_id
        ];
        assert_eq!(&buf[..n], &GOLDEN);
        let (nonce, req, img) = decode_exec_v3_id(&buf[..n]).unwrap();
        assert_eq!(nonce, 0x01020304);
        assert_eq!(req, 0x1122_3344_5566_7788);
        assert_eq!(img, 9);
    }

    #[test]
    fn rsp_v3_golden() {
        let frame = encode_rsp_v3(OP_ROUTE, 0xAABBCCDD, STATUS_DENY);
        const GOLDEN: [u8; 10] = [
            b'P',
            b'O',
            3,
            (2 | 0x80),
            0xDD,
            0xCC,
            0xBB,
            0xAA,
            1, // STATUS_DENY
            0,
        ];
        assert_eq!(frame, GOLDEN);
        let (ver, op, nonce, status) = decode_rsp_v2_or_v3(&frame).unwrap();
        assert_eq!(ver, VERSION_V3);
        assert_eq!(op, OP_ROUTE);
        assert_eq!(nonce, 0xAABBCCDD);
        assert_eq!(status, STATUS_DENY);
    }

    #[test]
    fn route_v2_roundtrip() {
        let mut buf = [0u8; 128];
        let n = encode_route_v2(0x12345678, b"bundlemgrd", b"execd", &mut buf).unwrap();
        let (nonce, req, tgt) = decode_route_v2(&buf[..n]).unwrap();
        assert_eq!(nonce, 0x12345678);
        assert_eq!(req, b"bundlemgrd");
        assert_eq!(tgt, b"execd");
    }

    #[test]
    fn exec_v2_roundtrip() {
        let mut buf = [0u8; 128];
        let n = encode_exec_v2(0x90ABCDEF, b"selftest-client", 2, &mut buf).unwrap();
        let (nonce, req, img) = decode_exec_v2(&buf[..n]).unwrap();
        assert_eq!(nonce, 0x90ABCDEF);
        assert_eq!(req, b"selftest-client");
        assert_eq!(img, 2);
    }

    #[test]
    fn rsp_v2_roundtrip() {
        let frame = encode_rsp_v2(OP_ROUTE, 0xAABBCCDD, STATUS_DENY);
        let (op, nonce, status) = decode_rsp_v2(&frame).unwrap();
        assert_eq!(op, OP_ROUTE);
        assert_eq!(nonce, 0xAABBCCDD);
        assert_eq!(status, STATUS_DENY);
        // A v3 frame is not a v2 frame.
        assert_eq!(decode_rsp_v2(&encode_rsp_v3(OP_ROUTE, 1, STATUS_ALLOW)), None);
    }

    #[test]
    fn abi_profile_v2_roundtrip() {
        let mut req = [0u8; 32];
        let n = encode_abi_profile_get_v2(0x0102_0304, 0x1122_3344_5566_7788, &mut req).unwrap();
        let (nonce, subject_id) = decode_abi_profile_get_v2(&req[..n]).unwrap();
        assert_eq!(nonce, 0x0102_0304);
        assert_eq!(subject_id, 0x1122_3344_5566_7788);

        let profile = [1u8, 2, 3, 4];
        let mut rsp = [0u8; 64];
        let m = encode_abi_profile_rsp_v2(0x1122_3344, STATUS_ALLOW, &profile, &mut rsp).unwrap();
        let (rsp_nonce, status, got_profile) = decode_abi_profile_rsp_v2(&rsp[..m]).unwrap();
        assert_eq!(rsp_nonce, 0x1122_3344);
        assert_eq!(status, STATUS_ALLOW);
        assert_eq!(got_profile, &profile);
    }

    #[test]
    fn reject_truncation_and_mutation_matrix() {
        let mut buf = [0u8; 128];
        let n = encode_route_v2(0x12345678, b"bundlemgrd", b"execd", &mut buf).unwrap();
        crate::codec::testing::assert_reject_matrix(&buf[..n], 4, &|f| {
            decode_route_v2(f).is_some()
        });
        let mut buf = [0u8; 32];
        let n = encode_route_v3_id(1, 2, 3, &mut buf).unwrap();
        crate::codec::testing::assert_reject_matrix(&buf[..n], 4, &|f| {
            decode_route_v3_id(f).is_some()
        });
    }
}
