// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Execd service frames (bring-up) used for OS-lite exec spawning.
//!
//! This is intentionally minimal and byte-oriented (no IDL) to keep early boot
//! deterministic.

/// First magic byte (`'E'`).
pub const MAGIC0: u8 = b'E';
/// Second magic byte (`'X'`).
pub const MAGIC1: u8 = b'X';
/// Protocol version.
pub const VERSION: u8 = 1;

/// Exec image request opcode.
pub const OP_EXEC_IMAGE: u8 = 1;

/// Status: operation succeeded.
pub const STATUS_OK: u8 = 0;
/// Status: request was malformed.
pub const STATUS_MALFORMED: u8 = 1;
/// Status: unsupported operation/version.
pub const STATUS_UNSUPPORTED: u8 = 2;
/// Status: exec failed.
pub const STATUS_FAILED: u8 = 3;
/// Status: denied by policy.
pub const STATUS_DENIED: u8 = 4;

/// Maximum supported requester-name length (bytes).
pub const MAX_REQUESTER_LEN: usize = 48;

/// Encodes an `execd` v1 exec-image request frame.
///
/// Frame: `[E,X,ver,OP_EXEC_IMAGE,image_id,stack_pages:u8, requester_len:u8, requester...]`
///
/// Hand-written (not `frames!`): the historical argument order (`requester`
/// first) differs from the wire field order, which the DSL mirrors 1:1.
pub fn encode_exec_image_req(
    requester: &[u8],
    image_id: u8,
    stack_pages: u8,
    out: &mut [u8],
) -> Option<usize> {
    let mut w = crate::codec::Writer::new(out);
    crate::codec::put_hdr(&mut w, MAGIC0, MAGIC1, VERSION, OP_EXEC_IMAGE)?;
    w.put_u8(image_id)?;
    w.put_u8(stack_pages)?;
    w.put_len8_bytes(requester, 1, MAX_REQUESTER_LEN)?;
    Some(w.pos())
}

crate::frames! {
    protocol(magic0 = MAGIC0, magic1 = MAGIC1, version = VERSION);

    /// Exec-image request:
    /// `[E,X,ver,OP_EXEC_IMAGE,image_id,stack_pages:u8, requester_len:u8, requester...]`
    /// → `(image_id, stack_pages, requester)`.
    request decode decode_exec_image_req (op = OP_EXEC_IMAGE) {
        image_id: u8,
        stack_pages: u8,
        requester: bytes8(min = 1, max = MAX_REQUESTER_LEN),
    }
    /// Exec-image response: `[E,X,ver,OP_EXEC_IMAGE|0x80,status:u8,pid:u32le]`.
    reply fixed encode_exec_image_rsp / decode_exec_image_rsp (op = OP_EXEC_IMAGE) {
        status: u8,
        pid: u32le,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execd_req_golden() {
        let requester = b"selftest-client";
        let mut buf = [0u8; 64];
        let n = encode_exec_image_req(requester, 1, 19, &mut buf).unwrap();
        const GOLDEN_PREFIX: [u8; 7] = [b'E', b'X', 1, 1, 1, 19, 15];
        assert_eq!(&buf[..7], &GOLDEN_PREFIX);
        assert_eq!(&buf[7..n], requester);
        let (img, sp, req) = decode_exec_image_req(&buf[..n]).unwrap();
        assert_eq!(img, 1);
        assert_eq!(sp, 19);
        assert_eq!(req, requester);
    }

    #[test]
    fn execd_rsp_golden() {
        let frame = encode_exec_image_rsp(STATUS_OK, 0x1122_3344);
        const GOLDEN: [u8; 9] = [b'E', b'X', 1, 0x81, 0, 0x44, 0x33, 0x22, 0x11];
        assert_eq!(frame, GOLDEN);
        let (status, pid) = decode_exec_image_rsp(&frame).unwrap();
        assert_eq!(status, STATUS_OK);
        assert_eq!(pid, 0x1122_3344);
    }

    #[test]
    fn reject_truncation_and_mutation_matrix() {
        let mut buf = [0u8; 64];
        let n = encode_exec_image_req(b"selftest-client", 1, 19, &mut buf).unwrap();
        crate::codec::testing::assert_reject_matrix(&buf[..n], 4, &|f| {
            decode_exec_image_req(f).is_some()
        });
        let rsp = encode_exec_image_rsp(STATUS_OK, 0x1122_3344);
        crate::codec::testing::assert_reject_matrix(&rsp, 4, &|f| {
            decode_exec_image_rsp(f).is_some()
        });
    }
}
