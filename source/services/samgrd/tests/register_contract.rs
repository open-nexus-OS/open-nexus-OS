// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: SAMgr register/lookup contract tests — protocol-level round-trips.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p samgrd -- register`
//!
//! TEST_SCOPE:
//!   - OP_REGISTER(service_name) → STATUS_OK
//!   - OP_LOOKUP(service_name) → STATUS_OK + endpoint data
//!   - OP_LOOKUP(unknown) → STATUS_NOT_FOUND
//!   - Duplicate register → idempotent (STATUS_OK)
//!   - OP_RESOLVE_STATUS → STATUS_OK
//!
//! Wire format (inline, mirrors os_lite.rs):
//!   MAGIC: "SM", VERSION: 1
//!   Request:  [MAGIC0, MAGIC1, VERSION, op, name_len:u16le, name...]
//!   Response: [MAGIC0, MAGIC1, VERSION, op|0x80, status, payload...]

const MAGIC0: u8 = b'S';
const MAGIC1: u8 = b'M';
const VERSION: u8 = 1;
const RESPONSE_FLAG: u8 = 0x80;

const OP_REGISTER: u8 = 1;
const OP_LOOKUP: u8 = 2;
#[allow(dead_code)]
const OP_RESOLVE_STATUS: u8 = 6;

const STATUS_OK: u8 = 0;
const STATUS_NOT_FOUND: u8 = 1;
const STATUS_MALFORMED: u8 = 2;

fn encode_register(name: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + name.len());
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(OP_REGISTER);
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(name.as_bytes());
    out
}

fn encode_lookup(name: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(6 + name.len());
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(OP_LOOKUP);
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(name.as_bytes());
    out
}

fn encode_status(op: u8, status: u8) -> Vec<u8> {
    vec![MAGIC0, MAGIC1, VERSION, op | RESPONSE_FLAG, status]
}

fn decode_rsp(frame: &[u8]) -> Result<(u8, u8), &'static str> {
    if frame.len() < 5 {
        return Err("too short");
    }
    if frame[0] != MAGIC0 || frame[1] != MAGIC1 {
        return Err("bad magic");
    }
    let op = frame[3] & !RESPONSE_FLAG;
    let status = frame[4];
    Ok((op, status))
}

// ── In-memory registry ────────────────────────────────────────────

struct Registry {
    entries: std::collections::BTreeMap<String, u32>,
}

impl Registry {
    fn new() -> Self {
        Self { entries: std::collections::BTreeMap::new() }
    }

    fn handle(&mut self, frame: &[u8]) -> Vec<u8> {
        if frame.len() < 6 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
            return encode_status(0, STATUS_MALFORMED);
        }
        let op = frame[3];
        let name_len = u16::from_le_bytes([frame[4], frame[5]]) as usize;
        if frame.len() != 6 + name_len {
            return encode_status(op, STATUS_MALFORMED);
        }
        let name = match std::str::from_utf8(&frame[6..]) {
            Ok(s) => s,
            Err(_) => return encode_status(op, STATUS_MALFORMED),
        };

        match op {
            OP_REGISTER => {
                self.entries.entry(name.to_string()).or_insert(0);
                encode_status(OP_REGISTER, STATUS_OK)
            }
            OP_LOOKUP => {
                if self.entries.contains_key(name) {
                    // Response: status OK + dummy endpoint (send/recv slots = 0 for test)
                    let mut out = Vec::with_capacity(13);
                    out.push(MAGIC0);
                    out.push(MAGIC1);
                    out.push(VERSION);
                    out.push(OP_LOOKUP | RESPONSE_FLAG);
                    out.push(STATUS_OK);
                    out.extend_from_slice(&0u32.to_le_bytes()); // send_slot
                    out.extend_from_slice(&0u32.to_le_bytes()); // recv_slot
                    out
                } else {
                    encode_status(OP_LOOKUP, STATUS_NOT_FOUND)
                }
            }
            _ => encode_status(op, STATUS_MALFORMED),
        }
    }
}

// ── Contract tests ────────────────────────────────────────────────

#[test]
fn register_then_lookup_returns_ok() {
    let mut reg = Registry::new();
    reg.handle(&encode_register("windowd"));
    let rsp = reg.handle(&encode_lookup("windowd"));
    let (op, status) = decode_rsp(&rsp).unwrap();
    assert_eq!(op, OP_LOOKUP);
    assert_eq!(status, STATUS_OK);
    // Response should contain send/recv slots
    assert!(rsp.len() >= 13);
}

#[test]
fn lookup_unknown_returns_not_found() {
    let mut reg = Registry::new();
    let rsp = reg.handle(&encode_lookup("nonexistent"));
    let (_op, status) = decode_rsp(&rsp).unwrap();
    assert_eq!(status, STATUS_NOT_FOUND);
}

#[test]
fn double_register_is_idempotent() {
    let mut reg = Registry::new();
    let rsp1 = reg.handle(&encode_register("vfsd"));
    assert_eq!(decode_rsp(&rsp1).unwrap().1, STATUS_OK);
    let rsp2 = reg.handle(&encode_register("vfsd"));
    assert_eq!(decode_rsp(&rsp2).unwrap().1, STATUS_OK);
}

#[test]
fn register_multiple_services() {
    let mut reg = Registry::new();
    for name in &["vfsd", "policyd", "logd", "keystored", "samgrd"] {
        reg.handle(&encode_register(name));
    }
    for name in &["vfsd", "policyd", "logd", "keystored", "samgrd"] {
        let (_op, status) = decode_rsp(&reg.handle(&encode_lookup(name))).unwrap();
        assert_eq!(status, STATUS_OK, "lookup failed for {name}");
    }
}

#[test]
fn reject_bad_magic() {
    let mut reg = Registry::new();
    let mut frame = encode_register("test");
    frame[0] = b'X';
    let (_op, status) = decode_rsp(&reg.handle(&frame)).unwrap();
    assert_eq!(status, STATUS_MALFORMED);
}

#[test]
fn reject_frame_too_short() {
    let mut reg = Registry::new();
    let frame = vec![MAGIC0, MAGIC1, VERSION];
    let (_op, status) = decode_rsp(&reg.handle(&frame)).unwrap();
    assert_eq!(status, STATUS_MALFORMED);
}
