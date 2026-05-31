// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Keystored wire protocol constants + encoding/decoding (host + OS).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable (v1.0)
//! TEST_COVERAGE: 20 unit tests (host-compilable)
//! ADR: docs/adr/0017-service-architecture.md
//!
//! This module is available on both host (`std`) and OS (`os-lite`) builds
//! so that contract tests can verify the protocol layer without QEMU.

extern crate alloc;
use alloc::vec::Vec;

/// Protocol magic bytes.
pub const MAGIC0: u8 = b'K';
pub const MAGIC1: u8 = b'S';
pub const VERSION: u8 = 1;

/// Operation codes.
pub const OP_PUT: u8 = 1;
pub const OP_GET: u8 = 2;
pub const OP_DEL: u8 = 3;
pub const OP_VERIFY: u8 = 4;
pub const OP_SIGN: u8 = 5;
pub const OP_PUBKEY: u8 = 6;
pub const OP_CAPMOVE: u8 = 7;
pub const OP_DEVICE_KEYGEN: u8 = 10;
pub const OP_GET_DEVICE_PUBKEY: u8 = 11;
pub const OP_DEVICE_SIGN: u8 = 12;
pub const OP_GET_DEVICE_PRIVKEY: u8 = 13;
pub const OP_DEVICE_RELOAD: u8 = 14;

/// Status codes.
pub const STATUS_OK: u8 = 0;
pub const STATUS_NOT_FOUND: u8 = 1;
pub const STATUS_MALFORMED: u8 = 2;
pub const STATUS_TOO_LARGE: u8 = 3;
pub const STATUS_UNSUPPORTED: u8 = 4;
pub const STATUS_DENY: u8 = 5;
pub const STATUS_KEY_EXISTS: u8 = 10;
pub const STATUS_KEY_NOT_FOUND: u8 = 11;
pub const STATUS_PRIVATE_EXPORT_DENIED: u8 = 12;

/// Maximum lengths.
pub const MAX_KEY_LEN: usize = 64;
pub const MAX_VAL_LEN: usize = 256;

/// Response flag OR-ed into the op byte.
pub const RESPONSE_FLAG: u8 = 0x80;

/// Errors produced by protocol decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    /// Frame too short to contain a valid header.
    FrameTooShort,
    /// Magic bytes do not match.
    BadMagic,
    /// Version mismatch.
    UnsupportedVersion,
    /// Opcode in response does not match the expected opcode.
    OpcodeMismatch,
    /// Key length exceeds maximum.
    KeyTooLong,
    /// Value length exceeds maximum.
    ValueTooLong,
    /// Frame length does not match the declared key+value lengths.
    LengthMismatch,
}

/// Encode a service request frame.
///
/// Request format: `[MAGIC0, MAGIC1, VERSION, op, key_len:u8, val_len:u16le, key..., val...]`
pub fn encode_request(op: u8, key: &[u8], val: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    if key.len() > MAX_KEY_LEN {
        return Err(ProtocolError::KeyTooLong);
    }
    if val.len() > MAX_VAL_LEN {
        return Err(ProtocolError::ValueTooLong);
    }
    let mut buf = Vec::with_capacity(7 + key.len() + val.len());
    buf.push(MAGIC0);
    buf.push(MAGIC1);
    buf.push(VERSION);
    buf.push(op);
    buf.push(key.len() as u8);
    buf.extend_from_slice(&(val.len() as u16).to_le_bytes());
    buf.extend_from_slice(key);
    buf.extend_from_slice(val);
    Ok(buf)
}

/// Encode a service response frame.
///
/// Response format: `[MAGIC0, MAGIC1, VERSION, op|0x80, status, val_len:u16le, val...]`
pub fn encode_response(op: u8, status: u8, value: &[u8]) -> Vec<u8> {
    let len = (value.len().min(u16::MAX as usize)) as u16;
    let mut out = Vec::with_capacity(7 + len as usize);
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(op | RESPONSE_FLAG);
    out.push(status);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&value[..len as usize]);
    out
}

/// Decode a service response frame.
///
/// Returns `(status, value)` on success, or a `ProtocolError` on failure.
pub fn decode_response(frame: &[u8], expect_op: u8) -> Result<(u8, &[u8]), ProtocolError> {
    if frame.len() < 7 {
        return Err(ProtocolError::FrameTooShort);
    }
    if frame[0] != MAGIC0 || frame[1] != MAGIC1 {
        return Err(ProtocolError::BadMagic);
    }
    if frame[2] != VERSION {
        return Err(ProtocolError::UnsupportedVersion);
    }
    let resp_op = frame[3];
    if resp_op != (expect_op | RESPONSE_FLAG) {
        return Err(ProtocolError::OpcodeMismatch);
    }
    let status = frame[4];
    let val_len = u16::from_le_bytes([frame[5], frame[6]]) as usize;
    let expected = 7usize.saturating_add(val_len);
    if frame.len() != expected {
        return Err(ProtocolError::LengthMismatch);
    }
    Ok((status, &frame[7..]))
}

/// Check whether a frame looks like a valid keystored request header.
pub fn is_valid_header(frame: &[u8]) -> bool {
    frame.len() >= 4 && frame[0] == MAGIC0 && frame[1] == MAGIC1 && frame[2] == VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Request encoding ──────────────────────────────────────────

    #[test]
    fn encode_put_request() {
        let req = encode_request(OP_PUT, b"k1", b"v1").unwrap();
        assert_eq!(&req[..4], &[MAGIC0, MAGIC1, VERSION, OP_PUT]);
        assert_eq!(req[4], 2); // key_len
        assert_eq!(u16::from_le_bytes([req[5], req[6]]), 2); // val_len
        assert_eq!(&req[7..9], b"k1");
        assert_eq!(&req[9..11], b"v1");
    }

    #[test]
    fn encode_get_request_with_empty_value() {
        let req = encode_request(OP_GET, b"mykey", &[]).unwrap();
        assert_eq!(&req[..4], &[MAGIC0, MAGIC1, VERSION, OP_GET]);
        assert_eq!(req[4], 5); // key_len
        assert_eq!(u16::from_le_bytes([req[5], req[6]]), 0); // val_len
    }

    #[test]
    fn encode_rejects_key_too_long() {
        let long_key = [b'x'; MAX_KEY_LEN + 1];
        assert_eq!(
            encode_request(OP_PUT, &long_key, b""),
            Err(ProtocolError::KeyTooLong)
        );
    }

    #[test]
    fn encode_rejects_value_too_long() {
        let long_val = [b'x'; MAX_VAL_LEN + 1];
        assert_eq!(
            encode_request(OP_PUT, b"k", &long_val),
            Err(ProtocolError::ValueTooLong)
        );
    }

    #[test]
    fn encode_accepts_max_key_and_value() {
        let key = [b'k'; MAX_KEY_LEN];
        let val = [b'v'; MAX_VAL_LEN];
        let req = encode_request(OP_PUT, &key, &val).unwrap();
        assert_eq!(req.len(), 7 + MAX_KEY_LEN + MAX_VAL_LEN);
    }

    // ── Response encoding/decoding ────────────────────────────────

    #[test]
    fn roundtrip_response_ok() {
        let rsp = encode_response(OP_PUT, STATUS_OK, b"result");
        let (status, value) = decode_response(&rsp, OP_PUT).unwrap();
        assert_eq!(status, STATUS_OK);
        assert_eq!(value, b"result");
    }

    #[test]
    fn roundtrip_response_not_found() {
        let rsp = encode_response(OP_GET, STATUS_NOT_FOUND, &[]);
        let (status, value) = decode_response(&rsp, OP_GET).unwrap();
        assert_eq!(status, STATUS_NOT_FOUND);
        assert!(value.is_empty());
    }

    #[test]
    fn roundtrip_response_malformed() {
        let rsp = encode_response(OP_GET, STATUS_MALFORMED, &[]);
        let (status, _) = decode_response(&rsp, OP_GET).unwrap();
        assert_eq!(status, STATUS_MALFORMED);
    }

    #[test]
    fn decode_rejects_frame_too_short() {
        assert_eq!(
            decode_response(&[0u8; 4], OP_PUT),
            Err(ProtocolError::FrameTooShort)
        );
    }

    #[test]
    fn decode_rejects_bad_magic() {
        let mut rsp = encode_response(OP_PUT, STATUS_OK, &[]);
        rsp[0] = b'X';
        assert_eq!(decode_response(&rsp, OP_PUT), Err(ProtocolError::BadMagic));
    }

    #[test]
    fn decode_rejects_wrong_version() {
        let mut rsp = encode_response(OP_PUT, STATUS_OK, &[]);
        rsp[2] = 99;
        assert_eq!(
            decode_response(&rsp, OP_PUT),
            Err(ProtocolError::UnsupportedVersion)
        );
    }

    #[test]
    fn decode_rejects_opcode_mismatch() {
        let rsp = encode_response(OP_PUT, STATUS_OK, &[]);
        assert_eq!(
            decode_response(&rsp, OP_GET),
            Err(ProtocolError::OpcodeMismatch)
        );
    }

    #[test]
    fn decode_rejects_length_mismatch() {
        let mut rsp = encode_response(OP_PUT, STATUS_OK, b"hi");
        // Corrupt the length field to claim 5 bytes.
        rsp[5] = 5;
        rsp[6] = 0;
        assert_eq!(
            decode_response(&rsp, OP_PUT),
            Err(ProtocolError::LengthMismatch)
        );
    }

    // ── Header validation ─────────────────────────────────────────

    #[test]
    fn valid_header_accepts_minimal_frame() {
        assert!(is_valid_header(&[MAGIC0, MAGIC1, VERSION, OP_PUT]));
    }

    #[test]
    fn valid_header_rejects_short_frame() {
        assert!(!is_valid_header(&[MAGIC0, MAGIC1]));
    }

    #[test]
    fn valid_header_rejects_bad_magic() {
        assert!(!is_valid_header(&[b'X', MAGIC1, VERSION, OP_PUT]));
    }

    #[test]
    fn valid_header_rejects_wrong_version() {
        assert!(!is_valid_header(&[MAGIC0, MAGIC1, 99, OP_PUT]));
    }

    // ── Status / opcode constant uniqueness ───────────────────────

    #[test]
    fn opcodes_do_not_overlap_with_response_flag() {
        // No opcode should have the RESPONSE_FLAG bit set.
        let ops = [
            OP_PUT, OP_GET, OP_DEL, OP_VERIFY, OP_SIGN, OP_PUBKEY, OP_CAPMOVE,
            OP_DEVICE_KEYGEN, OP_GET_DEVICE_PUBKEY, OP_DEVICE_SIGN,
            OP_GET_DEVICE_PRIVKEY, OP_DEVICE_RELOAD,
        ];
        for &op in &ops {
            assert_eq!(op & RESPONSE_FLAG, 0, "opcode {op:#x} overlaps response flag");
        }
    }

    #[test]
    fn status_codes_are_distinct() {
        let statuses = [
            STATUS_OK, STATUS_NOT_FOUND, STATUS_MALFORMED, STATUS_TOO_LARGE,
            STATUS_UNSUPPORTED, STATUS_DENY, STATUS_KEY_EXISTS, STATUS_KEY_NOT_FOUND,
            STATUS_PRIVATE_EXPORT_DENIED,
        ];
        for i in 0..statuses.len() {
            for j in (i + 1)..statuses.len() {
                assert_ne!(statuses[i], statuses[j], "status codes must be unique");
            }
        }
    }
}
