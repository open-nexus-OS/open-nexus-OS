// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Session-authority wire protocol (TASK-0065B). `sessiond` is the single
//! authority for session state (greeter → active; locked designed-in but
//! reserved) and the user registry; clients (windowd greeter, abilitymgr
//! launch gate) query it request/reply — there is no push channel.

/// First magic byte (`'S'`).
pub const MAGIC0: u8 = b'S';
/// Second magic byte (`'N'`).
pub const MAGIC1: u8 = b'N';
/// Protocol version.
pub const VERSION: u8 = 1;

/// Query session state + the user registry.
pub const OP_GET_STATE: u8 = 1;
/// Log a user in (greeter → active). Auth docks behind this op later.
pub const OP_LOGIN: u8 = 2;
/// Lock the active session. Reserved: answers `STATUS_UNSUPPORTED` today.
pub const OP_LOCK: u8 = 3;

/// Operation succeeded.
pub const STATUS_OK: u8 = 0;
/// Request frame was malformed.
pub const STATUS_MALFORMED: u8 = 1;
/// Operation is not supported (yet).
pub const STATUS_UNSUPPORTED: u8 = 2;
/// LOGIN named a user id that is not in the registry.
pub const STATUS_UNKNOWN_USER: u8 = 3;
/// Operation is invalid in the current session state.
pub const STATUS_WRONG_STATE: u8 = 4;

/// Wire value for the greeter state (no session yet).
pub const STATE_GREETER: u8 = 0;
/// Wire value for an active session.
pub const STATE_ACTIVE: u8 = 1;
/// Wire value for a locked session (reserved).
pub const STATE_LOCKED: u8 = 2;
/// `active_idx` value when no user is active.
pub const NO_ACTIVE_USER: u8 = 0xFF;

/// Byte offset where GET_STATE response user entries begin
/// (after status + state + active_idx + count).
pub const GET_STATE_BODY_OFFSET: usize = 8;

/// Encodes a GET_STATE request: `[S, N, ver, OP_GET_STATE]`.
pub fn encode_get_state(out: &mut [u8; 4]) {
    *out = [MAGIC0, MAGIC1, VERSION, OP_GET_STATE];
}

/// Decodes the request opcode from a sessiond v1 request frame.
pub fn decode_request_op(frame: &[u8]) -> Option<u8> {
    crate::codec::request_op(frame, MAGIC0, MAGIC1, VERSION)
}

/// Decodes the GET_STATE response header → `(status, state, active_idx, count)`.
///
/// Response frame:
/// `[S, N, ver, OP_GET_STATE|0x80, status:u8, state:u8, active_idx:u8, count:u8, entries...]`
/// where each entry is
/// `[id_len:u8, id..., name_len:u8, name..., product_len:u8, product...]` (UTF-8).
/// Entry parsing (which needs allocation) lives in the consumer — trailing
/// entry bytes are deliberately NOT length-checked here.
pub fn decode_get_state_header(frame: &[u8]) -> Option<(u8, u8, u8, u8)> {
    let mut r = crate::codec::Reader::new(frame);
    crate::codec::check_hdr(&mut r, MAGIC0, MAGIC1, VERSION, OP_GET_STATE | 0x80)?;
    let status = r.take_u8()?;
    let state = r.take_u8()?;
    let active_idx = r.take_u8()?;
    let count = r.take_u8()?;
    Some((status, state, active_idx, count))
}

crate::frames! {
    protocol(magic0 = MAGIC0, magic1 = MAGIC1, version = VERSION);

    /// LOGIN request: `[S, N, ver, OP_LOGIN, id_len:u8, id...]`.
    request encode_login_req / decode_login_req (op = OP_LOGIN) {
        user_id: bytes8(min = 1, max = 255),
    }
    /// LOGIN response → `(status, product_id_bytes)`:
    /// `[S, N, ver, OP_LOGIN|0x80, status:u8, product_len:u8, product...]`.
    reply decode decode_login_rsp (op = OP_LOGIN) {
        status: u8,
        product: bytes8(min = 0, max = 255),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_state_req_golden() {
        let mut req = [0u8; 4];
        encode_get_state(&mut req);
        assert_eq!(req, [b'S', b'N', 1, OP_GET_STATE]);
        assert_eq!(decode_request_op(&req).unwrap(), OP_GET_STATE);
    }

    #[test]
    fn get_state_rsp_header_roundtrip() {
        let rsp = [b'S', b'N', 1, OP_GET_STATE | 0x80, STATUS_OK, STATE_GREETER, NO_ACTIVE_USER, 1];
        assert_eq!(
            decode_get_state_header(&rsp),
            Some((STATUS_OK, STATE_GREETER, NO_ACTIVE_USER, 1))
        );
        // Wrong opcode rejected.
        let bad = [b'S', b'N', 1, OP_LOGIN | 0x80, STATUS_OK, 0, 0, 0];
        assert_eq!(decode_get_state_header(&bad), None);
    }

    #[test]
    fn login_roundtrip() {
        let mut req = [0u8; 64];
        let len = encode_login_req(b"jenning", &mut req).unwrap();
        assert_eq!(
            &req[..len],
            &[b'S', b'N', 1, OP_LOGIN, 7, b'j', b'e', b'n', b'n', b'i', b'n', b'g']
        );
        assert_eq!(decode_login_req(&req[..len]).unwrap(), b"jenning");

        let rsp = [
            b'S',
            b'N',
            1,
            OP_LOGIN | 0x80,
            STATUS_OK,
            7,
            b'd',
            b'e',
            b'f',
            b'a',
            b'u',
            b'l',
            b't',
        ];
        let (status, product) = decode_login_rsp(&rsp).unwrap();
        assert_eq!(status, STATUS_OK);
        assert_eq!(product, b"default");
    }

    #[test]
    fn malformed_rejected() {
        // Empty id refused at encode time.
        let mut out = [0u8; 8];
        assert_eq!(encode_login_req(b"", &mut out), None);
        // Truncated login request rejected.
        assert_eq!(decode_login_req(&[b'S', b'N', 1, OP_LOGIN, 3, b'a']), None);
        // Wrong magic rejected.
        assert_eq!(decode_request_op(&[b'X', b'N', 1, OP_GET_STATE]), None);
        // Length mismatch in login rsp rejected.
        assert_eq!(decode_login_rsp(&[b'S', b'N', 1, OP_LOGIN | 0x80, 0, 9, b'x']), None);
    }

    #[test]
    fn reject_truncation_and_mutation_matrix() {
        let mut req = [0u8; 64];
        let len = encode_login_req(b"jenning", &mut req).unwrap();
        crate::codec::testing::assert_reject_matrix(&req[..len], 4, &|f| {
            decode_login_req(f).is_some()
        });
    }
}
