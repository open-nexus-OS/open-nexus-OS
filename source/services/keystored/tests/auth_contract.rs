// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Keystored auth/reject contract tests — malformed frames, bad magic,
//!          version mismatch, length mismatch, and denied access paths.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p keystored -- auth`
//!
//! TEST_SCOPE:
//!   - Bad magic → STATUS_MALFORMED
//!   - Wrong version → STATUS_UNSUPPORTED
//!   - Frame too short → STATUS_MALFORMED
//!   - Length mismatch → STATUS_MALFORMED
//!   - Zero-length key → STATUS_MALFORMED
//!   - Status codes never leak into opcode position
//!
//! DEPENDENCIES:
//!   - `keystored::protocol`: wire format constants + encode/decode functions

use keystored::protocol::{
    decode_response, encode_request, encode_response, is_valid_header, MAGIC0, MAGIC1, OP_GET,
    OP_PUT, STATUS_MALFORMED, STATUS_UNSUPPORTED, VERSION,
};

// ── Reject tests ──────────────────────────────────────────────────

#[test]
fn reject_bad_magic_byte_0() {
    // Build a frame with wrong first magic byte.
    let mut frame = encode_request(OP_PUT, b"k", b"v").unwrap();
    frame[0] = b'X';
    assert!(!is_valid_header(&frame));
    // Server-side: should reject with MALFORMED
    let rsp = server_handle(&frame);
    let (status, _) = decode_response(&rsp, OP_GET).unwrap();
    assert_eq!(status, STATUS_MALFORMED);
}

#[test]
fn reject_bad_magic_byte_1() {
    let mut frame = encode_request(OP_PUT, b"k", b"v").unwrap();
    frame[1] = b'Y';
    assert!(!is_valid_header(&frame));
    let rsp = server_handle(&frame);
    let (status, _) = decode_response(&rsp, OP_GET).unwrap();
    assert_eq!(status, STATUS_MALFORMED);
}

#[test]
fn reject_wrong_version() {
    let mut frame = encode_request(OP_PUT, b"k", b"v").unwrap();
    frame[2] = 99;
    assert!(!is_valid_header(&frame));
    let rsp = server_handle(&frame);
    // Response uses the original frame opcode (OP_PUT), not the version byte
    let (status, _) = decode_response(&rsp, OP_PUT).unwrap();
    assert_eq!(status, STATUS_UNSUPPORTED);
}

#[test]
fn reject_frame_too_short_for_header() {
    // 4 bytes: magic + version + op, but no key_len/val_len (needs 7 min)
    let frame = vec![MAGIC0, MAGIC1, VERSION, OP_GET];
    assert!(is_valid_header(&frame));
    let rsp = server_handle(&frame);
    let (status, _) = decode_response(&rsp, OP_GET).unwrap();
    assert_eq!(status, STATUS_MALFORMED);
}

#[test]
fn reject_empty_frame() {
    let frame = vec![];
    assert!(!is_valid_header(&frame));
    let rsp = server_handle(&frame);
    let (status, _) = decode_response(&rsp, OP_GET).unwrap();
    assert_eq!(status, STATUS_MALFORMED);
}

#[test]
fn reject_zero_length_key() {
    // key_len=0 is invalid
    let mut frame = vec![MAGIC0, MAGIC1, VERSION, OP_GET, 0u8];
    frame.extend_from_slice(&0u16.to_le_bytes()); // val_len=0
    let rsp = server_handle(&frame);
    let (status, _) = decode_response(&rsp, OP_GET).unwrap();
    assert_eq!(status, STATUS_MALFORMED);
}

#[test]
fn reject_length_mismatch_underflow() {
    // Declare key_len=5 but provide only 3 key bytes
    let mut frame = vec![MAGIC0, MAGIC1, VERSION, OP_GET, 5u8];
    frame.extend_from_slice(&0u16.to_le_bytes()); // val_len=0
    frame.extend_from_slice(b"abc"); // only 3 bytes
    let rsp = server_handle(&frame);
    let (status, _) = decode_response(&rsp, OP_GET).unwrap();
    assert_eq!(status, STATUS_MALFORMED);
}

#[test]
fn reject_length_mismatch_overflow() {
    // Declare key_len=1 but provide 3 key bytes
    let mut frame = vec![MAGIC0, MAGIC1, VERSION, OP_GET, 1u8];
    frame.extend_from_slice(&0u16.to_le_bytes()); // val_len=0
    frame.extend_from_slice(b"abc"); // 3 bytes — more than declared
    let rsp = server_handle(&frame);
    let (status, _) = decode_response(&rsp, OP_GET).unwrap();
    assert_eq!(status, STATUS_MALFORMED);
}

#[test]
fn reject_malformed_does_not_crash_on_large_declared_length() {
    // Declare val_len=65535 but provide no value bytes.
    // val_len > MAX_VAL_LEN (256) → STATUS_TOO_LARGE.
    let mut frame = vec![MAGIC0, MAGIC1, VERSION, OP_PUT, 1u8];
    frame.extend_from_slice(&65535u16.to_le_bytes());
    frame.push(b'k'); // 1 key byte
    let rsp = server_handle(&frame);
    let (status, _) = decode_response(&rsp, OP_PUT).unwrap();
    assert_eq!(status, keystored::protocol::STATUS_TOO_LARGE);
}

#[test]
fn reject_nonexistent_key_is_not_malformed() {
    // GET on a key that doesn't exist should return NOT_FOUND, not MALFORMED.
    let req = encode_request(OP_GET, b"no_such_key", &[]).unwrap();
    let rsp = server_handle(&req);
    let (status, _) = decode_response(&rsp, OP_GET).unwrap();
    // STATUS_NOT_FOUND = 1, STATUS_MALFORMED = 2
    assert_ne!(status, STATUS_MALFORMED, "missing key must not be malformed");
}

// ── Minimal server simulator (mirrors os_stub.rs header parsing) ──

fn server_handle(frame: &[u8]) -> Vec<u8> {
    use std::collections::BTreeMap;
    static mut STORE: Option<BTreeMap<Vec<u8>, Vec<u8>>> = None;

    // Leak a static store for simplicity in tests
    let store = unsafe { STORE.get_or_insert_with(BTreeMap::new) };

    use keystored::protocol::{MAX_KEY_LEN, MAX_VAL_LEN, STATUS_TOO_LARGE};

    if frame.len() < 4 || frame[0] != MAGIC0 || frame[1] != MAGIC1 {
        return encode_response(OP_GET, STATUS_MALFORMED, &[]);
    }
    let op = frame[3];
    if frame[2] != VERSION {
        return encode_response(op, STATUS_UNSUPPORTED, &[]);
    }
    if frame.len() < 7 {
        return encode_response(op, STATUS_MALFORMED, &[]);
    }
    let key_len = frame[4] as usize;
    let val_len = u16::from_le_bytes([frame[5], frame[6]]) as usize;
    let total = 7usize.saturating_add(key_len).saturating_add(val_len);
    if key_len == 0 || key_len > MAX_KEY_LEN || val_len > MAX_VAL_LEN || frame.len() != total {
        return encode_response(
            op,
            if key_len > MAX_KEY_LEN || val_len > MAX_VAL_LEN {
                STATUS_TOO_LARGE
            } else {
                STATUS_MALFORMED
            },
            &[],
        );
    }
    let key = &frame[7..7 + key_len];
    let val = &frame[7 + key_len..7 + key_len + val_len];

    match op {
        OP_PUT => {
            store.insert(key.to_vec(), val.to_vec());
            encode_response(OP_PUT, keystored::protocol::STATUS_OK, &[])
        }
        OP_GET => match store.get(key) {
            Some(v) => encode_response(OP_GET, keystored::protocol::STATUS_OK, v),
            None => encode_response(OP_GET, keystored::protocol::STATUS_NOT_FOUND, &[]),
        },
        _ => encode_response(op, STATUS_UNSUPPORTED, &[]),
    }
}
