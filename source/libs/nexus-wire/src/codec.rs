// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Byte-frame codec core shared by every protocol module (ADR-0051)
//! OWNERS: @runtime
//! PUBLIC API: Writer, Reader, put_hdr, check_hdr, request_op, build, REPLY_BIT,
//!             testing::assert_reject_matrix
//! DEPENDS_ON: core only
//! INVARIANTS: scalars are little-endian; all operations are bounds-checked and
//!             fail-closed (`None`, never panic); on a failed encode the output
//!             buffer may be partially written — callers must only use the buffer
//!             after `Some(len)`

/// Reply frames set this bit on the request opcode (`op | 0x80`), the
/// repo-wide request/reply convention (RFC-0019 correlation frames carry an
/// additional `u32le` nonce field declared per protocol).
pub const REPLY_BIT: u8 = 0x80;

/// Sequential little-endian frame writer over a caller-provided buffer.
///
/// Every `put_*` returns `None` when the value violates its bound or the
/// buffer is too small; the buffer contents are unspecified after a failure.
pub struct Writer<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> Writer<'a> {
    /// Starts writing at the beginning of `buf`.
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Number of bytes written so far (= the frame length after the last put).
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Writes one byte.
    pub fn put_u8(&mut self, v: u8) -> Option<()> {
        let b = self.buf.get_mut(self.pos)?;
        *b = v;
        self.pos += 1;
        Some(())
    }

    /// Writes one byte that must be non-zero (frame-level constraint).
    pub fn put_nz_u8(&mut self, v: u8) -> Option<()> {
        if v == 0 {
            return None;
        }
        self.put_u8(v)
    }

    /// Writes raw bytes verbatim.
    pub fn put_bytes(&mut self, bytes: &[u8]) -> Option<()> {
        let end = self.pos.checked_add(bytes.len())?;
        self.buf.get_mut(self.pos..end)?.copy_from_slice(bytes);
        self.pos = end;
        Some(())
    }

    /// Writes a `u16` little-endian.
    pub fn put_u16le(&mut self, v: u16) -> Option<()> {
        self.put_bytes(&v.to_le_bytes())
    }

    /// Writes a `u32` little-endian.
    pub fn put_u32le(&mut self, v: u32) -> Option<()> {
        self.put_bytes(&v.to_le_bytes())
    }

    /// Writes a `u64` little-endian.
    pub fn put_u64le(&mut self, v: u64) -> Option<()> {
        self.put_bytes(&v.to_le_bytes())
    }

    /// Writes `n` zero bytes (reserved/padding regions).
    pub fn put_pad(&mut self, n: usize) -> Option<()> {
        let end = self.pos.checked_add(n)?;
        self.buf.get_mut(self.pos..end)?.fill(0);
        self.pos = end;
        Some(())
    }

    /// Writes a `u8`-length-prefixed byte field, bounds-checked to
    /// `min..=max` (and the implicit `u8::MAX` prefix limit).
    pub fn put_len8_bytes(&mut self, bytes: &[u8], min: usize, max: usize) -> Option<()> {
        if bytes.len() < min || bytes.len() > max || bytes.len() > u8::MAX as usize {
            return None;
        }
        self.put_u8(bytes.len() as u8)?;
        self.put_bytes(bytes)
    }

    /// Writes a `u8`-length-prefixed UTF-8 string field.
    pub fn put_len8_str(&mut self, s: &str, min: usize, max: usize) -> Option<()> {
        self.put_len8_bytes(s.as_bytes(), min, max)
    }

    /// Writes a `u16le`-length-prefixed byte field.
    pub fn put_len16_bytes(&mut self, bytes: &[u8], min: usize, max: usize) -> Option<()> {
        if bytes.len() < min || bytes.len() > max || bytes.len() > u16::MAX as usize {
            return None;
        }
        self.put_u16le(bytes.len() as u16)?;
        self.put_bytes(bytes)
    }

    /// Writes a `u32le`-length-prefixed byte field.
    pub fn put_len32_bytes(&mut self, bytes: &[u8], min: usize, max: usize) -> Option<()> {
        if bytes.len() < min || bytes.len() > max || bytes.len() > u32::MAX as usize {
            return None;
        }
        self.put_u32le(bytes.len() as u32)?;
        self.put_bytes(bytes)
    }
}

/// Sequential fail-closed frame reader.
///
/// Every `take_*`/`expect_*` returns `None` on a bound violation or short
/// input; [`Reader::finish_exact`] enforces the strict no-trailing-bytes rule
/// that generated decoders apply by default.
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    /// Starts reading at the beginning of `frame`.
    pub fn new(frame: &'a [u8]) -> Self {
        Self { buf: frame, pos: 0 }
    }

    /// Number of bytes consumed so far.
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Reads `n` raw bytes.
    pub fn take_bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.buf.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }

    /// Reads one byte.
    pub fn take_u8(&mut self) -> Option<u8> {
        let v = *self.buf.get(self.pos)?;
        self.pos += 1;
        Some(v)
    }

    /// Reads one byte, rejecting zero (frame-level constraint).
    pub fn take_nz_u8(&mut self) -> Option<u8> {
        match self.take_u8()? {
            0 => None,
            v => Some(v),
        }
    }

    /// Reads one byte and requires it to equal `v` (checked literal).
    pub fn expect_u8(&mut self, v: u8) -> Option<()> {
        (self.take_u8()? == v).then_some(())
    }

    /// Reads a `u16` little-endian.
    pub fn take_u16le(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take_bytes(2)?.try_into().ok()?))
    }

    /// Reads a `u32` little-endian.
    pub fn take_u32le(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take_bytes(4)?.try_into().ok()?))
    }

    /// Reads a `u64` little-endian.
    pub fn take_u64le(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take_bytes(8)?.try_into().ok()?))
    }

    /// Skips `n` bytes without inspecting them (reserved/padding regions —
    /// decoders deliberately do NOT require padding to be zero, matching the
    /// historical acceptance set).
    pub fn skip(&mut self, n: usize) -> Option<()> {
        self.take_bytes(n).map(|_| ())
    }

    /// Reads a `u8`-length-prefixed byte field, bounds-checked to `min..=max`.
    pub fn take_len8_bytes(&mut self, min: usize, max: usize) -> Option<&'a [u8]> {
        let n = self.take_u8()? as usize;
        if n < min || n > max {
            return None;
        }
        self.take_bytes(n)
    }

    /// Reads a `u8`-length-prefixed UTF-8 string field.
    pub fn take_len8_str(&mut self, min: usize, max: usize) -> Option<&'a str> {
        core::str::from_utf8(self.take_len8_bytes(min, max)?).ok()
    }

    /// Reads a `u16le`-length-prefixed byte field.
    pub fn take_len16_bytes(&mut self, min: usize, max: usize) -> Option<&'a [u8]> {
        let n = self.take_u16le()? as usize;
        if n < min || n > max {
            return None;
        }
        self.take_bytes(n)
    }

    /// Reads a `u32le`-length-prefixed byte field.
    pub fn take_len32_bytes(&mut self, min: usize, max: usize) -> Option<&'a [u8]> {
        let n = self.take_u32le()? as usize;
        if n < min || n > max {
            return None;
        }
        self.take_bytes(n)
    }

    /// Succeeds only when the whole frame has been consumed — the strict
    /// exact-length rule generated decoders apply by default.
    pub fn finish_exact(&self) -> Option<()> {
        (self.pos == self.buf.len()).then_some(())
    }
}

/// Writes the 4-byte frame header `[magic0, magic1, version, op]` — the
/// prefix every protocol frame in this crate starts with.
pub fn put_hdr(w: &mut Writer<'_>, m0: u8, m1: u8, version: u8, op: u8) -> Option<()> {
    w.put_u8(m0)?;
    w.put_u8(m1)?;
    w.put_u8(version)?;
    w.put_u8(op)
}

/// Checks the 4-byte frame header — the magic/version/op guard written once
/// instead of per decoder.
pub fn check_hdr(r: &mut Reader<'_>, m0: u8, m1: u8, version: u8, op: u8) -> Option<()> {
    r.expect_u8(m0)?;
    r.expect_u8(m1)?;
    r.expect_u8(version)?;
    r.expect_u8(op)
}

/// Decodes the opcode of a request frame after validating magic + version
/// (the per-protocol `decode_request_op` dispatch helper).
pub fn request_op(frame: &[u8], m0: u8, m1: u8, version: u8) -> Option<u8> {
    let mut r = Reader::new(frame);
    r.expect_u8(m0)?;
    r.expect_u8(m1)?;
    r.expect_u8(version)?;
    r.take_u8()
}

/// Runs an encode closure and reports success — used by generated fixed-size
/// encoders, whose buffers are sized by construction so the closure cannot
/// fail unless the declaration itself is wrong (guarded by `debug_assert!`
/// plus the golden tests).
#[inline]
pub fn build<F: FnOnce() -> Option<()>>(f: F) -> bool {
    f().is_some()
}

/// Deterministic negative-case helpers for protocol tests (TASK-0231 spirit).
pub mod testing {
    /// Asserts that `decodes` rejects every 1-byte truncation of `golden` and
    /// every single-byte mutation of its first `hdr_len` bytes (magic,
    /// version, opcode). Only valid for strict exact-length decoders; header
    /// decoders that tolerate trailing bytes need hand-written negatives.
    ///
    /// `decodes` returns whether the decoder accepted the frame.
    ///
    /// # Panics
    /// Panics (test assertion) when a truncated or mutated frame is accepted,
    /// or when `golden` exceeds the internal 256-byte scratch buffer.
    pub fn assert_reject_matrix(golden: &[u8], hdr_len: usize, decodes: &dyn Fn(&[u8]) -> bool) {
        assert!(decodes(golden), "golden frame itself must decode");
        assert!(golden.len() <= 256, "reject-matrix scratch buffer is 256 bytes");
        for cut in 0..golden.len() {
            assert!(!decodes(&golden[..cut]), "truncation to {cut} bytes must be rejected");
        }
        let mut scratch = [0u8; 256];
        let scratch = &mut scratch[..golden.len()];
        for i in 0..hdr_len.min(golden.len()) {
            for delta in [0x01u8, 0x80, 0xFF] {
                scratch.copy_from_slice(golden);
                scratch[i] ^= delta;
                assert!(
                    !decodes(scratch),
                    "mutation of header byte {i} (xor {delta:#04x}) must be rejected"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_reader_scalar_roundtrip() {
        let mut buf = [0u8; 32];
        let mut w = Writer::new(&mut buf);
        w.put_u8(0xAB).unwrap();
        w.put_u16le(0x1122).unwrap();
        w.put_u32le(0x3344_5566).unwrap();
        w.put_u64le(0x0102_0304_0506_0708).unwrap();
        w.put_pad(2).unwrap();
        let len = w.pos();
        assert_eq!(len, 1 + 2 + 4 + 8 + 2);
        // LE layout is the contract.
        assert_eq!(&buf[..4], &[0xAB, 0x22, 0x11, 0x66]);

        let mut r = Reader::new(&buf[..len]);
        assert_eq!(r.take_u8(), Some(0xAB));
        assert_eq!(r.take_u16le(), Some(0x1122));
        assert_eq!(r.take_u32le(), Some(0x3344_5566));
        assert_eq!(r.take_u64le(), Some(0x0102_0304_0506_0708));
        r.skip(2).unwrap();
        assert_eq!(r.finish_exact(), Some(()));
    }

    #[test]
    fn writer_rejects_overflow_and_bounds() {
        let mut buf = [0u8; 4];
        let mut w = Writer::new(&mut buf);
        assert_eq!(w.put_u64le(1), None);
        assert_eq!(w.put_nz_u8(0), None);
        let mut buf = [0u8; 8];
        let mut w = Writer::new(&mut buf);
        assert_eq!(w.put_len8_bytes(b"abc", 4, 8), None); // below min
        assert_eq!(w.put_len8_bytes(b"abc", 0, 2), None); // above max
        assert_eq!(w.put_len8_bytes(b"abcdefgh", 0, 8), None); // buffer too small (1 + 8 > 8)
        let mut buf = [0u8; 8];
        let mut w = Writer::new(&mut buf);
        assert_eq!(w.put_len8_bytes(b"abc", 1, 4), Some(()));
        assert_eq!(w.pos(), 4);
    }

    #[test]
    fn reader_is_fail_closed() {
        let mut r = Reader::new(&[3, b'a', b'b']);
        assert_eq!(r.take_len8_bytes(0, 8), None); // prefix promises 3, only 2 left
        let mut r = Reader::new(&[0]);
        assert_eq!(r.take_nz_u8(), None);
        let mut r = Reader::new(&[2, 0xFF, 0xFE]);
        assert_eq!(r.take_len8_str(0, 8), None); // invalid UTF-8
        let mut r = Reader::new(&[1, b'x', 9]);
        assert_eq!(r.take_len8_bytes(0, 8), Some(&b"x"[..]));
        assert_eq!(r.finish_exact(), None); // trailing byte
    }

    #[test]
    fn len16_and_len32_prefixes() {
        let mut buf = [0u8; 16];
        let mut w = Writer::new(&mut buf);
        w.put_len16_bytes(b"ab", 0, 16).unwrap();
        w.put_len32_bytes(b"c", 0, 16).unwrap();
        let len = w.pos();
        assert_eq!(&buf[..len], &[2, 0, b'a', b'b', 1, 0, 0, 0, b'c']);
        let mut r = Reader::new(&buf[..len]);
        assert_eq!(r.take_len16_bytes(0, 16), Some(&b"ab"[..]));
        assert_eq!(r.take_len32_bytes(1, 1), Some(&b"c"[..]));
        assert_eq!(r.finish_exact(), Some(()));
    }

    #[test]
    fn hdr_guard_and_request_op() {
        let mut buf = [0u8; 8];
        let mut w = Writer::new(&mut buf);
        put_hdr(&mut w, b'A', b'B', 1, 7).unwrap();
        let len = w.pos();
        let mut r = Reader::new(&buf[..len]);
        assert_eq!(check_hdr(&mut r, b'A', b'B', 1, 7), Some(()));
        assert_eq!(check_hdr(&mut Reader::new(&buf[..len]), b'A', b'B', 2, 7), None);
        assert_eq!(request_op(&buf[..len], b'A', b'B', 1), Some(7));
        assert_eq!(request_op(&buf[..3], b'A', b'B', 1), None);
    }
}
