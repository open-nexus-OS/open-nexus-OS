// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The file-op opcode SSOT + write-request codecs shared by vfsd,
//! nxfsd, and the app-host (RFC-0072 Phase 2). vfsd routes a mount path to a
//! provider and forwards these frames; one codec keeps the three ends in
//! lockstep. Bounded on every field — malformed frames decode to `None`.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0293)
//! TEST_COVERAGE: roundtrip + reject tests below

use alloc::string::String;
use alloc::vec::Vec;

use crate::entry::MAX_PATH_LEN;

/// Frame opcode (byte 0). Read ops (OPEN/READ/CLOSE/STAT/READDIR) keep their
/// vfsd bring-up numbers; write ops continue the sequence (RFC-0072 Phase 2).
pub const OP_OPEN: u8 = 1;
pub const OP_READ: u8 = 2;
pub const OP_CLOSE: u8 = 3;
pub const OP_STAT: u8 = 4;
pub const OP_MOUNT: u8 = 5;
pub const OP_READDIR: u8 = 6;
pub const OP_MKDIR: u8 = 8;
pub const OP_CREATE: u8 = 9;
pub const OP_WRITE_TEXT: u8 = 10;
pub const OP_REMOVE: u8 = 11;
pub const OP_RENAME: u8 = 12;

/// Bounded inline text payload for `writeText` (RFC-0073 v1 small-text seam).
pub const MAX_INLINE_TEXT: usize = 4096;

/// Encodes a single-path write request (`mkdir`/`create`/`remove`) — payload
/// is the path bytes (no opcode; the caller prepends it).
pub fn encode_path_request(path: &str) -> Option<Vec<u8>> {
    if path.is_empty() || path.len() > MAX_PATH_LEN {
        return None;
    }
    Some(path.as_bytes().to_vec())
}

/// Decodes a single-path write request payload.
pub fn decode_path_request(payload: &[u8]) -> Option<String> {
    if payload.is_empty() || payload.len() > MAX_PATH_LEN {
        return None;
    }
    core::str::from_utf8(payload).ok().map(String::from)
}

/// Encodes a `writeText` request: `path_len u16 | path | text`.
pub fn encode_write_text(path: &str, text: &str) -> Option<Vec<u8>> {
    if path.is_empty() || path.len() > MAX_PATH_LEN || text.len() > MAX_INLINE_TEXT {
        return None;
    }
    let mut out = Vec::with_capacity(2 + path.len() + text.len());
    out.extend_from_slice(&(path.len() as u16).to_le_bytes());
    out.extend_from_slice(path.as_bytes());
    out.extend_from_slice(text.as_bytes());
    Some(out)
}

/// Decodes a `writeText` request into `(path, text)`.
pub fn decode_write_text(payload: &[u8]) -> Option<(String, String)> {
    if payload.len() < 2 {
        return None;
    }
    let path_len = u16::from_le_bytes([payload[0], payload[1]]) as usize;
    if path_len == 0 || path_len > MAX_PATH_LEN || payload.len() < 2 + path_len {
        return None;
    }
    let path = core::str::from_utf8(&payload[2..2 + path_len]).ok()?;
    let text = core::str::from_utf8(&payload[2 + path_len..]).ok()?;
    if text.len() > MAX_INLINE_TEXT {
        return None;
    }
    Some((String::from(path), String::from(text)))
}

/// Encodes a `rename` request: `from_len u16 | from | to`.
pub fn encode_rename(from: &str, to: &str) -> Option<Vec<u8>> {
    if from.is_empty() || from.len() > MAX_PATH_LEN || to.is_empty() || to.len() > MAX_PATH_LEN {
        return None;
    }
    let mut out = Vec::with_capacity(2 + from.len() + to.len());
    out.extend_from_slice(&(from.len() as u16).to_le_bytes());
    out.extend_from_slice(from.as_bytes());
    out.extend_from_slice(to.as_bytes());
    Some(out)
}

/// Decodes a `rename` request into `(from, to)`.
pub fn decode_rename(payload: &[u8]) -> Option<(String, String)> {
    if payload.len() < 2 {
        return None;
    }
    let from_len = u16::from_le_bytes([payload[0], payload[1]]) as usize;
    if from_len == 0 || from_len > MAX_PATH_LEN || payload.len() < 2 + from_len {
        return None;
    }
    let from = core::str::from_utf8(&payload[2..2 + from_len]).ok()?;
    let to = core::str::from_utf8(&payload[2 + from_len..]).ok()?;
    if to.is_empty() || to.len() > MAX_PATH_LEN {
        return None;
    }
    Some((String::from(from), String::from(to)))
}

/// A write-op reply: `[status u16 LE]` (RFC-0072 error codes; 0 = OK).
pub fn encode_status_reply(err: u16) -> Vec<u8> {
    err.to_le_bytes().to_vec()
}

/// Decodes a write-op reply into the RFC-0072 error code.
pub fn decode_status_reply(frame: &[u8]) -> Option<u16> {
    (frame.len() == 2).then(|| u16::from_le_bytes([frame[0], frame[1]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_request_roundtrip() {
        let payload = encode_path_request("/photos/2026").expect("encode");
        assert_eq!(decode_path_request(&payload).as_deref(), Some("/photos/2026"));
        assert!(encode_path_request("").is_none());
    }

    #[test]
    fn write_text_roundtrip() {
        let payload = encode_write_text("/notes.txt", "hello").expect("encode");
        assert_eq!(
            decode_write_text(&payload),
            Some(("/notes.txt".into(), "hello".into()))
        );
    }

    #[test]
    fn rename_roundtrip() {
        let payload = encode_rename("/a", "/b").expect("encode");
        assert_eq!(decode_rename(&payload), Some(("/a".into(), "/b".into())));
    }

    #[test]
    fn status_reply_roundtrip() {
        assert_eq!(decode_status_reply(&encode_status_reply(0)), Some(0));
        assert_eq!(decode_status_reply(&encode_status_reply(9)), Some(9));
        assert_eq!(decode_status_reply(&[1, 2, 3]), None);
    }

    #[test]
    fn test_reject_malformed() {
        assert!(decode_write_text(&[0]).is_none());
        assert!(decode_write_text(&[10, 0, b'x']).is_none()); // path_len exceeds
        assert!(decode_rename(&[5, 0, b'a']).is_none());
        let long = "x".repeat(MAX_PATH_LEN + 1);
        assert!(encode_path_request(&long).is_none());
    }
}
