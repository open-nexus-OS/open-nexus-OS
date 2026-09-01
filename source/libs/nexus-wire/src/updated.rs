// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Updated service frames (system-set staging + boot control).

/// First magic byte (`'U'`).
pub const MAGIC0: u8 = b'U';
/// Second magic byte (`'D'`).
pub const MAGIC1: u8 = b'D';
/// Protocol version.
pub const VERSION: u8 = 1;

/// RETIRED (TASK-0179): opcode 1 was the v1 inline stage (8 KiB cap, bytes
/// verified in RAM and discarded). Path-based staging replaced it — there
/// is deliberately no dual API and opcode 1 is never reused.
///
/// Stage-from-source request opcode (RFC-0089 §8): the payload is a
/// bounded VFS path (`/updates/...` on the data volume), the engine streams the
/// container from there.
pub const OP_STAGE_SOURCE: u8 = 6;
/// Switch to staged slot opcode.
pub const OP_SWITCH: u8 = 2;
/// Commit health for pending slot opcode.
pub const OP_HEALTH_OK: u8 = 3;
/// Query status opcode.
pub const OP_GET_STATUS: u8 = 4;
/// Record a boot attempt (decrement tries / trigger rollback).
pub const OP_BOOT_ATTEMPT: u8 = 5;
/// Offline feed enumeration (RFC-0089 §9): deterministic listing of
/// staged-source candidates.
pub const OP_FEED_LIST: u8 = 7;
/// Feed check: is a stageable candidate present?
pub const OP_CHECK: u8 = 8;
/// Rollback request (TASK-0140): pass-through to bootctld OP_ROLLBACK —
/// clears a pending trial back to the standing slot. Gated like the other
/// mutating ops.
pub const OP_ROLLBACK: u8 = 9;

/// Status: operation succeeded.
pub const STATUS_OK: u8 = 0;
/// Status: request was malformed.
pub const STATUS_MALFORMED: u8 = 1;
/// Status: unsupported operation/version.
pub const STATUS_UNSUPPORTED: u8 = 2;
/// Status: operation failed.
pub const STATUS_FAILED: u8 = 3;
/// Status: sender lacks the `updates.manage` grant (TASK-0140). Distinct
/// from FAILED so a policy deny is never mistaken for a machine reject.
pub const STATUS_DENIED: u8 = 4;

/// Maximum staging-source path bytes (RFC-0089 §8 bounded path).
pub const MAX_SOURCE_PATH_BYTES: usize = 200;

crate::frames! {
    protocol(magic0 = MAGIC0, magic1 = MAGIC1, version = VERSION);

    /// Stage-from-source request:
    /// `[U, D, ver, OP_STAGE_SOURCE, len:u32le, path...]`.
    request encode_stage_source_req / decode_stage_source_req (op = OP_STAGE_SOURCE) {
        path: bytes32(min = 1, max = MAX_SOURCE_PATH_BYTES),
    }
    /// Feed-list request: `[U, D, ver, OP_FEED_LIST]`.
    request encode encode_feed_list_req (op = OP_FEED_LIST) {}
    /// Feed-check request: `[U, D, ver, OP_CHECK]`.
    request encode encode_check_req (op = OP_CHECK) {}
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
    /// Rollback request: `[U, D, ver, OP_ROLLBACK]`.
    request encode encode_rollback_req (op = OP_ROLLBACK) {}
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

/// Decodes a rollback request frame.
pub fn decode_rollback_req(frame: &[u8]) -> bool {
    frame.len() == 4 && decode_request_op(frame) == Some(OP_ROLLBACK)
}

/// Decodes the opcode from a request frame.
pub fn decode_request_op(frame: &[u8]) -> Option<u8> {
    crate::codec::request_op(frame, MAGIC0, MAGIC1, VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_source_roundtrip_and_bounds() {
        // TASK-0179: the v1 inline stage is retired; staging names a
        // bounded VFS path and the engine streams the container from it.
        let mut buf = [0u8; 256];
        let path = b"/updates/os-B.nxs";
        let n = encode_stage_source_req(path, &mut buf).unwrap();
        assert_eq!(&buf[..8], &[b'U', b'D', 1, OP_STAGE_SOURCE, 17, 0, 0, 0]);
        assert_eq!(decode_stage_source_req(&buf[..n]), Some(&path[..]));
        assert_eq!(encode_stage_source_req(b"", &mut buf), None);
        // Above the path bound: refused, never truncated.
        let oversized = [b'x'; MAX_SOURCE_PATH_BYTES + 1];
        assert_eq!(encode_stage_source_req(&oversized, &mut buf), None);
        crate::codec::testing::assert_reject_matrix(&buf[..n], 4, &|f| {
            decode_stage_source_req(f).is_some()
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

        let n = encode_rollback_req(&mut buf).unwrap();
        assert_eq!(&buf[..n], &[b'U', b'D', 1, OP_ROLLBACK]);
        assert!(decode_rollback_req(&buf[..n]));
        assert!(!decode_rollback_req(&buf[..n - 1]));
    }

    #[test]
    fn boot_attempt_rsp_ignores_reserved_bytes() {
        let rsp = [b'U', b'D', 1, OP_BOOT_ATTEMPT | 0x80, STATUS_OK, 1, 0xEE, 0xFF];
        assert_eq!(decode_boot_attempt_rsp(&rsp), Some((STATUS_OK, 1)));
        assert_eq!(decode_boot_attempt_rsp(&rsp[..7]), None);
    }
}
