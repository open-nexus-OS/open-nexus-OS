//! Inter-process communication primitives.

/// Message passing channel placeholder supporting request/response.
pub struct Channel;

impl Channel {
    pub fn send(&self, _bytes: &[u8]) {
        // Future implementation will enqueue payloads into shared memory windows.
    }

    pub fn receive(&self, _buffer: &mut [u8]) -> usize {
        // The scheduler will provide doorbell notifications when data arrives.
        0
    }
}

/// Shared memory window metadata stub.
pub struct ShmWindow {
    pub base: usize,
    pub length: usize,
}

impl ShmWindow {
    pub const fn new(base: usize, length: usize) -> Self {
        Self { base, length }
    }
}
