// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Buffer budget — bounded byte + count accounting for device buffers.
//!
//! A device server hands out buffers (VMO-backed command/staging/scanout memory) to clients.
//! [`BufferBudget`] caps both the **total bytes** and the **number of live buffers** so a
//! misbehaving or compromised client cannot exhaust device memory (DoS). It is pure
//! accounting — the actual VMO handles live in the device server; this just gates `reserve` /
//! `release` deterministically. Over-budget requests are rejected with no partial state.

/// Buffer-budget errors (deterministic; no panics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferError {
    /// Reserving would exceed the total byte budget.
    ByteBudgetExceeded,
    /// Reserving would exceed the live-buffer count budget.
    CountBudgetExceeded,
    /// Releasing more bytes than reserved, or with no live buffers.
    Underflow,
}

/// Bounded byte + count accounting for device buffers.
#[derive(Debug, Clone, Copy)]
pub struct BufferBudget {
    total_bytes: usize,
    used_bytes: usize,
    max_buffers: usize,
    live_buffers: usize,
}

impl BufferBudget {
    /// Create a budget capping total bytes and the number of simultaneously live buffers.
    pub fn new(total_bytes: usize, max_buffers: usize) -> Self {
        Self { total_bytes, used_bytes: 0, max_buffers, live_buffers: 0 }
    }

    /// Reserve `bytes` for one buffer. Rejects (no state change) if it would exceed either
    /// the byte budget or the live-buffer count.
    pub fn reserve(&mut self, bytes: usize) -> Result<(), BufferError> {
        if self.live_buffers >= self.max_buffers {
            return Err(BufferError::CountBudgetExceeded);
        }
        // `checked_add` guards against overflow folding a huge request into a small sum.
        let new_used = self.used_bytes.checked_add(bytes).ok_or(BufferError::ByteBudgetExceeded)?;
        if new_used > self.total_bytes {
            return Err(BufferError::ByteBudgetExceeded);
        }
        self.used_bytes = new_used;
        self.live_buffers += 1;
        Ok(())
    }

    /// Release one buffer of `bytes`. Errors on underflow (releasing more than reserved or
    /// with no live buffers) rather than wrapping.
    pub fn release(&mut self, bytes: usize) -> Result<(), BufferError> {
        if self.live_buffers == 0 || bytes > self.used_bytes {
            return Err(BufferError::Underflow);
        }
        self.used_bytes -= bytes;
        self.live_buffers -= 1;
        Ok(())
    }

    /// Bytes currently reserved.
    pub fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    /// Bytes still available under the byte budget.
    pub fn available_bytes(&self) -> usize {
        self.total_bytes - self.used_bytes
    }

    /// Number of buffers currently live.
    pub fn live_buffers(&self) -> usize {
        self.live_buffers
    }

    /// Maximum number of simultaneously live buffers.
    pub fn capacity_buffers(&self) -> usize {
        self.max_buffers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserve_up_to_byte_budget_then_rejects() {
        let mut b = BufferBudget::new(1000, 100);
        b.reserve(600).unwrap();
        b.reserve(400).unwrap();
        assert_eq!(b.used_bytes(), 1000);
        assert_eq!(b.available_bytes(), 0);
        // One more byte over budget → rejected, no state change.
        assert_eq!(b.reserve(1), Err(BufferError::ByteBudgetExceeded));
        assert_eq!(b.used_bytes(), 1000);
        assert_eq!(b.live_buffers(), 2);
    }

    #[test]
    fn reserve_up_to_count_budget_then_rejects() {
        let mut b = BufferBudget::new(1_000_000, 2);
        b.reserve(1).unwrap();
        b.reserve(1).unwrap();
        assert_eq!(b.reserve(1), Err(BufferError::CountBudgetExceeded));
        assert_eq!(b.live_buffers(), 2);
    }

    #[test]
    fn release_frees_bytes_and_count() {
        let mut b = BufferBudget::new(1000, 100);
        b.reserve(400).unwrap();
        b.reserve(300).unwrap();
        b.release(400).unwrap();
        assert_eq!(b.used_bytes(), 300);
        assert_eq!(b.live_buffers(), 1);
        // After release, room is available again.
        b.reserve(700).unwrap();
        assert_eq!(b.used_bytes(), 1000);
    }

    #[test]
    fn release_underflow_errors() {
        let mut b = BufferBudget::new(1000, 100);
        assert_eq!(b.release(1), Err(BufferError::Underflow));
        b.reserve(100).unwrap();
        assert_eq!(b.release(200), Err(BufferError::Underflow));
        assert_eq!(b.used_bytes(), 100);
    }

    #[test]
    fn huge_request_does_not_overflow_into_acceptance() {
        let mut b = BufferBudget::new(1000, 100);
        assert_eq!(b.reserve(usize::MAX), Err(BufferError::ByteBudgetExceeded));
        assert_eq!(b.used_bytes(), 0);
    }
}
