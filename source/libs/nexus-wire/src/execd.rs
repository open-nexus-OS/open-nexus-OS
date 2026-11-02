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
/// The squeeze order (TASK-0333 6f-1, ADR-0087): the lifecycle authority orders the parent to
/// squeeze a frozen child — the batches of `task_squeeze` until the walk ends, the totals
/// answered. Taken from the lifecycle authority's kernel sender id alone, for a live child alone
/// (the freeze/thaw/kill orders 4–6 are the same gate, hand-written in execd and abilitymgr).
pub const OP_SQUEEZE: u8 = 7;
/// The compositor is back (RFC-0112, amendment 2026-11-02): init's one-way notice once a
/// respawned windowd is wired, `[E,X,ver,OP_COMPOSITOR_BACK]`, taken from init's kernel id alone
/// and never answered (execd's answers ride a response endpoint its spawn callers read). execd
/// re-pins each live app's windowd slots over the dead capabilities and tells each app.
pub const OP_COMPOSITOR_BACK: u8 = 8;

/// The notice's frame.
#[must_use]
pub const fn encode_compositor_back() -> [u8; 4] {
    [MAGIC0, MAGIC1, VERSION, OP_COMPOSITOR_BACK]
}

/// Whether `frame` is the notice.
#[must_use]
pub fn is_compositor_back(frame: &[u8]) -> bool {
    frame == encode_compositor_back()
}

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
    /// Squeeze order: `[E,X,ver,OP_SQUEEZE,pid:u32le]`.
    request fixed encode_squeeze / decode_squeeze (op = OP_SQUEEZE) {
        pid: u32le,
    }
    /// Squeeze reply: `[E,X,ver,OP_SQUEEZE|0x80,status:u8,pid:u32le,squeezed:u32le,zero:u32le,
    /// kept:u32le,taken:u32le,stored:u32le]` — the pages recorded, the zero pages dropped, the
    /// pages kept, the frames the stores took, the record bytes written (the batches' sum).
    #[allow(clippy::too_many_arguments)]
    reply fixed encode_squeeze_rsp / decode_squeeze_rsp (op = OP_SQUEEZE) {
        status: u8,
        pid: u32le,
        squeezed: u32le,
        zero: u32le,
        kept: u32le,
        taken: u32le,
        stored: u32le,
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
    fn squeeze_order_and_reply_golden() {
        let order = encode_squeeze(0x0102_0304);
        assert_eq!(order, [b'E', b'X', 1, OP_SQUEEZE, 4, 3, 2, 1]);
        assert_eq!(decode_squeeze(&order), Some(0x0102_0304));
        let rsp = encode_squeeze_rsp(STATUS_OK, 41, 400, 12, 3, 52, 812_000);
        assert_eq!(&rsp[..5], &[b'E', b'X', 1, OP_SQUEEZE | 0x80, STATUS_OK]);
        assert_eq!(rsp.len(), 9 + 5 * 4);
        assert_eq!(decode_squeeze_rsp(&rsp), Some((STATUS_OK, 41, 400, 12, 3, 52, 812_000)));
        assert_eq!(decode_squeeze(&encode_exec_image_rsp(STATUS_OK, 1)), None, "another op");
    }

    #[test]
    fn test_reject_truncated_or_mutated_squeeze_frames() {
        crate::codec::testing::assert_reject_matrix(&encode_squeeze(7), 4, &|f| {
            decode_squeeze(f).is_some()
        });
        let rsp = encode_squeeze_rsp(STATUS_OK, 7, 1, 2, 3, 4, 5);
        crate::codec::testing::assert_reject_matrix(&rsp, 4, &|f| decode_squeeze_rsp(f).is_some());
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
