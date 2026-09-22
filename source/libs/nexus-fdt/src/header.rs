// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the DTB header, block bounds and the memory-reservation block — the part
//! of the format that decides whether the rest may be read at all. Every accessor in
//! the crate goes through [`Fdt::bytes_at`] / [`Fdt::u32_at`], which are the two
//! places a bound is checked; nothing indexes the buffer directly.

use core::fmt;

/// Why a tree was refused. Typed so a boot stage can print WHICH check failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The first word is not `0xd00dfeed`.
    BadMagic,
    /// `totalsize` or a block offset points past the buffer, or the header is short.
    Truncated,
    /// `last_comp_version` above 17: the layout may not be what we parse.
    Version,
    /// A structure-block token or a property body runs past its block.
    Structure,
    /// A property's name offset points past the strings block.
    StringOverflow,
    /// A property is shorter than the cells it must hold.
    ShortProp,
    /// Nesting deeper than [`crate::MAX_DEPTH`].
    TooDeep,
    /// The buffer has no headroom for the requested `/chosen` edit.
    NoHeadroom,
    /// A write needs the strings block to be the last block (it is on every producer
    /// we know); refuse rather than relocate blocks.
    Layout,
    /// A node or property the caller required is absent.
    NotFound,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Error::BadMagic => "bad magic",
            Error::Truncated => "truncated",
            Error::Version => "unsupported version",
            Error::Structure => "malformed structure block",
            Error::StringOverflow => "string offset past strings block",
            Error::ShortProp => "property shorter than its cells",
            Error::TooDeep => "nesting too deep",
            Error::NoHeadroom => "no headroom for the edit",
            Error::Layout => "strings block is not last",
            Error::NotFound => "not found",
        };
        f.write_str(s)
    }
}

/// One entry of the memory-reservation block: a physical range the tree's
/// producer asks every consumer to leave alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReservedEntry {
    pub address: u64,
    pub size: u64,
}

pub(crate) const MAGIC: u32 = 0xd00d_feed;
pub(crate) const HEADER_LEN: usize = 40;
const SUPPORTED_VERSION: u32 = 17;

/// A validated view over a flattened device tree.
#[derive(Clone, Copy)]
pub struct Fdt<'a> {
    bytes: &'a [u8],
    pub(crate) totalsize: usize,
    pub(crate) off_struct: usize,
    pub(crate) size_struct: usize,
    pub(crate) off_strings: usize,
    pub(crate) size_strings: usize,
    pub(crate) off_rsvmap: usize,
}

impl<'a> Fdt<'a> {
    /// Validate the header and block bounds. `bytes` may be longer than `totalsize`
    /// (headroom); nothing past `totalsize` is ever read.
    pub fn new(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER_LEN {
            return Err(Error::Truncated);
        }
        let word = |i: usize| -> u32 {
            let b = &bytes[i * 4..i * 4 + 4];
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        };
        if word(0) != MAGIC {
            return Err(Error::BadMagic);
        }
        let totalsize = word(1) as usize;
        let off_struct = word(2) as usize;
        let off_strings = word(3) as usize;
        let off_rsvmap = word(4) as usize;
        let last_comp_version = word(6);
        let size_strings = word(8) as usize;
        let size_struct = word(9) as usize;
        if last_comp_version > SUPPORTED_VERSION {
            return Err(Error::Version);
        }
        if totalsize > bytes.len() || totalsize < HEADER_LEN {
            return Err(Error::Truncated);
        }
        let inside = |off: usize, len: usize| off <= totalsize && len <= totalsize - off;
        if !inside(off_struct, size_struct)
            || !inside(off_strings, size_strings)
            || off_rsvmap > totalsize
            || off_struct % 4 != 0
        {
            return Err(Error::Truncated);
        }
        Ok(Fdt { bytes, totalsize, off_struct, size_struct, off_strings, size_strings, off_rsvmap })
    }

    /// The whole buffer this view was built over (headroom included).
    pub fn buffer(&self) -> &'a [u8] {
        self.bytes
    }

    /// `totalsize` from the header: the bytes that are the tree.
    pub fn total_size(&self) -> usize {
        self.totalsize
    }

    /// `len` bytes at `off`, refused when they leave the tree.
    pub(crate) fn bytes_at(&self, off: usize, len: usize) -> Result<&'a [u8], Error> {
        if off > self.totalsize || len > self.totalsize - off {
            return Err(Error::Truncated);
        }
        Ok(&self.bytes[off..off + len])
    }

    /// One big-endian word at `off`.
    pub(crate) fn u32_at(&self, off: usize) -> Result<u32, Error> {
        let b = self.bytes_at(off, 4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub(crate) fn u64_at(&self, off: usize) -> Result<u64, Error> {
        let hi = self.u32_at(off)? as u64;
        let lo = self.u32_at(off + 4)? as u64;
        Ok((hi << 32) | lo)
    }

    /// A NUL-terminated string in the strings block at `nameoff`.
    pub(crate) fn string(&self, nameoff: usize) -> Result<&'a str, Error> {
        if nameoff >= self.size_strings {
            return Err(Error::StringOverflow);
        }
        let block = self.bytes_at(self.off_strings, self.size_strings)?;
        let tail = &block[nameoff..];
        let end = tail.iter().position(|&b| b == 0).ok_or(Error::StringOverflow)?;
        core::str::from_utf8(&tail[..end]).map_err(|_| Error::StringOverflow)
    }

    /// A NUL-terminated string inside the structure block (a node name).
    pub(crate) fn struct_str(&self, off: usize) -> Result<(&'a str, usize), Error> {
        let end_of_block = self.off_struct + self.size_struct;
        if off >= end_of_block {
            return Err(Error::Structure);
        }
        let tail = self.bytes_at(off, end_of_block - off)?;
        let end = tail.iter().position(|&b| b == 0).ok_or(Error::Structure)?;
        let s = core::str::from_utf8(&tail[..end]).map_err(|_| Error::Structure)?;
        Ok((s, off + end + 1))
    }

    /// The memory-reservation block, in order, without its terminator.
    pub fn reserved_entries(&self) -> impl Iterator<Item = ReservedEntry> + 'a {
        let fdt = *self;
        let mut off = self.off_rsvmap;
        core::iter::from_fn(move || {
            let address = fdt.u64_at(off).ok()?;
            let size = fdt.u64_at(off + 8).ok()?;
            if address == 0 && size == 0 {
                return None;
            }
            off += 16;
            Some(ReservedEntry { address, size })
        })
    }
}

/// Round up to the next multiple of four (token and property bodies are aligned).
pub(crate) fn align4(v: usize) -> usize {
    (v + 3) & !3
}
