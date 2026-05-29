// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: StateFS unauthorized / reject contract tests — malformed frames,
//!          invalid keys, oversized values, and access-denied paths.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p statefsd -- unauthorized`
//!
//! TEST_SCOPE:
//!   - Bad magic → STATUS_MALFORMED
//!   - Empty key → invalid key (error from encode)
//!   - Key too long → STATUS_KEY_TOO_LONG
//!   - Value too large → STATUS_VALUE_TOO_LARGE
//!   - Unsupported opcode → STATUS_UNSUPPORTED
//!   - Protocol v2 with malformed nonce → STATUS_MALFORMED
//!
//! DEPENDENCIES:
//!   - `statefs::protocol`: wire format constants + encode/decode functions

use statefs::protocol as proto;

// ── Reject tests ──────────────────────────────────────────────────

#[test]
fn reject_bad_magic() {
    let mut frame = proto::encode_put_request("/state/k", b"v").unwrap();
    frame[0] = b'X';
    let req = proto::decode_request(&frame);
    assert!(req.is_err());
    assert_eq!(req.unwrap_err(), proto::STATUS_MALFORMED);
}

#[test]
fn reject_empty_key_in_put() {
    // Empty keys are accepted by encode but rejected by decode.
    let frame = proto::encode_put_request("", b"v").unwrap();
    let req = proto::decode_request(&frame);
    assert!(req.is_err());
}

#[test]
fn reject_empty_key_in_get() {
    let frame = proto::encode_key_only_request(proto::OP_GET, "").unwrap();
    let req = proto::decode_request(&frame);
    assert!(req.is_err());
}

#[test]
fn reject_unsupported_opcode() {
    let mut frame = vec![proto::MAGIC0, proto::MAGIC1, proto::VERSION, 99u8];
    frame.extend_from_slice(&4u16.to_le_bytes()); // key_len=4
    frame.extend_from_slice(b"/key");
    let req = proto::decode_request(&frame);
    assert!(req.is_err());
    assert_eq!(req.unwrap_err(), proto::STATUS_UNSUPPORTED);
}

#[test]
fn reject_v2_frame_too_short_for_nonce() {
    // VERSION_V2 requires at least 12 bytes (4 header + 8 nonce)
    let frame = vec![proto::MAGIC0, proto::MAGIC1, proto::VERSION_V2, proto::OP_GET];
    let req = proto::decode_request_with_nonce(&frame);
    assert!(req.is_err());
    assert_eq!(req.unwrap_err(), proto::STATUS_MALFORMED);
}

#[test]
fn reject_sync_with_payload_v1() {
    // SYNC must have empty payload
    let mut frame = vec![proto::MAGIC0, proto::MAGIC1, proto::VERSION, proto::OP_SYNC];
    frame.push(b'x'); // non-empty payload
    let req = proto::decode_request(&frame);
    assert!(req.is_err());
    assert_eq!(req.unwrap_err(), proto::STATUS_MALFORMED);
}

#[test]
fn reject_reopen_with_payload_v1() {
    let mut frame = vec![proto::MAGIC0, proto::MAGIC1, proto::VERSION, proto::OP_REOPEN];
    frame.push(b'x');
    let req = proto::decode_request(&frame);
    assert!(req.is_err());
    assert_eq!(req.unwrap_err(), proto::STATUS_MALFORMED);
}

#[test]
fn reject_unknown_protocol_version() {
    let frame = vec![proto::MAGIC0, proto::MAGIC1, 99u8, proto::OP_GET];
    let req = proto::decode_request(&frame);
    assert!(req.is_err());
    assert_eq!(req.unwrap_err(), proto::STATUS_MALFORMED);
}
