// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
//! CONTEXT: The boot image is one verified container (ADR-0081): a header sector naming two
//! items — the flat kernel and the bootfs — followed by the items; and the bootfs is a table of
//! contents followed by page-aligned files, each a program's ELF with the stack size and the
//! global pointer its spawner needs. This crate is the only code that understands either
//! layout: the packer (`nx image payload`) writes them, `nxboot` reads the header to place the
//! items and streams the one digest over them, the kernel reads the table to adopt every file as
//! a sealed object, init reads the same table to know what it spawns.
//!
//! WHY BOUNDED AND ALLOCATION-FREE ON THE READ SIDE: the bytes are inside the digest the loader
//! verified (like the kernel's own), yet every offset and length is checked against the
//! container they live in — a malformed table is a typed error and a loud boot failure, never a
//! wild read. The builders (`alloc` feature) exist for the host and for tests.
//!
//! OWNERS: @kernel-team @security @runtime
//! STATUS: Functional
//! API_STABILITY: Stable within the workspace — the two layouts are the contract between the
//!   build, the loader, the kernel and init; a change bumps the magic.
//! TEST_COVERAGE: unit tests below — round trips and the reject matrix of both codecs
//! ADR: docs/adr/0081-the-boot-image-is-one-verified-container-kernel-plus-bootfs-every-core-program-runs-from-it-no-program-embeds-another.md

#[cfg(feature = "alloc")]
extern crate alloc;

/// A page: every file in the bootfs starts on one; the table fills the first.
pub const PAGE: u64 = 4096;
/// A disk sector: the payload header is one; every item starts on one.
pub const SECTOR: u64 = 512;

/// Why a header or a table was refused. Each variant names the one check that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The bytes do not start with the magic of the layout asked for.
    Magic,
    /// A version this codec does not read.
    Version,
    /// A reserved field is not zero.
    Reserved,
    /// Too few bytes for the header or the table.
    Truncated,
    /// An item or a file is empty.
    Empty,
    /// An item does not start on a sector, or a file does not start on a page.
    Alignment,
    /// An item or a file does not lie where the layout says it must (gap, overlap, order).
    Layout,
    /// A memory footprint smaller than the bytes it must hold.
    Footprint,
    /// More files than a table holds.
    TooManyFiles,
    /// A file name that is empty, not plain ASCII, or not NUL-padded.
    Name,
    /// Two files with one name.
    DuplicateName,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let text = match self {
            Error::Magic => "wrong magic",
            Error::Version => "unknown version",
            Error::Reserved => "reserved field not zero",
            Error::Truncated => "truncated",
            Error::Empty => "empty item or file",
            Error::Alignment => "item or file off its grid",
            Error::Layout => "item or file out of place",
            Error::Footprint => "footprint smaller than the bytes",
            Error::TooManyFiles => "too many files",
            Error::Name => "bad file name",
            Error::DuplicateName => "duplicate file name",
        };
        f.write_str(text)
    }
}

#[cfg(feature = "alloc")]
impl core::error::Error for Error {}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&bytes[at..at + 8]);
    u64::from_le_bytes(b)
}

fn align_up(value: u64, align: u64) -> Option<u64> {
    value.checked_add(align - 1).map(|v| v & !(align - 1))
}

/// The payload container: `[header sector][kernel item][bootfs item]`, all three inside the
/// NXBD's `image_size` and `image_sha256` (RFC-0089 §5 as amended by ADR-0081).
pub mod payload {
    use super::{align_up, u64_at, Error, SECTOR};

    pub const MAGIC: &[u8; 8] = b"NXBI0001";
    /// The header is one sector; the kernel item starts right after it.
    pub const HEADER_LEN: usize = SECTOR as usize;

    /// What the header says about the two items. Offsets are from the payload's first byte.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Header {
        /// Always `HEADER_LEN`: the kernel item follows the header.
        pub kernel_offset: u64,
        /// The flat kernel's bytes.
        pub kernel_len: u64,
        /// The kernel's memory footprint from its load address: file bytes, `.bss` and stack.
        /// The loader places the bootfs beyond it, so nothing the kernel zeroes touches it.
        pub kernel_footprint: u64,
        /// The bootfs item: the kernel item rounded up to the next sector.
        pub bootfs_offset: u64,
        /// The bootfs's bytes (a whole number of pages).
        pub bootfs_len: u64,
    }

    impl Header {
        /// The header for a kernel of `kernel_len` bytes and `kernel_footprint` in memory and a
        /// bootfs of `bootfs_len` bytes, laid out as the container rule says.
        pub fn for_items(kernel_len: u64, kernel_footprint: u64, bootfs_len: u64) -> Option<Self> {
            let kernel_offset = HEADER_LEN as u64;
            let bootfs_offset = align_up(kernel_offset.checked_add(kernel_len)?, SECTOR)?;
            Some(Self { kernel_offset, kernel_len, kernel_footprint, bootfs_offset, bootfs_len })
        }

        /// The payload's total length: the bootfs item rounded up to a sector.
        pub fn total_len(&self) -> Option<u64> {
            align_up(self.bootfs_offset.checked_add(self.bootfs_len)?, SECTOR)
        }

        /// The checks every reader makes: the items in order, on their grids, non-empty, the
        /// footprint holding the kernel's bytes.
        fn check(&self) -> Result<(), Error> {
            if self.kernel_offset != HEADER_LEN as u64 {
                return Err(Error::Layout);
            }
            if self.kernel_len == 0 || self.bootfs_len == 0 {
                return Err(Error::Empty);
            }
            if self.bootfs_offset % SECTOR != 0 || self.bootfs_len % super::PAGE != 0 {
                return Err(Error::Alignment);
            }
            let kernel_end =
                self.kernel_offset.checked_add(self.kernel_len).ok_or(Error::Layout)?;
            if align_up(kernel_end, SECTOR).ok_or(Error::Layout)? != self.bootfs_offset {
                return Err(Error::Layout);
            }
            if self.kernel_footprint < self.kernel_len {
                return Err(Error::Footprint);
            }
            self.total_len().ok_or(Error::Layout)?;
            Ok(())
        }
    }

    /// The header sector of `h`.
    pub fn encode(h: &Header) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        out[0..8].copy_from_slice(MAGIC);
        out[8..16].copy_from_slice(&h.kernel_offset.to_le_bytes());
        out[16..24].copy_from_slice(&h.kernel_len.to_le_bytes());
        out[24..32].copy_from_slice(&h.kernel_footprint.to_le_bytes());
        out[32..40].copy_from_slice(&h.bootfs_offset.to_le_bytes());
        out[40..48].copy_from_slice(&h.bootfs_len.to_le_bytes());
        out
    }

    /// The header a sector holds, checked.
    pub fn decode(bytes: &[u8]) -> Result<Header, Error> {
        if bytes.len() < HEADER_LEN {
            return Err(Error::Truncated);
        }
        if &bytes[0..8] != MAGIC {
            return Err(Error::Magic);
        }
        if bytes[48..HEADER_LEN].iter().any(|&b| b != 0) {
            return Err(Error::Reserved);
        }
        let h = Header {
            kernel_offset: u64_at(bytes, 8),
            kernel_len: u64_at(bytes, 16),
            kernel_footprint: u64_at(bytes, 24),
            bootfs_offset: u64_at(bytes, 32),
            bootfs_len: u64_at(bytes, 40),
        };
        h.check()?;
        Ok(h)
    }

    /// The whole payload: header, kernel, padding, bootfs, padding to a sector.
    #[cfg(feature = "alloc")]
    pub fn compose(
        kernel: &[u8],
        kernel_footprint: u64,
        bootfs: &[u8],
    ) -> Result<alloc::vec::Vec<u8>, Error> {
        let header = Header::for_items(kernel.len() as u64, kernel_footprint, bootfs.len() as u64)
            .ok_or(Error::Layout)?;
        header.check()?;
        let total = header.total_len().ok_or(Error::Layout)? as usize;
        let mut out = alloc::vec![0u8; total];
        out[..HEADER_LEN].copy_from_slice(&encode(&header));
        let k = header.kernel_offset as usize;
        out[k..k + kernel.len()].copy_from_slice(kernel);
        let b = header.bootfs_offset as usize;
        out[b..b + bootfs.len()].copy_from_slice(bootfs);
        Ok(out)
    }
}

/// The bootfs: a table of contents in page 0, then the files, each on its own pages.
pub mod toc {
    use super::{u16_at, u32_at, u64_at, Error, PAGE};

    pub const MAGIC: &[u8; 8] = b"NXFS0001";
    pub const VERSION: u16 = 1;
    /// The table's header: 64 bytes, then one 64-byte entry per file.
    pub const HEADER_LEN: usize = 64;
    pub const ENTRY_LEN: usize = 64;
    /// Longest file name (bytes); shorter names are NUL-padded.
    pub const NAME_LEN: usize = 32;
    /// As many entries as the first page holds after the header.
    pub const MAX_FILES: usize = (PAGE as usize - HEADER_LEN) / ENTRY_LEN;

    /// One file of the bootfs, as the table describes it.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct File<'a> {
        pub name: &'a str,
        /// From the bootfs's first byte; a page boundary at or past the table's page.
        pub offset: u64,
        /// The file's bytes (not rounded).
        pub len: u64,
        /// The stack its spawner gives it.
        pub stack_pages: u32,
        /// The RISC-V global pointer its spawner passes.
        pub global_pointer: u64,
    }

    impl File<'_> {
        /// The pages the file occupies: `[offset, offset + len)` rounded up to a page.
        pub fn end_rounded(&self) -> Option<u64> {
            super::align_up(self.offset.checked_add(self.len)?, PAGE)
        }
    }

    /// A checked view of a table page.
    #[derive(Clone, Copy, Debug)]
    pub struct Toc<'a> {
        page: &'a [u8],
        count: usize,
        total_len: u64,
    }

    /// A name is 1..=NAME_LEN bytes of plain ASCII (letters, digits, `-`, `_`, `.`) followed by
    /// NUL padding.
    pub fn name_ok(raw: &[u8]) -> Option<&str> {
        if raw.len() != NAME_LEN {
            return None;
        }
        let end = raw.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
        if end == 0 || raw[end..].iter().any(|&b| b != 0) {
            return None;
        }
        let name = &raw[..end];
        if !name.iter().all(|&b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.') {
            return None;
        }
        core::str::from_utf8(name).ok()
    }

    impl<'a> Toc<'a> {
        /// Reads and checks the table in `page` (the bootfs's first page; more bytes are
        /// ignored). Every entry is checked here, once: names, grid, order, bounds.
        pub fn parse(page: &'a [u8]) -> Result<Self, Error> {
            if page.len() < PAGE as usize {
                return Err(Error::Truncated);
            }
            if &page[0..8] != MAGIC {
                return Err(Error::Magic);
            }
            if u16_at(page, 8) != VERSION {
                return Err(Error::Version);
            }
            let count = u16_at(page, 10) as usize;
            if u32_at(page, 12) != 0 || page[24..HEADER_LEN].iter().any(|&b| b != 0) {
                return Err(Error::Reserved);
            }
            if count > MAX_FILES {
                return Err(Error::TooManyFiles);
            }
            let total_len = u64_at(page, 16);
            if total_len % PAGE != 0 {
                return Err(Error::Alignment);
            }
            let toc = Self { page, count, total_len };
            let mut cursor = PAGE;
            for i in 0..count {
                let f = toc.entry(i)?;
                if f.offset % PAGE != 0 {
                    return Err(Error::Alignment);
                }
                if f.len == 0 {
                    return Err(Error::Empty);
                }
                // Files follow the table in order, each on the pages right after the one before.
                if f.offset != cursor {
                    return Err(Error::Layout);
                }
                cursor = f.end_rounded().ok_or(Error::Layout)?;
                if cursor > total_len {
                    return Err(Error::Layout);
                }
                for j in 0..i {
                    if toc.entry(j)?.name == f.name {
                        return Err(Error::DuplicateName);
                    }
                }
            }
            if cursor != total_len {
                return Err(Error::Layout);
            }
            Ok(toc)
        }

        /// How many files the table lists.
        pub fn len(&self) -> usize {
            self.count
        }

        pub fn is_empty(&self) -> bool {
            self.count == 0
        }

        /// The bootfs's length in bytes (table page and every file's pages).
        pub fn total_len(&self) -> u64 {
            self.total_len
        }

        fn entry(&self, i: usize) -> Result<File<'a>, Error> {
            let at = HEADER_LEN + i * ENTRY_LEN;
            let e = &self.page[at..at + ENTRY_LEN];
            let name = name_ok(&e[..NAME_LEN]).ok_or(Error::Name)?;
            if u32_at(e, 52) != 0 {
                return Err(Error::Reserved);
            }
            Ok(File {
                name,
                offset: u64_at(e, 32),
                len: u64_at(e, 40),
                stack_pages: u32_at(e, 48),
                global_pointer: u64_at(e, 56),
            })
        }

        /// The files in table order. Every entry passed `parse`, so none fails here.
        pub fn files(&self) -> impl Iterator<Item = File<'a>> + '_ {
            (0..self.count).filter_map(move |i| self.entry(i).ok())
        }

        /// The file called `name`, if the table lists one.
        pub fn find(&self, name: &str) -> Option<File<'a>> {
            self.files().find(|f| f.name == name)
        }
    }

    /// Builds a bootfs on the host: files in the order added, each on fresh pages.
    #[cfg(feature = "alloc")]
    pub struct Builder {
        /// name, offset, len, stack_pages, global_pointer
        entries: alloc::vec::Vec<(alloc::string::String, u64, u64, u32, u64)>,
        bytes: alloc::vec::Vec<u8>,
    }

    #[cfg(feature = "alloc")]
    impl Default for Builder {
        fn default() -> Self {
            Self::new()
        }
    }

    #[cfg(feature = "alloc")]
    impl Builder {
        pub fn new() -> Self {
            Self { entries: alloc::vec::Vec::new(), bytes: alloc::vec![0u8; PAGE as usize] }
        }

        /// Adds `file` as `name` with the stack and global pointer its spawner passes.
        pub fn add(
            &mut self,
            name: &str,
            file: &[u8],
            stack_pages: u32,
            global_pointer: u64,
        ) -> Result<(), Error> {
            if self.entries.len() == MAX_FILES {
                return Err(Error::TooManyFiles);
            }
            let mut raw = [0u8; NAME_LEN];
            if name.len() > NAME_LEN {
                return Err(Error::Name);
            }
            raw[..name.len()].copy_from_slice(name.as_bytes());
            if name_ok(&raw).is_none() {
                return Err(Error::Name);
            }
            if self.entries.iter().any(|(n, ..)| n == name) {
                return Err(Error::DuplicateName);
            }
            if file.is_empty() {
                return Err(Error::Empty);
            }
            let offset = self.bytes.len() as u64;
            self.bytes.extend_from_slice(file);
            let rounded = super::align_up(self.bytes.len() as u64, PAGE).ok_or(Error::Layout)?;
            self.bytes.resize(rounded as usize, 0);
            self.entries.push((
                alloc::string::String::from(name),
                offset,
                file.len() as u64,
                stack_pages,
                global_pointer,
            ));
            Ok(())
        }

        /// The bootfs bytes: the table in page 0, every file on its pages.
        pub fn finish(mut self) -> Result<alloc::vec::Vec<u8>, Error> {
            let total = self.bytes.len() as u64;
            let page = &mut self.bytes[..PAGE as usize];
            page[0..8].copy_from_slice(MAGIC);
            page[8..10].copy_from_slice(&VERSION.to_le_bytes());
            page[10..12].copy_from_slice(&(self.entries.len() as u16).to_le_bytes());
            page[16..24].copy_from_slice(&total.to_le_bytes());
            for (i, (name, offset, len, stack_pages, gp)) in self.entries.iter().enumerate() {
                let at = HEADER_LEN + i * ENTRY_LEN;
                let e = &mut page[at..at + ENTRY_LEN];
                e[..name.len()].copy_from_slice(name.as_bytes());
                e[32..40].copy_from_slice(&offset.to_le_bytes());
                e[40..48].copy_from_slice(&len.to_le_bytes());
                e[48..52].copy_from_slice(&stack_pages.to_le_bytes());
                e[56..64].copy_from_slice(&gp.to_le_bytes());
            }
            Ok(self.bytes)
        }
    }
}

#[cfg(test)]
mod tests;
