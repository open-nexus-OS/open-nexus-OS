// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Read-only bundle image format used in OS bring-up (served by bundlemgrd,
//! consumed by packagefsd).
//!
//! This is a streaming container format (header + cursor-advancing entry
//! parser), not a request/reply frame — it stays hand-written on
//! [`crate::codec::Reader`] instead of the `frames!` DSL.

use crate::codec::Reader;

/// Image magic `NXBI` ("NeXuS Bundle Image").
const MAGIC: [u8; 4] = *b"NXBI";
/// Image format version.
pub const VERSION: u8 = 1;

/// Entry kind: file.
pub const KIND_FILE: u16 = 0;

/// Parsed entry view.
pub struct Entry<'a> {
    /// Bundle name bytes (UTF-8).
    pub bundle: &'a [u8],
    /// Bundle version bytes (UTF-8).
    pub version: &'a [u8],
    /// Entry path bytes (UTF-8, relative inside the bundle).
    pub path: &'a [u8],
    /// Entry kind (e.g. [`KIND_FILE`]).
    pub kind: u16,
    /// Entry payload bytes (for files).
    pub data: &'a [u8],
}

/// Parses the header and returns (entry_count, first_entry_offset).
pub fn decode_header(frame: &[u8]) -> Option<(u16, usize)> {
    let mut r = Reader::new(frame);
    if r.take_bytes(4)? != MAGIC {
        return None;
    }
    r.expect_u8(VERSION)?;
    let count = r.take_u16le()?;
    Some((count, 7))
}

/// Parses the next entry starting at `*off` and advances `off` on success.
pub fn decode_next<'a>(frame: &'a [u8], off: &mut usize) -> Option<Entry<'a>> {
    if *off >= frame.len() {
        return None;
    }
    let mut r = Reader::new(frame.get(*off..)?);
    let bundle = r.take_len8_bytes(0, u8::MAX as usize)?;
    let version = r.take_len8_bytes(0, u8::MAX as usize)?;
    let path = r.take_len16_bytes(0, u16::MAX as usize)?;
    let kind = r.take_u16le()?;
    let data = r.take_len32_bytes(0, u32::MAX as usize)?;
    *off += r.pos();
    Some(Entry { bundle, version, path, kind, data })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Golden image used by bring-up (mirrors bundlemgrd os-lite image):
    // NXBI v1 with one entry: system@1.0.0/build.prop => "ro.nexus.build=dev\n"
    const GOLDEN_IMG: &[u8] = &[
        b'N', b'X', b'B', b'I', 1, 1, 0, // magic + version + count=1
        6, b's', b'y', b's', b't', b'e', b'm', // bundle "system"
        5, b'1', b'.', b'0', b'.', b'0', // version "1.0.0"
        10, 0, b'b', b'u', b'i', b'l', b'd', b'.', b'p', b'r', b'o',
        b'p', // path len=10 + "build.prop"
        0, 0, // kind=KIND_FILE
        19, 0, 0, 0, // data_len=19
        b'r', b'o', b'.', b'n', b'e', b'x', b'u', b's', b'.', b'b', b'u', b'i', b'l', b'd', b'=',
        b'd', b'e', b'v', b'\n',
    ];

    #[test]
    fn header_golden() {
        let (count, off) = decode_header(GOLDEN_IMG).unwrap();
        assert_eq!(count, 1);
        assert_eq!(off, 7);
    }

    #[test]
    fn entry_golden() {
        let (_count, mut off) = decode_header(GOLDEN_IMG).unwrap();
        let e = decode_next(GOLDEN_IMG, &mut off).unwrap();
        assert_eq!(e.bundle, b"system");
        assert_eq!(e.version, b"1.0.0");
        assert_eq!(e.path, b"build.prop");
        assert_eq!(e.kind, KIND_FILE);
        assert_eq!(e.data, b"ro.nexus.build=dev\n");
        assert_eq!(off, GOLDEN_IMG.len());
    }
}
