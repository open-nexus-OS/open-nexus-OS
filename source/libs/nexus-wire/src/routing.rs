// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Bootstrap routing protocol frames shared between init-lite and services (RFC-0005).

/// First magic byte (`'R'`) — private: routing frames are only touched via the codecs.
const MAGIC0: u8 = b'R';
/// Second magic byte (`'T'`).
const MAGIC1: u8 = b'T';
/// Routing protocol version.
pub const VERSION: u8 = 1;

/// Route query opcode.
pub const OP_ROUTE_GET: u8 = 0x40;
/// Route response opcode.
pub const OP_ROUTE_RSP: u8 = 0x41;

/// Status code returned in ROUTE_RSP.
pub const STATUS_OK: u8 = 0;
/// Service is unknown or not routed for the caller.
pub const STATUS_NOT_FOUND: u8 = 1;
/// Request was malformed.
pub const STATUS_MALFORMED: u8 = 2;
/// Request was understood but denied by policy.
pub const STATUS_DENIED: u8 = 3;

/// Maximum supported service-name length in routing frames.
pub const MAX_SERVICE_NAME_LEN: usize = 48;

crate::frames! {
    protocol(magic0 = MAGIC0, magic1 = MAGIC1, version = VERSION);

    /// ROUTE_GET request: `[R, T, ver, OP_ROUTE_GET, name_len:u8, name...]`.
    request encode_route_get / decode_route_get (op = OP_ROUTE_GET) {
        name: bytes8(min = 1, max = MAX_SERVICE_NAME_LEN),
    }
    /// ROUTE_RSP response: `[R, T, ver, OP_ROUTE_RSP, status, send_slot:u32le,
    /// recv_slot:u32le]` (a distinct opcode, not the `|0x80` convention).
    request fixed encode_route_rsp / decode_route_rsp (op = OP_ROUTE_RSP) {
        status: u8,
        send_slot: u32le,
        recv_slot: u32le,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_get_golden() {
        let name = b"vfsd";
        let mut buf = [0u8; 32];
        let n = encode_route_get(name, &mut buf).expect("encode");
        const GOLDEN: [u8; 9] = [b'R', b'T', 1, 0x40, 4, b'v', b'f', b's', b'd'];
        assert_eq!(&buf[..n], &GOLDEN);
        assert_eq!(decode_route_get(&buf[..n]).unwrap(), name);
    }

    #[test]
    fn route_get_roundtrip() {
        let name = b"vfsd";
        let mut buf = [0u8; 32];
        let n = encode_route_get(name, &mut buf).expect("encode");
        assert_eq!(decode_route_get(&buf[..n]).unwrap(), name);
    }

    #[test]
    fn route_rsp_golden() {
        let frame = encode_route_rsp(STATUS_OK, 0x1122_3344, 0xAABB_CCDD);
        const GOLDEN: [u8; 13] = [
            b'R', b'T', 1, 0x41, 0, // status OK
            0x44, 0x33, 0x22, 0x11, // send_slot LE
            0xDD, 0xCC, 0xBB, 0xAA, // recv_slot LE
        ];
        assert_eq!(frame, GOLDEN);
        let (status, send, recv) = decode_route_rsp(&frame).unwrap();
        assert_eq!(status, STATUS_OK);
        assert_eq!(send, 0x1122_3344);
        assert_eq!(recv, 0xAABB_CCDD);
    }

    #[test]
    fn route_rsp_roundtrip() {
        let frame = encode_route_rsp(STATUS_OK, 12, 34);
        let (status, send, recv) = decode_route_rsp(&frame).unwrap();
        assert_eq!(status, STATUS_OK);
        assert_eq!(send, 12);
        assert_eq!(recv, 34);
    }

    #[test]
    fn reject_truncation_and_mutation_matrix() {
        let mut buf = [0u8; 32];
        let n = encode_route_get(b"vfsd", &mut buf).unwrap();
        crate::codec::testing::assert_reject_matrix(&buf[..n], 4, &|f| {
            decode_route_get(f).is_some()
        });
        let rsp = encode_route_rsp(STATUS_OK, 1, 2);
        crate::codec::testing::assert_reject_matrix(&rsp, 4, &|f| decode_route_rsp(f).is_some());
    }
}
