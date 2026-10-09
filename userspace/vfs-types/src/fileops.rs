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
/// Copy a file (`from`, `to` — same payload codec as rename).
pub const OP_COPY: u8 = 13;
/// Write a whole file's content from a VMO (TASK-0068, RFC-0095): `len` bytes
/// of the VMO this sender armed with [`OP_ARM_VMO`], from its offset 0, land
/// in `path` at offset 0 as ONE filesystem transaction. The file must exist
/// (`OP_CREATE` first); like `OP_WRITE_TEXT`, a longer existing file keeps its
/// tail. Payload: [`encode_write_vmo`].
///
/// The second of TWO messages — a message moves ONE capability: the arm moves
/// the VMO, this one moves the reply cap (`call_into`). The server takes the
/// armed VMO, closes it BEFORE it answers [`encode_status_reply`] (so the
/// client may destroy its VMO on the answer), and answers `Invalid` when
/// nothing is armed and `TooBig` when the VMO is shorter than `len`.
pub const OP_WRITE_VMO: u8 = 14;
/// Arm a VMO for this sender's next [`OP_WRITE_VMO`] (TASK-0068). Frame:
/// `[OP_ARM_VMO]` and nothing else; the moved capability IS the VMO — a plain
/// `cap_clone`, not a read-only alias (the server pulls the bytes with
/// `vmo_read`, which a read-only alias refuses). No reply. The server keys
/// it by the KERNEL sender id: a second arm replaces the first (closing it),
/// the table is bounded, and the `OP_WRITE_VMO` that follows consumes it.
pub const OP_ARM_VMO: u8 = 15;

/// Bounded inline text payload for `writeText` (RFC-0073 v1 small-text seam).
pub const MAX_INLINE_TEXT: usize = 4096;

/// Upper bound on an `OP_WRITE_VMO` length: the nxfs engine's per-file cap
/// (`MAX_FILE_BYTES`, 64 MiB). vfs-types sits below nxfs in the crate graph,
/// so the match is pinned by a test on the nxfs side.
pub const MAX_WRITE_VMO_BYTES: u32 = 64 * 1024 * 1024;

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

/// Decodes a `writeText` request into `(path, text)`, BORROWED from the
/// frame: the text can be 4 KiB, past the 2 KiB size classes a service heap
/// recycles, so an owned copy per write would stay on the heap for good.
pub fn decode_write_text(payload: &[u8]) -> Option<(&str, &str)> {
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
    Some((path, text))
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

/// Encodes an `OP_WRITE_VMO` request: `path_len u16 LE | path | len u32 LE`.
pub fn encode_write_vmo(path: &str, len: u32) -> Option<Vec<u8>> {
    if path.is_empty() || path.len() > MAX_PATH_LEN || len > MAX_WRITE_VMO_BYTES {
        return None;
    }
    let mut out = Vec::with_capacity(2 + path.len() + 4);
    out.extend_from_slice(&(path.len() as u16).to_le_bytes());
    out.extend_from_slice(path.as_bytes());
    out.extend_from_slice(&len.to_le_bytes());
    Some(out)
}

/// Decodes an `OP_WRITE_VMO` request into `(path, len)`. Fail-closed: the
/// payload is EXACTLY `2 + path_len + 4` bytes, the path non-empty, bounded
/// and UTF-8, and `len` within [`MAX_WRITE_VMO_BYTES`].
pub fn decode_write_vmo(payload: &[u8]) -> Option<(String, u32)> {
    if payload.len() < 2 {
        return None;
    }
    let path_len = u16::from_le_bytes([payload[0], payload[1]]) as usize;
    if path_len == 0 || path_len > MAX_PATH_LEN || payload.len() != 2 + path_len + 4 {
        return None;
    }
    let path = core::str::from_utf8(&payload[2..2 + path_len]).ok()?;
    let len = u32::from_le_bytes(payload[2 + path_len..].try_into().ok()?);
    if len > MAX_WRITE_VMO_BYTES {
        return None;
    }
    Some((String::from(path), len))
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
        assert_eq!(decode_write_text(&payload), Some(("/notes.txt", "hello")));
    }

    #[test]
    fn rename_roundtrip() {
        let payload = encode_rename("/a", "/b").expect("encode");
        assert_eq!(decode_rename(&payload), Some(("/a".into(), "/b".into())));
    }

    #[test]
    fn write_vmo_roundtrip_and_boundaries() {
        let path = "/Bilder/Bildschirmfoto.png";
        let payload = encode_write_vmo(path, 1_234_567).expect("encode");
        assert_eq!(payload.len(), 2 + path.len() + 4);
        assert_eq!(decode_write_vmo(&payload), Some((path.into(), 1_234_567)));
        // An empty file, the length cap and the path cap are all valid.
        let zero = encode_write_vmo("/a", 0).expect("zero length");
        assert_eq!(decode_write_vmo(&zero), Some(("/a".into(), 0)));
        let cap = encode_write_vmo("/a", MAX_WRITE_VMO_BYTES).expect("length cap");
        assert_eq!(decode_write_vmo(&cap), Some(("/a".into(), MAX_WRITE_VMO_BYTES)));
        let longest = "x".repeat(MAX_PATH_LEN);
        let payload = encode_write_vmo(&longest, 1).expect("path cap");
        assert_eq!(decode_write_vmo(&payload), Some((longest, 1)));
    }

    #[test]
    fn test_reject_write_vmo_truncated() {
        let payload = encode_write_vmo("/a.png", 7).expect("encode");
        for cut in 0..payload.len() {
            assert_eq!(decode_write_vmo(&payload[..cut]), None, "cut at {cut}");
        }
    }

    #[test]
    fn test_reject_write_vmo_path_len_lies() {
        let honest = encode_write_vmo("/a.png", 7).expect("encode");
        for lie in [honest[0] + 1, honest[0] - 1] {
            let mut payload = honest.clone();
            payload[0] = lie;
            assert_eq!(decode_write_vmo(&payload), None, "path_len {lie}");
        }
        // A path_len past MAX_PATH_LEN is refused even when the bytes are there.
        let mut huge = ((MAX_PATH_LEN + 1) as u16).to_le_bytes().to_vec();
        huge.resize(2 + MAX_PATH_LEN + 1 + 4, b'x');
        assert_eq!(decode_write_vmo(&huge), None);
        assert!(encode_write_vmo(&"x".repeat(MAX_PATH_LEN + 1), 7).is_none());
    }

    #[test]
    fn test_reject_write_vmo_zero_path() {
        assert_eq!(decode_write_vmo(&[0, 0, 7, 0, 0, 0]), None);
        assert!(encode_write_vmo("", 7).is_none());
    }

    #[test]
    fn test_reject_write_vmo_over_cap_len() {
        for len in [MAX_WRITE_VMO_BYTES + 1, u32::MAX] {
            let mut payload = encode_write_vmo("/a.png", 0).expect("encode");
            let at = payload.len() - 4;
            payload[at..].copy_from_slice(&len.to_le_bytes());
            assert_eq!(decode_write_vmo(&payload), None, "len {len}");
            assert!(encode_write_vmo("/a.png", len).is_none());
        }
    }

    #[test]
    fn test_reject_write_vmo_trailing_bytes() {
        let mut payload = encode_write_vmo("/a.png", 7).expect("encode");
        payload.push(0);
        assert_eq!(decode_write_vmo(&payload), None);
    }

    #[test]
    fn test_reject_write_vmo_non_utf8_path() {
        assert_eq!(decode_write_vmo(&[2, 0, 0xFF, 0xFE, 7, 0, 0, 0]), None);
    }

    /// The opcode space is shared by vfsd, nxfsd and the app-host, and its
    /// constants live in two modules: no two ops may share a byte.
    #[test]
    fn opcodes_are_unique() {
        let ops = [
            OP_OPEN,
            OP_READ,
            OP_CLOSE,
            OP_STAT,
            OP_MOUNT,
            OP_READDIR,
            crate::OP_READ_VMO,
            OP_MKDIR,
            OP_CREATE,
            OP_WRITE_TEXT,
            OP_REMOVE,
            OP_RENAME,
            OP_COPY,
            OP_WRITE_VMO,
            OP_ARM_VMO,
        ];
        for (i, a) in ops.iter().enumerate() {
            assert!(!ops[i + 1..].contains(a), "opcode {a} is used twice");
        }
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
