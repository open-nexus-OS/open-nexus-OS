// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Keystored CRUD contract tests — PUT/GET/DEL protocol-level round-trips.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p keystored -- crud`
//!
//! TEST_SCOPE:
//!   - Request encoding / response decoding round-trips
//!   - PUT → GET → value matches
//!   - GET miss → STATUS_NOT_FOUND
//!   - DEL → GET miss → STATUS_NOT_FOUND
//!   - PUT overwrite → new value
//!
//! DEPENDENCIES:
//!   - `keystored::protocol`: wire format constants + encode/decode functions

use keystored::protocol::{
    self, decode_response, encode_request, encode_response, MAX_KEY_LEN, MAX_VAL_LEN, OP_DEL,
    OP_GET, OP_PUT, RESPONSE_FLAG, STATUS_MALFORMED, STATUS_NOT_FOUND, STATUS_OK, STATUS_TOO_LARGE,
    STATUS_UNSUPPORTED,
};

// ── Helpers ───────────────────────────────────────────────────────

/// Simulate a service that stores key-value pairs and responds via the wire protocol.
struct KeyValueStore {
    store: std::collections::BTreeMap<Vec<u8>, Vec<u8>>,
}

impl KeyValueStore {
    fn new() -> Self {
        Self { store: std::collections::BTreeMap::new() }
    }

    fn handle(&mut self, frame: &[u8]) -> Vec<u8> {
        if frame.len() < 7 || frame[0] != protocol::MAGIC0 || frame[1] != protocol::MAGIC1 {
            return encode_response(OP_GET, STATUS_MALFORMED, &[]);
        }
        if frame[2] != protocol::VERSION {
            return encode_response(frame.get(3).copied().unwrap_or(0), STATUS_UNSUPPORTED, &[]);
        }
        let op = frame[3];
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
                self.store.insert(key.to_vec(), val.to_vec());
                encode_response(OP_PUT, STATUS_OK, &[])
            }
            OP_GET => match self.store.get(key) {
                Some(v) => encode_response(OP_GET, STATUS_OK, v),
                None => encode_response(OP_GET, STATUS_NOT_FOUND, &[]),
            },
            OP_DEL => {
                if self.store.remove(key).is_some() {
                    encode_response(OP_DEL, STATUS_OK, &[])
                } else {
                    encode_response(OP_DEL, STATUS_NOT_FOUND, &[])
                }
            }
            _ => encode_response(op, STATUS_UNSUPPORTED, &[]),
        }
    }
}

// ── CRUD contract tests ───────────────────────────────────────────

#[test]
fn put_then_get_returns_inserted_value() {
    let mut svc = KeyValueStore::new();

    // PUT k1=v1
    let req = encode_request(OP_PUT, b"k1", b"v1").unwrap();
    let rsp = svc.handle(&req);
    let (status, _) = decode_response(&rsp, OP_PUT).unwrap();
    assert_eq!(status, STATUS_OK);

    // GET k1 → v1
    let req = encode_request(OP_GET, b"k1", &[]).unwrap();
    let rsp = svc.handle(&req);
    let (status, value) = decode_response(&rsp, OP_GET).unwrap();
    assert_eq!(status, STATUS_OK);
    assert_eq!(value, b"v1");
}

#[test]
fn get_miss_returns_not_found() {
    let mut svc = KeyValueStore::new();

    let req = encode_request(OP_GET, b"nonexistent", &[]).unwrap();
    let rsp = svc.handle(&req);
    let (status, value) = decode_response(&rsp, OP_GET).unwrap();
    assert_eq!(status, STATUS_NOT_FOUND);
    assert!(value.is_empty());
}

#[test]
fn put_overwrite_then_get_returns_new_value() {
    let mut svc = KeyValueStore::new();

    // PUT k=v1
    let req = encode_request(OP_PUT, b"k", b"v1").unwrap();
    svc.handle(&req);

    // PUT k=v2 (overwrite)
    let req = encode_request(OP_PUT, b"k", b"v2").unwrap();
    let rsp = svc.handle(&req);
    let (status, _) = decode_response(&rsp, OP_PUT).unwrap();
    assert_eq!(status, STATUS_OK);

    // GET k → v2
    let req = encode_request(OP_GET, b"k", &[]).unwrap();
    let rsp = svc.handle(&req);
    let (status, value) = decode_response(&rsp, OP_GET).unwrap();
    assert_eq!(status, STATUS_OK);
    assert_eq!(value, b"v2");
}

#[test]
fn del_existing_then_get_returns_not_found() {
    let mut svc = KeyValueStore::new();

    // PUT k1=v1
    let req = encode_request(OP_PUT, b"k1", b"v1").unwrap();
    svc.handle(&req);

    // DEL k1
    let req = encode_request(OP_DEL, b"k1", &[]).unwrap();
    let rsp = svc.handle(&req);
    let (status, _) = decode_response(&rsp, OP_DEL).unwrap();
    assert_eq!(status, STATUS_OK);

    // GET k1 → NOT_FOUND
    let req = encode_request(OP_GET, b"k1", &[]).unwrap();
    let rsp = svc.handle(&req);
    let (status, _) = decode_response(&rsp, OP_GET).unwrap();
    assert_eq!(status, STATUS_NOT_FOUND);
}

#[test]
fn del_nonexistent_returns_not_found() {
    let mut svc = KeyValueStore::new();

    let req = encode_request(OP_DEL, b"ghost", &[]).unwrap();
    let rsp = svc.handle(&req);
    let (status, _) = decode_response(&rsp, OP_DEL).unwrap();
    assert_eq!(status, STATUS_NOT_FOUND);
}

#[test]
fn put_empty_value_then_get_returns_empty() {
    let mut svc = KeyValueStore::new();

    let req = encode_request(OP_PUT, b"empty", b"").unwrap();
    let rsp = svc.handle(&req);
    let (status, _) = decode_response(&rsp, OP_PUT).unwrap();
    assert_eq!(status, STATUS_OK);

    let req = encode_request(OP_GET, b"empty", &[]).unwrap();
    let rsp = svc.handle(&req);
    let (status, value) = decode_response(&rsp, OP_GET).unwrap();
    assert_eq!(status, STATUS_OK);
    assert!(value.is_empty());
}

#[test]
fn multiple_independent_keys() {
    let mut svc = KeyValueStore::new();

    for i in 0u8..5u8 {
        let key = [b'k', i];
        let val = [b'v', i];
        let req = encode_request(OP_PUT, &key, &val).unwrap();
        let rsp = svc.handle(&req);
        assert_eq!(decode_response(&rsp, OP_PUT).unwrap().0, STATUS_OK);
    }

    for i in 0u8..5u8 {
        let key = [b'k', i];
        let val = [b'v', i];
        let req = encode_request(OP_GET, &key, &[]).unwrap();
        let rsp = svc.handle(&req);
        let (status, value) = decode_response(&rsp, OP_GET).unwrap();
        assert_eq!(status, STATUS_OK);
        assert_eq!(value, &val);
    }
}

#[test]
fn key_too_long_returns_too_large() {
    let mut svc = KeyValueStore::new();
    let long_key = vec![b'x'; MAX_KEY_LEN + 1];

    // encode_request should reject it at the client side
    assert!(encode_request(OP_PUT, &long_key, b"").is_err());

    // But a malformed frame with bad length field should be caught server-side
    let mut bad = vec![protocol::MAGIC0, protocol::MAGIC1, protocol::VERSION, OP_PUT];
    bad.push((MAX_KEY_LEN + 1) as u8); // key_len overflow as u8
    bad.extend_from_slice(&0u16.to_le_bytes()); // val_len=0
    bad.extend(std::iter::repeat(b'x').take(MAX_KEY_LEN + 1));

    let rsp = svc.handle(&bad);
    let (status, _) = decode_response(&rsp, OP_PUT).unwrap();
    assert_eq!(status, STATUS_TOO_LARGE);
}

#[test]
fn response_flag_is_set_on_all_responses() {
    let mut svc = KeyValueStore::new();

    let ops = [OP_PUT, OP_GET, OP_DEL];
    for &op in &ops {
        let req = encode_request(op, b"t", b"").unwrap();
        let rsp = svc.handle(&req);
        assert!(rsp.len() >= 4, "response too short for op {op}");
        assert_eq!(rsp[3] & RESPONSE_FLAG, RESPONSE_FLAG, "response flag not set for op {op}");
    }
}

#[test]
fn unsupported_opcode_returns_unsupported_status() {
    let mut svc = KeyValueStore::new();

    // Use opcode 99 (not defined), with valid key_len/val_len to bypass header checks.
    let req = {
        let mut buf = vec![
            protocol::MAGIC0,
            protocol::MAGIC1,
            protocol::VERSION,
            99u8,
            1u8, // key_len=1
        ];
        buf.extend_from_slice(&0u16.to_le_bytes()); // val_len=0
        buf.push(b'k'); // 1 key byte
        buf
    };

    let rsp = svc.handle(&req);
    let (status, _) = decode_response(&rsp, 99).unwrap();
    assert_eq!(status, STATUS_UNSUPPORTED);
}
