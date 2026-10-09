// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the output side of the encoder. Packs the deflate stream LSB-first, stages the
//! zlib bytes in one fixed buffer, and hands every full buffer to the sink as ONE complete
//! IDAT chunk (length, type, payload, CRC-32) in a single write; the last chunk is shorter.
//! Also passes the other chunks through unchanged and counts every byte the sink accepted.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/encode/stream.rs (chunk boundaries, sizes, CRCs, failing sinks)

use crate::crc::Crc32;
use crate::sink::Sink;

/// Payload bytes per IDAT chunk: every IDAT but the last carries exactly this many.
pub const IDAT_PAYLOAD_MAX: usize = 16 * 1024;

/// Length (4 bytes) and type (4 bytes) in front of a chunk's payload.
const HEAD: usize = 8;

/// The staging buffer holds one whole IDAT chunk: head, payload, CRC.
pub(crate) const STAGING_LEN: usize = HEAD + IDAT_PAYLOAD_MAX + 4;

/// One encode's view of the staging buffer and the sink.
pub(crate) struct ChunkWriter<'a, S: Sink + ?Sized> {
    staging: &'a mut [u8; STAGING_LEN],
    /// Payload bytes staged so far.
    staged: usize,
    /// Pending deflate bits, LSB first; fewer than 32 between calls.
    acc: u64,
    nbits: u32,
    sink: &'a mut S,
    written: u64,
}

impl<'a, S: Sink + ?Sized> ChunkWriter<'a, S> {
    pub(crate) fn new(staging: &'a mut [u8; STAGING_LEN], sink: &'a mut S) -> Self {
        Self { staging, staged: 0, acc: 0, nbits: 0, sink, written: 0 }
    }

    /// Bytes the sink has accepted so far.
    pub(crate) fn written(&self) -> u64 {
        self.written
    }

    /// Hands a complete non-IDAT unit (signature + IHDR, IEND) straight to the sink.
    pub(crate) fn raw(&mut self, bytes: &[u8]) -> Result<(), S::Error> {
        self.sink.write(bytes)?;
        self.written = self.written.saturating_add(count(bytes.len()));
        Ok(())
    }

    /// Appends the low `n` bits of `value` (`n <= 32`, `value < 2^n`) to the deflate stream.
    pub(crate) fn bits(&mut self, value: u64, n: u32) -> Result<(), S::Error> {
        self.acc |= value << self.nbits;
        self.nbits += n;
        if self.nbits >= 32 {
            let [b0, b1, b2, b3, ..] = self.acc.to_le_bytes();
            self.stage(&[b0, b1, b2, b3])?;
            self.acc >>= 32;
            self.nbits -= 32;
        }
        Ok(())
    }

    /// Appends whole bytes after padding the bit stream to a byte boundary with zeros (the
    /// zlib header before the first block, the Adler-32 after the last).
    pub(crate) fn bytes(&mut self, bytes: &[u8]) -> Result<(), S::Error> {
        let pending = self.acc.to_le_bytes();
        let n = (self.nbits as usize).div_ceil(8);
        self.acc = 0;
        self.nbits = 0;
        self.stage(pending.get(..n).unwrap_or_default())?;
        self.stage(bytes)
    }

    /// Emits the staged bytes as one IDAT chunk; nothing when none are staged.
    pub(crate) fn flush(&mut self) -> Result<(), S::Error> {
        let n = core::mem::take(&mut self.staged);
        if n == 0 {
            return Ok(());
        }
        let end = HEAD + n;
        let len = u32::try_from(n).unwrap_or(u32::MAX);
        self.staging[..4].copy_from_slice(&len.to_be_bytes());
        self.staging[4..HEAD].copy_from_slice(b"IDAT");
        let mut crc = Crc32::new();
        crc.update(self.staging.get(4..end).unwrap_or_default());
        if let Some(tail) = self.staging.get_mut(end..end + 4) {
            tail.copy_from_slice(&crc.finish().to_be_bytes());
        }
        let chunk = self.staging.get(..end + 4).unwrap_or_default();
        self.sink.write(chunk)?;
        self.written = self.written.saturating_add(count(chunk.len()));
        Ok(())
    }

    /// Copies stream bytes into the staging buffer, flushing a chunk whenever it fills.
    fn stage(&mut self, mut bytes: &[u8]) -> Result<(), S::Error> {
        while !bytes.is_empty() {
            let free = self
                .staging
                .get_mut(HEAD + self.staged..HEAD + IDAT_PAYLOAD_MAX)
                .unwrap_or_default();
            let n = free.len().min(bytes.len());
            if n == 0 {
                self.flush()?;
                continue;
            }
            let (now, rest) = bytes.split_at(n);
            if let Some(dst) = free.get_mut(..n) {
                dst.copy_from_slice(now);
            }
            self.staged += n;
            bytes = rest;
        }
        Ok(())
    }
}

/// A byte count as the sink total's type (lossless on every supported target).
fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}
