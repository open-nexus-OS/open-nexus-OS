// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Policy control frames (bring-up) shared between init-lite, policyd, and
//! privileged services.

/// First magic byte (`'P'`) — private: only touched via the codecs.
const MAGIC0: u8 = b'P';
/// Second magic byte (`'C'`).
const MAGIC1: u8 = b'C';
/// Policy control protocol version.
pub const VERSION: u8 = 1;

/// Exec authorization request opcode (service -> init-lite).
pub const OP_EXEC_CHECK: u8 = 1;

/// Status: operation allowed.
pub const STATUS_ALLOW: u8 = 0;
/// Status: operation denied.
pub const STATUS_DENY: u8 = 1;
/// Status: malformed request.
pub const STATUS_MALFORMED: u8 = 2;

/// Nonce used to correlate requests and responses.
pub type Nonce = u32;

crate::frames! {
    protocol(magic0 = MAGIC0, magic1 = MAGIC1, version = VERSION);

    /// Exec-check request:
    /// `[P, C, ver, OP_EXEC_CHECK, nonce:u32le, requester_len:u8, requester..., image_id:u8]`.
    request encode_exec_check / decode_exec_check (op = OP_EXEC_CHECK) {
        nonce: u32le,
        requester: bytes8(min = 1, max = 48),
        image_id: u8,
    }
    /// Exec-check response: `[P, C, ver, OP_EXEC_CHECK|0x80, nonce:u32le, status:u8]`.
    reply fixed encode_exec_check_rsp / decode_exec_check_rsp (op = OP_EXEC_CHECK) {
        nonce: u32le,
        status: u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_check_roundtrip() {
        let mut buf = [0u8; 64];
        let n = encode_exec_check(0x11223344, b"selftest-client", 2, &mut buf).unwrap();
        let (nonce, requester, image) = decode_exec_check(&buf[..n]).unwrap();
        assert_eq!(nonce, 0x11223344);
        assert_eq!(requester, b"selftest-client");
        assert_eq!(image, 2);
    }

    #[test]
    fn exec_check_rsp_roundtrip() {
        let frame = encode_exec_check_rsp(0xAABBCCDD, STATUS_DENY);
        let (nonce, status) = decode_exec_check_rsp(&frame).unwrap();
        assert_eq!(nonce, 0xAABBCCDD);
        assert_eq!(status, STATUS_DENY);
    }

    #[test]
    fn reject_truncation_and_mutation_matrix() {
        let mut buf = [0u8; 64];
        let n = encode_exec_check(0x11223344, b"selftest-client", 2, &mut buf).unwrap();
        crate::codec::testing::assert_reject_matrix(&buf[..n], 4, &|f| {
            decode_exec_check(f).is_some()
        });
        let rsp = encode_exec_check_rsp(0xAABBCCDD, STATUS_DENY);
        crate::codec::testing::assert_reject_matrix(&rsp, 4, &|f| {
            decode_exec_check_rsp(f).is_some()
        });
    }
}
