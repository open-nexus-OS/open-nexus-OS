// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The kernel console ring (RFC-0107 Phase 2, TASK-0327B P2): every byte the kernel
//! sends to the console — its own lines and every service's, in exactly the order the UART
//! receives them — kept in a fixed ring of whole pages that the kernel exposes read-only to the
//! block owner, which keeps them in the boot trace. This crate is the ring's one format: the
//! layout both sides compile against, the header check, and the reader. The kernel writes
//! (a header page, then `DATA_BYTES` of data; `head` counts every byte ever written, so the next
//! byte lands at `head % DATA_BYTES`); a reader copies what it has not seen, counts what the
//! writer overwrote before it could be read, and drops a prefix the writer overwrote DURING the
//! copy — so every byte it returns is a byte the kernel wrote, in order.
//! Pure: the memory itself is behind [`Source`] (the kernel's pages mapped read-only on the OS,
//! a buffer in the tests).
//! OWNERS: @runtime @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal (the layout is RFC-0107's contract between the kernel and the reader)
//! TEST_COVERAGE: unit tests below

#![no_std]
#![forbid(unsafe_code)]

/// One page: the header's, and the unit the ring is exposed in.
pub const PAGE: usize = 4096;
/// The ring's data: a power of two, so a position is `head % DATA_BYTES`.
pub const DATA_BYTES: usize = 128 * 1024;
/// The whole ring as the kernel exposes it: the header page, then the data.
pub const BYTES: usize = PAGE + DATA_BYTES;
/// Where the data starts.
pub const DATA_AT: usize = PAGE;

/// The header (little-endian): `magic` at 0, `version` (u32) at 8, `data_bytes` (u32) at 12,
/// `head` (u64, written atomically by the kernel) at 16; the rest of the page is zero.
pub const MAGIC: [u8; 8] = *b"NXCRING1";
pub const VERSION: u32 = 1;
pub const OFF_MAGIC: usize = 0;
pub const OFF_VERSION: usize = 8;
pub const OFF_DATA_BYTES: usize = 12;
pub const OFF_HEAD: usize = 16;

/// Whether a header page describes this ring: the magic, the version and the data size.
pub fn header_ok(header: &[u8]) -> bool {
    let Some(h) = header.get(..OFF_HEAD) else { return false };
    let u32_at = |at: usize| u32::from_le_bytes([h[at], h[at + 1], h[at + 2], h[at + 3]]);
    h[OFF_MAGIC..OFF_MAGIC + 8] == MAGIC
        && u32_at(OFF_VERSION) == VERSION
        && u32_at(OFF_DATA_BYTES) as usize == DATA_BYTES
}

/// The header fields as the kernel writes them once, before the first byte.
pub fn header_fields() -> [(usize, [u8; 8]); 2] {
    let mut sizes = [0u8; 8];
    sizes[..4].copy_from_slice(&VERSION.to_le_bytes());
    sizes[4..].copy_from_slice(&(DATA_BYTES as u32).to_le_bytes());
    [(OFF_MAGIC, MAGIC), (OFF_VERSION, sizes)]
}

/// The ring's memory as a reader sees it.
pub trait Source {
    /// Every byte the writer has written so far (acquire: the bytes before it are visible).
    fn head(&self) -> u64;
    /// Copy the data bytes at `at..at + out.len()` (never past `DATA_BYTES`: the caller splits
    /// a wrapping span).
    fn copy(&self, at: usize, out: &mut [u8]);
}

/// What one read returned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Batch {
    /// Bytes placed at the start of the caller's buffer, in the writer's order.
    pub len: usize,
    /// Bytes the writer overwrote before this reader could copy them.
    pub lost: u64,
}

/// A reader's position in the byte stream.
#[derive(Clone, Copy, Debug, Default)]
pub struct Reader {
    next: u64,
}

impl Reader {
    pub const fn new() -> Self {
        Self { next: 0 }
    }

    /// The stream position of the next byte this reader returns.
    pub fn position(&self) -> u64 {
        self.next
    }

    /// Copies the bytes written since the last read — at most `out.len()` — to the start of
    /// `out`, oldest first.
    pub fn read<S: Source>(&mut self, src: &S, out: &mut [u8]) -> Batch {
        let data = DATA_BYTES as u64;
        let head = src.head();
        if head <= self.next || out.is_empty() {
            return Batch::default();
        }
        // What the writer already overwrote is gone: start at the oldest byte still there.
        let mut from = self.next;
        let mut lost = 0;
        if head - from > data {
            lost = head - from - data;
            from = head - data;
        }
        let want = (head - from).min(out.len() as u64) as usize;
        let at = (from % data) as usize;
        let first = want.min(DATA_BYTES - at);
        src.copy(at, &mut out[..first]);
        if want > first {
            src.copy(0, &mut out[first..want]);
        }
        // Bytes the writer overwrote while they were copied are not the bytes that were there:
        // everything before `after - data` is gone, so a prefix of the copy may be newer text.
        let gone_before = src.head().saturating_sub(data);
        let skip = gone_before.saturating_sub(from).min(want as u64) as usize;
        if skip > 0 {
            out.copy_within(skip..want, 0);
            lost += skip as u64;
        }
        self.next = from + want as u64;
        Batch { len: want - skip, lost }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use core::cell::{Cell, RefCell};
    use std::vec;
    use std::vec::Vec;

    /// The kernel's side, in memory: the ring and its head, plus a writer that can run
    /// "during" a copy (the torn-read case).
    struct Ring {
        data: RefCell<Vec<u8>>,
        head: Cell<u64>,
        on_copy: Cell<u64>,
        next_byte: Cell<u8>,
    }

    impl Ring {
        fn new() -> Self {
            Self {
                data: RefCell::new(vec![0; DATA_BYTES]),
                head: Cell::new(0),
                on_copy: Cell::new(0),
                next_byte: Cell::new(0),
            }
        }
        /// Writes `n` bytes of the stream 0, 1, 2, … (mod 251): byte k of the stream is k % 251.
        fn write(&self, n: u64) {
            for _ in 0..n {
                let h = self.head.get();
                self.data.borrow_mut()[(h % DATA_BYTES as u64) as usize] = self.next_byte.get();
                self.next_byte.set(((h + 1) % 251) as u8);
                self.head.set(h + 1);
            }
        }
    }

    impl Source for Ring {
        fn head(&self) -> u64 {
            self.head.get()
        }
        fn copy(&self, at: usize, out: &mut [u8]) {
            let n = self.on_copy.replace(0);
            self.write(n);
            out.copy_from_slice(&self.data.borrow()[at..at + out.len()]);
        }
    }

    fn stream(from: u64, len: usize) -> Vec<u8> {
        (from..from + len as u64).map(|k| (k % 251) as u8).collect()
    }

    #[test]
    fn a_reader_returns_every_byte_once_in_order_across_small_buffers_and_the_wrap() {
        let ring = Ring::new();
        let mut reader = Reader::new();
        let mut got = Vec::new();
        let mut buf = [0u8; 700];
        for round in 0..6u64 {
            ring.write(50_000 + round * 13);
            loop {
                let b = reader.read(&ring, &mut buf);
                assert_eq!(b.lost, 0, "a reader that keeps up loses nothing");
                if b.len == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..b.len]);
            }
        }
        assert_eq!(got, stream(0, got.len()));
        assert_eq!(reader.position(), ring.head.get());
        assert!(ring.head.get() > 2 * DATA_BYTES as u64, "the ring wrapped");
    }

    #[test]
    fn test_reject_bytes_the_writer_overwrote_before_the_read() {
        let ring = Ring::new();
        ring.write(DATA_BYTES as u64 + 1000);
        let mut reader = Reader::new();
        let mut buf = vec![0u8; DATA_BYTES];
        let b = reader.read(&ring, &mut buf);
        assert_eq!((b.len, b.lost), (DATA_BYTES, 1000), "the oldest 1000 bytes are gone");
        assert_eq!(&buf[..b.len], &stream(1000, DATA_BYTES)[..]);
    }

    #[test]
    fn test_reject_a_prefix_the_writer_overwrote_during_the_copy() {
        let ring = Ring::new();
        ring.write(DATA_BYTES as u64);
        let mut reader = Reader::new();
        let mut buf = vec![0u8; DATA_BYTES];
        // While the first span is copied, the writer laps the 300 oldest bytes.
        ring.on_copy.set(300);
        let b = reader.read(&ring, &mut buf);
        assert_eq!((b.len, b.lost), (DATA_BYTES - 300, 300));
        assert_eq!(
            &buf[..b.len],
            &stream(300, DATA_BYTES - 300)[..],
            "never a newer byte in an older place"
        );
        // The next read picks up exactly where the stream continues.
        let b = reader.read(&ring, &mut buf);
        assert_eq!((b.len, b.lost), (300, 0));
        assert_eq!(&buf[..b.len], &stream(DATA_BYTES as u64, 300)[..]);
    }

    #[test]
    fn test_reject_a_header_of_another_ring() {
        let mut page = vec![0u8; PAGE];
        for (at, field) in header_fields() {
            page[at..at + 8].copy_from_slice(&field);
        }
        assert!(header_ok(&page));
        let mut bad = page.clone();
        bad[0] ^= 1;
        assert!(!header_ok(&bad), "magic");
        let mut bad = page.clone();
        bad[OFF_VERSION] = 2;
        assert!(!header_ok(&bad), "version");
        let mut bad = page.clone();
        bad[OFF_DATA_BYTES + 1] ^= 1;
        assert!(!header_ok(&bad), "data size");
        assert!(!header_ok(&page[..8]), "too short");
    }
}
