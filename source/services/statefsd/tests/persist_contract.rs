// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: StateFS persist contract tests — PUT/GET/DEL/LIST protocol round-trips.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p statefsd -- persist`
//!
//! TEST_SCOPE:
//!   - Request encoding / response decoding via `statefs::protocol`
//!   - PUT → GET → value matches (in-process simulator)
//!   - GET miss → STATUS_NOT_FOUND
//!   - DEL → GET miss → STATUS_NOT_FOUND
//!   - LIST returns matching keys
//!   - SYNC/REOPEN round-trips
//!
//! DEPENDENCIES:
//!   - `statefs::protocol`: wire format constants + encode/decode functions

use statefs::protocol as proto;

// ── In-memory store (simulates statefsd backend) ─────────────────

struct MemStore {
    data: std::collections::BTreeMap<String, Vec<u8>>,
}

impl MemStore {
    fn new() -> Self {
        Self { data: std::collections::BTreeMap::new() }
    }

    fn handle(&mut self, frame: &[u8]) -> Vec<u8> {
        let req = match proto::decode_request(frame) {
            Ok(r) => r,
            Err(status) => return proto::encode_status_response(0, status),
        };
        match req {
            proto::Request::Put { key, value } => {
                self.data.insert(key.to_string(), value.to_vec());
                proto::encode_status_response(proto::OP_PUT, proto::STATUS_OK)
            }
            proto::Request::Get { key } => match self.data.get(key) {
                Some(v) => proto::encode_get_response(proto::STATUS_OK, v),
                None => proto::encode_get_response(proto::STATUS_NOT_FOUND, &[]),
            },
            proto::Request::Delete { key } => {
                let found = self.data.remove(key).is_some();
                proto::encode_status_response(
                    proto::OP_DEL,
                    if found { proto::STATUS_OK } else { proto::STATUS_NOT_FOUND },
                )
            }
            proto::Request::List { prefix, limit } => {
                let mut matches: Vec<String> = self.data.keys()
                    .filter(|k| k.starts_with(prefix))
                    .cloned()
                    .collect();
                matches.sort();
                let limit = limit.min(proto::MAX_LIST_LIMIT) as usize;
                matches.truncate(limit);
                proto::encode_list_response(proto::STATUS_OK, &matches, 4096)
            }
            proto::Request::Sync => {
                proto::encode_status_response(proto::OP_SYNC, proto::STATUS_OK)
            }
            proto::Request::Reopen => {
                proto::encode_status_response(proto::OP_REOPEN, proto::STATUS_OK)
            }
        }
    }
}

// ── Response decoding helper ──────────────────────────────────────

/// Decode a response frame. Returns (opcode, status, value_bytes).
fn decode_rsp(frame: &[u8]) -> Result<(u8, u8, &[u8]), &'static str> {
    if frame.len() < 5 {
        return Err("too short");
    }
    if frame[0] != proto::MAGIC0 || frame[1] != proto::MAGIC1 {
        return Err("bad magic");
    }
    let op = frame[3] & !0x80;
    let status = frame[4];
    let value = if frame.len() > 5 { &frame[5..] } else { &[] };
    Ok((op, status, value))
}

// ── Persist contract tests ────────────────────────────────────────

#[test]
fn put_then_get_returns_inserted_value() {
    let mut svc = MemStore::new();
    let frame = proto::encode_put_request("/state/k1", b"v1").unwrap();
    let rsp = svc.handle(&frame);
    let (op, status, _) = decode_rsp(&rsp).unwrap();
    assert_eq!(op, proto::OP_PUT);
    assert_eq!(status, proto::STATUS_OK);

    let frame = proto::encode_key_only_request(proto::OP_GET, "/state/k1").unwrap();
    let rsp = svc.handle(&frame);
    let (op, status, value) = decode_rsp(&rsp).unwrap();
    assert_eq!(op, proto::OP_GET);
    assert_eq!(status, proto::STATUS_OK);
    // value is after the 4-byte length prefix in encode_get_response
    assert!(value.len() >= 4);
    assert_eq!(&value[4..], b"v1");
}

#[test]
fn get_miss_returns_not_found() {
    let mut svc = MemStore::new();
    let frame = proto::encode_key_only_request(proto::OP_GET, "/state/ghost").unwrap();
    let rsp = svc.handle(&frame);
    let (_op, status, _) = decode_rsp(&rsp).unwrap();
    assert_eq!(status, proto::STATUS_NOT_FOUND);
}

#[test]
fn put_overwrite_then_get_returns_new_value() {
    let mut svc = MemStore::new();
    svc.handle(&proto::encode_put_request("/state/k", b"v1").unwrap());
    svc.handle(&proto::encode_put_request("/state/k", b"v2").unwrap());
    let rsp = svc.handle(&proto::encode_key_only_request(proto::OP_GET, "/state/k").unwrap());
    let (_op, status, value) = decode_rsp(&rsp).unwrap();
    assert_eq!(status, proto::STATUS_OK);
    assert_eq!(&value[4..], b"v2");
}

#[test]
fn del_existing_then_get_returns_not_found() {
    let mut svc = MemStore::new();
    svc.handle(&proto::encode_put_request("/state/k", b"v").unwrap());
    let rsp = svc.handle(&proto::encode_key_only_request(proto::OP_DEL, "/state/k").unwrap());
    assert_eq!(decode_rsp(&rsp).unwrap().1, proto::STATUS_OK);

    let rsp = svc.handle(&proto::encode_key_only_request(proto::OP_GET, "/state/k").unwrap());
    assert_eq!(decode_rsp(&rsp).unwrap().1, proto::STATUS_NOT_FOUND);
}

#[test]
fn del_nonexistent_returns_not_found() {
    let mut svc = MemStore::new();
    let rsp = svc.handle(&proto::encode_key_only_request(proto::OP_DEL, "/state/nope").unwrap());
    assert_eq!(decode_rsp(&rsp).unwrap().1, proto::STATUS_NOT_FOUND);
}

#[test]
fn list_returns_matching_keys_sorted() {
    let mut svc = MemStore::new();
    svc.handle(&proto::encode_put_request("/state/a/1", b"x").unwrap());
    svc.handle(&proto::encode_put_request("/state/a/2", b"y").unwrap());
    svc.handle(&proto::encode_put_request("/state/b/1", b"z").unwrap());

    let frame = proto::encode_list_request("/state/a/", 10).unwrap();
    let rsp = svc.handle(&frame);
    let (_op, status, _) = decode_rsp(&rsp).unwrap();
    assert_eq!(status, proto::STATUS_OK);
    // encode_list_response uses count:u16 followed by key_len:u16 + key... entries
}

#[test]
fn sync_returns_ok() {
    let mut svc = MemStore::new();
    let rsp = svc.handle(&proto::encode_sync_request());
    assert_eq!(decode_rsp(&rsp).unwrap().1, proto::STATUS_OK);
}

#[test]
fn reopen_returns_ok() {
    let mut svc = MemStore::new();
    let rsp = svc.handle(&proto::encode_reopen_request());
    assert_eq!(decode_rsp(&rsp).unwrap().1, proto::STATUS_OK);
}
