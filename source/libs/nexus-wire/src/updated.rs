// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Updated service frames (system-set staging + boot control).

/// First magic byte (`'U'`).
pub const MAGIC0: u8 = b'U';
/// Second magic byte (`'D'`).
pub const MAGIC1: u8 = b'D';
/// Protocol version.
pub const VERSION: u8 = 1;

/// Stage system-set request opcode.
pub const OP_STAGE: u8 = 1;
/// Switch to staged slot opcode.
pub const OP_SWITCH: u8 = 2;
/// Commit health for pending slot opcode.
pub const OP_HEALTH_OK: u8 = 3;
/// Query status opcode.
pub const OP_GET_STATUS: u8 = 4;
/// Record a boot attempt (decrement tries / trigger rollback).
pub const OP_BOOT_ATTEMPT: u8 = 5;

/// Status: operation succeeded.
pub const STATUS_OK: u8 = 0;
/// Status: request was malformed.
pub const STATUS_MALFORMED: u8 = 1;
/// Status: unsupported operation/version.
pub const STATUS_UNSUPPORTED: u8 = 2;
/// Status: operation failed.
pub const STATUS_FAILED: u8 = 3;

/// Maximum inline system-set bytes for stage requests.
pub const MAX_STAGE_BYTES: usize = 8 * 1024;

crate::frames! {
    protocol(magic0 = MAGIC0, magic1 = MAGIC1, version = VERSION);

    /// Stage request: `[U, D, ver, OP_STAGE, len:u32le, bytes...]`.
    request encode_stage_req / decode_stage_req (op = OP_STAGE) {
        bytes: bytes32(min = 1, max = MAX_STAGE_BYTES),
    }
    /// Switch request: `[U, D, ver, OP_SWITCH, tries_left:u8]` (non-zero).
    request encode_switch_req / decode_switch_req (op = OP_SWITCH) {
        tries_left: nz_u8,
    }
    /// Health-ok request: `[U, D, ver, OP_HEALTH_OK]`.
    request encode encode_health_ok_req (op = OP_HEALTH_OK) {}
    /// Get-status request: `[U, D, ver, OP_GET_STATUS]`.
    request encode encode_get_status_req (op = OP_GET_STATUS) {}
    /// Boot-attempt request: `[U, D, ver, OP_BOOT_ATTEMPT]`.
    request encode encode_boot_attempt_req (op = OP_BOOT_ATTEMPT) {}
    /// Boot-attempt response → `(status, slot)` (two reserved trailing bytes).
    reply decode decode_boot_attempt_rsp (op = OP_BOOT_ATTEMPT) {
        status: u8,
        slot: u8,
        _r: pad(2),
    }
}

/// Decodes a health-ok request frame.
pub fn decode_health_ok_req(frame: &[u8]) -> bool {
    frame.len() == 4 && decode_request_op(frame) == Some(OP_HEALTH_OK)
}

/// Decodes a get-status request frame.
pub fn decode_get_status_req(frame: &[u8]) -> bool {
    frame.len() == 4 && decode_request_op(frame) == Some(OP_GET_STATUS)
}

/// Decodes a boot-attempt request frame.
pub fn decode_boot_attempt_req(frame: &[u8]) -> bool {
    frame.len() == 4 && decode_request_op(frame) == Some(OP_BOOT_ATTEMPT)
}

/// Decodes the opcode from a request frame.
pub fn decode_request_op(frame: &[u8]) -> Option<u8> {
    crate::codec::request_op(frame, MAGIC0, MAGIC1, VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_roundtrip_and_bounds() {
        let mut buf = [0u8; 64];
        let n = encode_stage_req(b"system-set", &mut buf).unwrap();
        assert_eq!(&buf[..8], &[b'U', b'D', 1, OP_STAGE, 10, 0, 0, 0]);
        assert_eq!(decode_stage_req(&buf[..n]), Some(&b"system-set"[..]));
        assert_eq!(encode_stage_req(b"", &mut buf), None);
        crate::codec::testing::assert_reject_matrix(&buf[..n], 4, &|f| {
            decode_stage_req(f).is_some()
        });
    }

    #[test]
    fn switch_requires_nonzero_tries() {
        let mut buf = [0u8; 8];
        assert_eq!(encode_switch_req(0, &mut buf), None);
        let n = encode_switch_req(3, &mut buf).unwrap();
        assert_eq!(&buf[..n], &[b'U', b'D', 1, OP_SWITCH, 3]);
        assert_eq!(decode_switch_req(&buf[..n]), Some(3));
        assert_eq!(decode_switch_req(&[b'U', b'D', 1, OP_SWITCH, 0]), None);
    }

    #[test]
    fn empty_body_requests() {
        let mut buf = [0u8; 8];
        let n = encode_health_ok_req(&mut buf).unwrap();
        assert_eq!(&buf[..n], &[b'U', b'D', 1, OP_HEALTH_OK]);
        assert!(decode_health_ok_req(&buf[..n]));
        assert!(!decode_health_ok_req(&buf[..n - 1]));

        let n = encode_get_status_req(&mut buf).unwrap();
        assert!(decode_get_status_req(&buf[..n]));
        let n = encode_boot_attempt_req(&mut buf).unwrap();
        assert!(decode_boot_attempt_req(&buf[..n]));
        assert!(!decode_get_status_req(&buf[..n])); // wrong op
    }

    #[test]
    fn boot_attempt_rsp_ignores_reserved_bytes() {
        let rsp = [b'U', b'D', 1, OP_BOOT_ATTEMPT | 0x80, STATUS_OK, 1, 0xEE, 0xFF];
        assert_eq!(decode_boot_attempt_rsp(&rsp), Some((STATUS_OK, 1)));
        assert_eq!(decode_boot_attempt_rsp(&rsp[..7]), None);
    }
}
