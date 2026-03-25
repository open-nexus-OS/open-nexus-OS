// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: In-memory loopback shims for deterministic netstackd bring-up
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Planned in netstackd host seam tests
//! ADR: docs/adr/0005-dsoftbus-architecture.md

pub(crate) const LOOPBUF_CAPACITY: usize = 128;

#[derive(Clone, Copy)]
pub(crate) struct LoopBuf {
    buf: [u8; LOOPBUF_CAPACITY],
    r: usize,
    w: usize,
    len: usize,
}

impl LoopBuf {
    pub(crate) const fn new() -> Self {
        Self { buf: [0u8; LOOPBUF_CAPACITY], r: 0, w: 0, len: 0 }
    }

    #[must_use]
    pub(crate) fn push(&mut self, data: &[u8]) -> usize {
        let mut n = 0;
        for &b in data {
            if self.len == self.buf.len() {
                break;
            }
            self.buf[self.w] = b;
            self.w = (self.w + 1) % self.buf.len();
            self.len += 1;
            n += 1;
        }
        n
    }

    #[must_use]
    pub(crate) fn pop(&mut self, out: &mut [u8]) -> usize {
        let mut n = 0;
        for slot in out.iter_mut() {
            if self.len == 0 {
                break;
            }
            *slot = self.buf[self.r];
            self.r = (self.r + 1) % self.buf.len();
            self.len -= 1;
            n += 1;
        }
        n
    }
}

#[must_use]
pub(crate) fn reject_oversized_loopback_payload(len: usize) -> bool {
    len > LOOPBUF_CAPACITY
}
