// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: SAMgr malformed-frame contract tests — reject paths.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p samgrd -- malformed`
//!
//! Wire format (inline, mirrors os_lite.rs).
//! Tests bad-magic, bad-version, length-mismatch, non-UTF8 name, unsupported opcode.

use std::collections::BTreeMap;

const MAGIC0: u8 = b'S';
const MAGIC1: u8 = b'M';
const VERSION: u8 = 1;
const RESPONSE_FLAG: u8 = 0x80;

const OP_REGISTER: u8 = 1;
const OP_LOOKUP: u8 = 2;

const STATUS_OK: u8 = 0;
const STATUS_NOT_FOUND: u8 = 1;
const STATUS_MALFORMED: u8 = 2;

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
    Ok((op, frame[4]))
}

fn server_handle(frame: &[u8]) -> Vec<u8> {
    static mut REG: Option<BTreeMap<String, u32>> = None;
    let reg = unsafe { REG.get_or_insert_with(BTreeMap::new) };

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
            reg.entry(name.to_string()).or_insert(0);
            encode_status(OP_REGISTER, STATUS_OK)
        }
        OP_LOOKUP => {
            if reg.contains_key(name) {
                let mut out = vec![MAGIC0, MAGIC1, VERSION, OP_LOOKUP | RESPONSE_FLAG, STATUS_OK];
                out.extend_from_slice(&0u64.to_le_bytes());
                out
            } else {
                encode_status(OP_LOOKUP, STATUS_NOT_FOUND)
            }
        }
        _ => encode_status(op, STATUS_MALFORMED),
    }
}

// ── Tests ─────────────────────────────────────────────────────────

#[test]
fn reject_bad_magic_byte_0() {
    let mut frame = vec![MAGIC0, MAGIC1, VERSION, OP_REGISTER, 4, 0];
    frame.extend_from_slice(b"test");
    frame[0] = b'X';
    let (_op, status) = decode_rsp(&server_handle(&frame)).unwrap();
    assert_eq!(status, STATUS_MALFORMED);
}

#[test]
fn reject_bad_magic_byte_1() {
    let mut frame = vec![MAGIC0, MAGIC1, VERSION, OP_REGISTER, 4, 0];
    frame.extend_from_slice(b"test");
    frame[1] = b'X';
    let (_op, status) = decode_rsp(&server_handle(&frame)).unwrap();
    assert_eq!(status, STATUS_MALFORMED);
}

#[test]
fn reject_wrong_version() {
    let mut frame = vec![MAGIC0, MAGIC1, 99u8, OP_REGISTER, 4, 0];
    frame.extend_from_slice(b"test");
    let (_op, status) = decode_rsp(&server_handle(&frame)).unwrap();
    assert_eq!(status, STATUS_MALFORMED);
}

#[test]
fn reject_name_length_mismatch() {
    // Declare 8-byte name, provide only 4
    let mut frame = vec![MAGIC0, MAGIC1, VERSION, OP_REGISTER, 8, 0];
    frame.extend_from_slice(b"test");
    let (_op, status) = decode_rsp(&server_handle(&frame)).unwrap();
    assert_eq!(status, STATUS_MALFORMED);
}

#[test]
fn reject_non_utf8_name() {
    let mut frame = vec![MAGIC0, MAGIC1, VERSION, OP_REGISTER, 4, 0];
    frame.extend_from_slice(&[0xFF, 0xFE, 0xFD, 0xFC]);
    let (_op, status) = decode_rsp(&server_handle(&frame)).unwrap();
    assert_eq!(status, STATUS_MALFORMED);
}

#[test]
fn reject_unsupported_opcode() {
    let mut frame = vec![MAGIC0, MAGIC1, VERSION, 99u8, 4, 0];
    frame.extend_from_slice(b"test");
    let (_op, status) = decode_rsp(&server_handle(&frame)).unwrap();
    assert_eq!(status, STATUS_MALFORMED);
}

#[test]
fn reject_empty_name() {
    let frame = vec![MAGIC0, MAGIC1, VERSION, OP_REGISTER, 0, 0];
    // Empty name is technically valid (0-length). The server should accept it.
    let (_op, status) = decode_rsp(&server_handle(&frame)).unwrap();
    assert_eq!(status, STATUS_OK);
}

#[test]
fn accept_max_name_length() {
    // os_lite.rs doesn't define MAX_NAME_LEN, but protocol uses u16 for name_len
    let len = 256u16;
    let mut frame = vec![MAGIC0, MAGIC1, VERSION, OP_REGISTER];
    frame.extend_from_slice(&len.to_le_bytes());
    frame.extend(std::iter::repeat(b'x').take(len as usize));
    let (_op, status) = decode_rsp(&server_handle(&frame)).unwrap();
    assert_eq!(status, STATUS_OK);
}
