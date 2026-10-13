// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The console's transmit queue (TASK-0330 S2a): the bytes a console write leaves
//! behind for the UART's own interrupt to send, instead of waiting for the transmitter under
//! the kernel lock. Measured on the board (`docs/board/measurements/2026-10-10-mgs-baseline/`):
//! the steady state's longest holders of the lock were the two console syscalls (10.5 ms and
//! 5.0 ms) and the bring-up's 16–19 ms holds were console writes too — a line costs its
//! length in serial time at 115 200 baud (87 µs a byte). Pure and bounded, NOT target-gated,
//! so the order and the bound run on the host; `hal::platform` owns the UART and the lock.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host (`tests` below)
//! INVARIANTS: first in, first out; never more than [`CAPACITY`] bytes; a push past the
//!   capacity is refused (the caller drains synchronously first, so the order holds).

/// Bytes the queue holds — about 0.7 s of a 115 200-baud console; the bring-up's bursts
/// overflow it and fall back to the synchronous write, the steady state never does.
pub const CAPACITY: usize = 8192;

/// A bounded FIFO of console bytes.
pub struct TxRing {
    buf: [u8; CAPACITY],
    head: usize,
    len: usize,
}

impl TxRing {
    pub const fn new() -> Self {
        Self { buf: [0; CAPACITY], head: 0, len: 0 }
    }

    /// Queues `byte`; `false` when full (nothing is dropped — the caller decides).
    pub fn push(&mut self, byte: u8) -> bool {
        if self.len == CAPACITY {
            return false;
        }
        self.buf[(self.head + self.len) % CAPACITY] = byte;
        self.len += 1;
        true
    }

    /// The oldest byte, if any.
    pub fn pop(&mut self) -> Option<u8> {
        if self.len == 0 {
            return None;
        }
        let byte = self.buf[self.head];
        self.head = (self.head + 1) % CAPACITY;
        self.len -= 1;
        Some(byte)
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for TxRing {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_leave_in_the_order_they_came() {
        let mut q = TxRing::new();
        for b in b"KSELFTEST: console off lock ok\n" {
            assert!(q.push(*b));
        }
        let mut out = alloc::vec::Vec::new();
        while let Some(b) = q.pop() {
            out.push(b);
        }
        assert_eq!(&out, b"KSELFTEST: console off lock ok\n");
        assert!(q.is_empty());
        assert_eq!(q.pop(), None);
    }

    #[test]
    fn test_reject_push_past_the_capacity_keeps_every_queued_byte() {
        let mut q = TxRing::new();
        for i in 0..CAPACITY {
            assert!(q.push(i as u8));
        }
        assert!(!q.push(0xee), "the queue is full: refused, not dropped");
        assert_eq!(q.len(), CAPACITY);
        assert_eq!(q.pop(), Some(0));
        assert!(q.push(0xee), "one slot free again");
        // The wrap-around keeps the order.
        for i in 1..CAPACITY {
            assert_eq!(q.pop(), Some(i as u8));
        }
        assert_eq!(q.pop(), Some(0xee));
        assert!(q.is_empty());
    }
}
