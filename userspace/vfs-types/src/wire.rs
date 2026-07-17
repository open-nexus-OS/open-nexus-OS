// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The bounded raw wire codec for the ReadDir op. This exact payload
//! layout is spoken on BOTH raw hops of the os-lite chain (app-host/client →
//! vfsd, vfsd → packagefsd list) so vfsd can validate-and-relay pages without
//! re-encoding, and no hand-rolled copy can drift. Payloads exclude the
//! per-service opcode byte — that stays at each dispatch layer.
//!
//! Request payload:  `cursor u32 LE | limit u16 LE | path bytes (rest)`
//! Response payload: `status u16 LE | next_cursor u32 LE | eof u8 | count u16 LE |`
//!                   `count × ( kind u8 | size u64 LE | name_len u8 | name )`
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0291)
//! TEST_COVERAGE: roundtrip, byte-budget truncation, negative decode tests

use alloc::string::String;
use alloc::vec::Vec;

use crate::entry::{DirEntry, FileKind, MAX_ENTRIES_PER_PAGE, MAX_NAME_LEN, MAX_PATH_LEN};
use crate::error::{VfsError, CODE_OK};

/// Hard byte budget for an encoded response payload. Keeps the full frame
/// (opcode byte + payload) comfortably inside the 8 KiB IPC frame cap.
pub const MAX_READDIR_RESPONSE_BYTES: usize = 7 * 1024;

const REQ_HEADER: usize = 4 + 2;
const RSP_HEADER: usize = 2 + 4 + 1 + 2;
const ENTRY_FIXED: usize = 1 + 8 + 1;

/// Decoded ReadDir request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadDirRequest {
    /// Opaque continuation cursor (0 = first page).
    pub cursor: u32,
    /// Client page-size wish; servers clamp to [`MAX_ENTRIES_PER_PAGE`].
    pub limit: u16,
    /// Path to list.
    pub path: String,
}

/// Decoded ReadDir response page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadDirPage {
    /// Entries in canonical provider order.
    pub entries: Vec<DirEntry>,
    /// Cursor for the next page (meaningless when `eof`).
    pub next_cursor: u32,
    /// True when the listing is exhausted.
    pub eof: bool,
}

/// Encodes a ReadDir request payload. Fails with `Invalid` on oversize paths.
pub fn encode_readdir_request(path: &str, cursor: u32, limit: u16) -> Result<Vec<u8>, VfsError> {
    if path.is_empty() || path.len() > MAX_PATH_LEN {
        return Err(VfsError::Invalid);
    }
    let mut out = Vec::with_capacity(REQ_HEADER + path.len());
    out.extend_from_slice(&cursor.to_le_bytes());
    out.extend_from_slice(&limit.to_le_bytes());
    out.extend_from_slice(path.as_bytes());
    Ok(out)
}

/// Decodes a ReadDir request payload (server side). Bounded and fail-closed.
pub fn decode_readdir_request(payload: &[u8]) -> Result<ReadDirRequest, VfsError> {
    if payload.len() < REQ_HEADER + 1 {
        return Err(VfsError::Invalid);
    }
    let cursor = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
    let limit = u16::from_le_bytes([payload[4], payload[5]]);
    let path_bytes = &payload[REQ_HEADER..];
    if path_bytes.len() > MAX_PATH_LEN {
        return Err(VfsError::Invalid);
    }
    let path = core::str::from_utf8(path_bytes).map_err(|_| VfsError::Invalid)?;
    Ok(ReadDirRequest { cursor, limit, path: String::from(path) })
}

/// Encodes an error response payload.
#[must_use]
pub fn encode_readdir_error(err: VfsError) -> Vec<u8> {
    let mut out = Vec::with_capacity(RSP_HEADER);
    out.extend_from_slice(&err.code().to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.push(1);
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

/// Encodes a success page from `entries[start..]`, honoring the entry `limit`
/// and the byte budget. Returns the payload plus how many entries were
/// included; callers derive `next_cursor = start + included`.
///
/// Entries with oversize names are a server bug and yield `Invalid` rather
/// than a silently corrupted page.
pub fn encode_readdir_response(
    entries: &[DirEntry],
    start: usize,
    limit: u16,
    total_is_end: bool,
) -> Result<(Vec<u8>, usize), VfsError> {
    let limit = limit.clamp(1, MAX_ENTRIES_PER_PAGE) as usize;
    let slice = entries.get(start..).unwrap_or(&[]);
    let mut out = Vec::with_capacity(RSP_HEADER.min(MAX_READDIR_RESPONSE_BYTES));
    out.extend_from_slice(&CODE_OK.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // next_cursor patched below
    out.push(0); // eof patched below
    out.extend_from_slice(&0u16.to_le_bytes()); // count patched below

    let mut included = 0usize;
    for entry in slice.iter().take(limit) {
        let name = entry.name.as_bytes();
        if name.is_empty() || name.len() > MAX_NAME_LEN {
            return Err(VfsError::Invalid);
        }
        if out.len() + ENTRY_FIXED + name.len() > MAX_READDIR_RESPONSE_BYTES {
            break;
        }
        out.push(entry.kind as u8);
        out.extend_from_slice(&entry.size.to_le_bytes());
        out.push(name.len() as u8);
        out.extend_from_slice(name);
        included += 1;
    }

    let next = (start + included) as u32;
    let eof = total_is_end && start + included >= entries.len();
    out[2..6].copy_from_slice(&next.to_le_bytes());
    out[6] = u8::from(eof);
    out[7..9].copy_from_slice(&(included as u16).to_le_bytes());
    Ok((out, included))
}

/// Encodes an already-paginated [`ReadDirPage`] verbatim (for providers that
/// paginate internally, e.g. the nxfs engine). Same framing as
/// [`encode_readdir_response`]; honors the byte budget by truncating and
/// clearing `eof` if the page somehow overflows a frame.
pub fn encode_readdir_page(page: &ReadDirPage) -> Result<Vec<u8>, VfsError> {
    let mut out = Vec::with_capacity(RSP_HEADER);
    out.extend_from_slice(&CODE_OK.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.push(0);
    out.extend_from_slice(&0u16.to_le_bytes());
    let mut included = 0usize;
    for entry in page.entries.iter().take(MAX_ENTRIES_PER_PAGE as usize) {
        let name = entry.name.as_bytes();
        if name.is_empty() || name.len() > MAX_NAME_LEN {
            return Err(VfsError::Invalid);
        }
        if out.len() + ENTRY_FIXED + name.len() > MAX_READDIR_RESPONSE_BYTES {
            break;
        }
        out.push(entry.kind as u8);
        out.extend_from_slice(&entry.size.to_le_bytes());
        out.push(name.len() as u8);
        out.extend_from_slice(name);
        included += 1;
    }
    let full = included == page.entries.len();
    out[2..6].copy_from_slice(&page.next_cursor.to_le_bytes());
    out[6] = u8::from(page.eof && full);
    out[7..9].copy_from_slice(&(included as u16).to_le_bytes());
    Ok(out)
}

/// Decodes a ReadDir response payload (client side). Bounded and fail-closed:
/// a count that disagrees with the actual bytes is `Io`, never a short page.
pub fn decode_readdir_response(payload: &[u8]) -> Result<ReadDirPage, VfsError> {
    if payload.len() < RSP_HEADER {
        return Err(VfsError::Io);
    }
    let status = u16::from_le_bytes([payload[0], payload[1]]);
    if let Some(err) = VfsError::from_code(status) {
        return Err(err);
    }
    let next_cursor = u32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]]);
    let eof = match payload[6] {
        0 => false,
        1 => true,
        _ => return Err(VfsError::Io),
    };
    let count = u16::from_le_bytes([payload[7], payload[8]]) as usize;
    if count > MAX_ENTRIES_PER_PAGE as usize {
        return Err(VfsError::Io);
    }
    let mut entries = Vec::with_capacity(count);
    let mut offset = RSP_HEADER;
    for _ in 0..count {
        if payload.len() < offset + ENTRY_FIXED {
            return Err(VfsError::Io);
        }
        let kind = FileKind::from_wire(payload[offset]).ok_or(VfsError::Io)?;
        let mut size_bytes = [0u8; 8];
        size_bytes.copy_from_slice(&payload[offset + 1..offset + 9]);
        let size = u64::from_le_bytes(size_bytes);
        let name_len = payload[offset + 9] as usize;
        offset += ENTRY_FIXED;
        if name_len == 0 || payload.len() < offset + name_len {
            return Err(VfsError::Io);
        }
        let name =
            core::str::from_utf8(&payload[offset..offset + name_len]).map_err(|_| VfsError::Io)?;
        offset += name_len;
        entries.push(DirEntry { name: String::from(name), kind, size });
    }
    if offset != payload.len() {
        return Err(VfsError::Io);
    }
    Ok(ReadDirPage { entries, next_cursor, eof })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::ToString;

    fn sample(n: usize) -> Vec<DirEntry> {
        (0..n)
            .map(|i| DirEntry {
                name: format!("entry-{i:03}"),
                kind: if i % 3 == 0 { FileKind::Dir } else { FileKind::File },
                size: (i as u64) * 17,
            })
            .collect()
    }

    #[test]
    fn request_roundtrip() {
        let payload = encode_readdir_request("pkg:/chat", 7, 32).expect("encode");
        let req = decode_readdir_request(&payload).expect("decode");
        assert_eq!(req, ReadDirRequest { cursor: 7, limit: 32, path: "pkg:/chat".into() });
    }

    #[test]
    fn test_reject_oversize_or_empty_request_path() {
        assert_eq!(encode_readdir_request("", 0, 1), Err(VfsError::Invalid));
        let long = "x".repeat(MAX_PATH_LEN + 1);
        assert_eq!(encode_readdir_request(&long, 0, 1), Err(VfsError::Invalid));
        // Server side: truncated header
        assert_eq!(decode_readdir_request(&[0, 0, 0]), Err(VfsError::Invalid));
    }

    #[test]
    fn response_roundtrip_single_page() {
        let entries = sample(5);
        let (payload, included) = encode_readdir_response(&entries, 0, 64, true).expect("encode");
        assert_eq!(included, 5);
        let page = decode_readdir_response(&payload).expect("decode");
        assert_eq!(page.entries, entries);
        assert!(page.eof);
        assert_eq!(page.next_cursor, 5);
    }

    #[test]
    fn pagination_is_deterministic_and_exact() {
        let entries = sample(150);
        let mut collected = Vec::new();
        let mut cursor = 0usize;
        let mut pages = 0;
        loop {
            let (payload, included) =
                encode_readdir_response(&entries, cursor, 64, true).expect("encode");
            let page = decode_readdir_response(&payload).expect("decode");
            assert_eq!(page.entries.len(), included);
            assert!(included <= MAX_ENTRIES_PER_PAGE as usize);
            collected.extend(page.entries);
            cursor = page.next_cursor as usize;
            pages += 1;
            if page.eof {
                break;
            }
            assert!(pages < 10, "listing must terminate");
        }
        assert_eq!(collected, entries);
        assert_eq!(pages, 3); // 64 + 64 + 22
    }

    #[test]
    fn byte_budget_truncates_before_frame_overflow() {
        // Max-size names force the byte budget to bite before the 64-entry cap.
        let entries: Vec<DirEntry> = (0..64)
            .map(|i| DirEntry {
                name: format!("{i:03}-").to_string() + &"n".repeat(MAX_NAME_LEN - 4),
                kind: FileKind::File,
                size: 1,
            })
            .collect();
        let (payload, included) = encode_readdir_response(&entries, 0, 64, true).expect("encode");
        assert!(payload.len() <= MAX_READDIR_RESPONSE_BYTES, "budget respected");
        assert!(included < 64, "budget must truncate the page");
        let page = decode_readdir_response(&payload).expect("decode");
        assert_eq!(page.entries.len(), included);
        assert!(!page.eof, "truncated page is never eof");
    }

    #[test]
    fn error_page_roundtrips_as_error() {
        let payload = encode_readdir_error(VfsError::NotDir);
        assert_eq!(decode_readdir_response(&payload), Err(VfsError::NotDir));
    }

    #[test]
    fn test_reject_corrupt_response() {
        let entries = sample(3);
        let (mut payload, _) = encode_readdir_response(&entries, 0, 64, true).expect("encode");
        // Trailing garbage
        payload.push(0xFF);
        assert_eq!(decode_readdir_response(&payload), Err(VfsError::Io));
        payload.pop();
        // Count lies (says 4, carries 3)
        payload[7..9].copy_from_slice(&4u16.to_le_bytes());
        assert_eq!(decode_readdir_response(&payload), Err(VfsError::Io));
        // Unknown kind byte
        let (mut payload, _) = encode_readdir_response(&entries, 0, 64, true).expect("encode");
        payload[RSP_HEADER] = 9;
        assert_eq!(decode_readdir_response(&payload), Err(VfsError::Io));
        // Truncated header
        assert_eq!(decode_readdir_response(&[0u8; 4]), Err(VfsError::Io));
    }

    #[test]
    fn test_reject_oversize_entry_name_at_encode() {
        let bad = [DirEntry { name: "x".repeat(MAX_NAME_LEN + 1), kind: FileKind::File, size: 0 }];
        assert_eq!(encode_readdir_response(&bad, 0, 64, true), Err(VfsError::Invalid));
    }
}
