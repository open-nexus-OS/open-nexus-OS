// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Allocation-free marker lines for the gateway loop — a bounded
//! stack buffer written in ONE `debug_write` record (no per-line `format!`
//! on the never-freeing bump allocator; `debug_println` folds for armed
//! services, `debug_write` is the raw witness path).
//! OWNERS: @runtime
//! STATUS: Experimental

/// Longest marker line the gateway prints.
const LINE_CAPACITY: usize = 160;

pub(crate) struct Line {
    buf: [u8; LINE_CAPACITY],
    len: usize,
}

impl Line {
    pub(crate) const fn new() -> Self {
        Self { buf: [0; LINE_CAPACITY], len: 0 }
    }

    /// Starts a line with the service prefix.
    pub(crate) fn prefixed(text: &str) -> Self {
        let mut l = Self::new();
        l.push(b"ingressd: ");
        l.push(text.as_bytes());
        l
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) -> &mut Self {
        let room = LINE_CAPACITY - self.len;
        let n = bytes.len().min(room);
        self.buf[self.len..self.len + n].copy_from_slice(&bytes[..n]);
        self.len += n;
        self
    }

    pub(crate) fn push_str(&mut self, s: &str) -> &mut Self {
        self.push(s.as_bytes())
    }

    /// Decimal, no allocation.
    pub(crate) fn push_dec(&mut self, mut v: u64) -> &mut Self {
        let mut digits = [0u8; 20];
        let mut i = digits.len();
        loop {
            i -= 1;
            digits[i] = b'0' + (v % 10) as u8;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        self.push(&digits[i..])
    }

    /// Writes the line + `\n` as one UART record.
    pub(crate) fn emit(&mut self) {
        self.push(b"\n");
        let _ = nexus_abi::debug_write(&self.buf[..self.len]);
    }
}
