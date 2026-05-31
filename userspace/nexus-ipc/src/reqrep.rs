// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Deterministic request/reply correlation helpers (RFC-0019).
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Unit-tested (host)
//!
//! This module is intentionally small and no_std-friendly: it avoids heap allocations and uses
//! fixed-capacity storage so it can be reused in os-lite services.

#![forbid(unsafe_code)]

use core::fmt;

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
extern crate alloc;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
use alloc::vec::Vec;
#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
use std::vec::Vec;

use crate::budget;
use crate::{Client, IpcError, Wait};

/// Monotonic nonce generator (no randomness; deterministic).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NonceGen {
    next: u64,
}

impl NonceGen {
    /// Create a generator starting at `start` (the first `next_nonce()` returns `start`).
    pub const fn new(start: u64) -> Self {
        Self { next: start }
    }

    /// Returns the next nonce value and increments the generator.
    pub fn next_nonce(&mut self) -> u64 {
        let out = self.next;
        self.next = self.next.wrapping_add(1);
        out
    }
}

impl Iterator for NonceGen {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        Some(self.next_nonce())
    }
}

/// Errors produced when inserting an unmatched reply into a bounded buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushError {
    /// The reply frame exceeded the configured maximum size.
    TooLarge,
    /// Buffer capacity is zero (cannot store any replies).
    NoCapacity,
    /// A reply for this nonce is already buffered.
    NonceCollision,
}

impl fmt::Display for PushError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(f, "reply frame too large"),
            Self::NoCapacity => write!(f, "no pending-reply capacity"),
            Self::NonceCollision => write!(f, "reply nonce collision"),
        }
    }
}

/// Bounded store of unmatched replies keyed by nonce.
///
/// Determinism contract:
/// - if full, we evict the oldest entry using a deterministic round-robin cursor.
/// - drops are counted explicitly.
pub struct ReplyBuffer<const PENDING: usize, const MAX_FRAME: usize> {
    slots: [Slot<MAX_FRAME>; PENDING],
    evict_cursor: usize,
    drops: u64,
}

/// Bounded stash of unmatched frames without a nonce.
///
/// This is for legacy/unversioned protocols where adding a nonce is not yet possible, but callers
/// still need deterministic "shared inbox" semantics (never drop unrelated frames silently).
///
/// Determinism contract:
/// - bounded capacity with deterministic eviction (round-robin cursor)
/// - explicit drop counter
pub struct FrameStash<const PENDING: usize, const MAX_FRAME: usize> {
    slots: [FrameSlot<MAX_FRAME>; PENDING],
    evict_cursor: usize,
    drops: u64,
}

#[derive(Clone, Copy)]
struct FrameSlot<const MAX: usize> {
    used: bool,
    len: usize,
    buf: [u8; MAX],
}

impl<const MAX: usize> FrameSlot<MAX> {
    const EMPTY: Self = Self { used: false, len: 0, buf: [0u8; MAX] };
}

impl<const PENDING: usize, const MAX_FRAME: usize> FrameStash<PENDING, MAX_FRAME> {
    /// Create an empty frame stash.
    pub const fn new() -> Self {
        Self { slots: [FrameSlot::EMPTY; PENDING], evict_cursor: 0, drops: 0 }
    }

    /// Number of frames dropped due to bounded capacity or oversized input.
    pub const fn drops(&self) -> u64 {
        self.drops
    }

    /// Insert an unmatched frame into the stash.
    ///
    /// If the stash is full, the oldest entry is evicted deterministically and `drops()` is
    /// incremented.
    pub fn push(&mut self, frame: &[u8]) -> core::result::Result<(), PushError> {
        if PENDING == 0 {
            self.drops = self.drops.saturating_add(1);
            return Err(PushError::NoCapacity);
        }
        if frame.len() > MAX_FRAME {
            self.drops = self.drops.saturating_add(1);
            return Err(PushError::TooLarge);
        }

        if let Some(slot) = self.slots.iter_mut().find(|s| !s.used) {
            slot.used = true;
            slot.len = frame.len();
            slot.buf[..frame.len()].copy_from_slice(frame);
            return Ok(());
        }

        let idx = self.evict_cursor % PENDING;
        self.evict_cursor = (self.evict_cursor + 1) % PENDING;
        self.drops = self.drops.saturating_add(1);
        let slot = &mut self.slots[idx];
        slot.used = true;
        slot.len = frame.len();
        slot.buf[..frame.len()].copy_from_slice(frame);
        Ok(())
    }

    /// Removes the first stashed frame matching `pred` and copies it into `out`.
    pub fn take_into_where(
        &mut self,
        out: &mut [u8],
        pred: impl Fn(&[u8]) -> bool,
    ) -> Option<usize> {
        for slot in &mut self.slots {
            if slot.used && pred(&slot.buf[..slot.len]) {
                if out.len() < slot.len {
                    return None;
                }
                out[..slot.len].copy_from_slice(&slot.buf[..slot.len]);
                let n = slot.len;
                *slot = FrameSlot::EMPTY;
                return Some(n);
            }
        }
        None
    }
}

impl<const PENDING: usize, const MAX_FRAME: usize> Default for FrameStash<PENDING, MAX_FRAME> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
struct Slot<const MAX: usize> {
    used: bool,
    nonce: u64,
    len: usize,
    buf: [u8; MAX],
}

impl<const MAX: usize> Slot<MAX> {
    const EMPTY: Self = Self { used: false, nonce: 0, len: 0, buf: [0u8; MAX] };
}

impl<const PENDING: usize, const MAX_FRAME: usize> ReplyBuffer<PENDING, MAX_FRAME> {
    /// Create an empty reply buffer.
    pub const fn new() -> Self {
        Self { slots: [Slot::EMPTY; PENDING], evict_cursor: 0, drops: 0 }
    }

    /// Number of replies dropped due to bounded capacity or collisions.
    pub const fn drops(&self) -> u64 {
        self.drops
    }

    /// Returns true if a reply for `nonce` is currently buffered.
    pub fn contains(&self, nonce: u64) -> bool {
        self.slots.iter().any(|s| s.used && s.nonce == nonce)
    }

    /// Insert a reply frame keyed by `nonce`.
    ///
    /// If the buffer is full, the oldest entry is evicted deterministically and `drops()` is
    /// incremented.
    pub fn push(&mut self, nonce: u64, frame: &[u8]) -> core::result::Result<(), PushError> {
        if PENDING == 0 {
            self.drops = self.drops.saturating_add(1);
            return Err(PushError::NoCapacity);
        }
        if frame.len() > MAX_FRAME {
            self.drops = self.drops.saturating_add(1);
            return Err(PushError::TooLarge);
        }
        if self.contains(nonce) {
            self.drops = self.drops.saturating_add(1);
            return Err(PushError::NonceCollision);
        }

        // Prefer a free slot.
        if let Some(slot) = self.slots.iter_mut().find(|s| !s.used) {
            slot.used = true;
            slot.nonce = nonce;
            slot.len = frame.len();
            slot.buf[..frame.len()].copy_from_slice(frame);
            return Ok(());
        }

        // Full: evict deterministically.
        let idx = self.evict_cursor % PENDING;
        self.evict_cursor = (self.evict_cursor + 1) % PENDING;
        self.drops = self.drops.saturating_add(1);

        let slot = &mut self.slots[idx];
        slot.used = true;
        slot.nonce = nonce;
        slot.len = frame.len();
        slot.buf[..frame.len()].copy_from_slice(frame);
        Ok(())
    }

    /// Remove the buffered reply for `nonce` and copy it into `out`.
    ///
    /// Returns the number of bytes copied, or `None` if no buffered reply matches.
    ///
    /// If `out` is too small, the reply is left buffered and `None` is returned.
    pub fn take_into(&mut self, nonce: u64, out: &mut [u8]) -> Option<usize> {
        for slot in &mut self.slots {
            if slot.used && slot.nonce == nonce {
                if out.len() < slot.len {
                    return None;
                }
                out[..slot.len].copy_from_slice(&slot.buf[..slot.len]);
                let n = slot.len;
                *slot = Slot::EMPTY;
                return Some(n);
            }
        }
        None
    }
}

impl<const PENDING: usize, const MAX_FRAME: usize> Default for ReplyBuffer<PENDING, MAX_FRAME> {
    fn default() -> Self {
        Self::new()
    }
}

/// Receive from a shared reply inbox until `expected_nonce` is observed, using a bounded buffer
/// to retain out-of-order/unrelated replies.
///
/// This is deterministic (no sleeps, no wall-clock): callers provide an explicit `max_iters` bound.
///
/// - `extract_nonce(frame)` MUST return the reply's correlation nonce if present.
/// - Replies with `None` nonce are ignored (but still count against iteration budget).
pub fn recv_match_bounded<const PENDING: usize, const MAX_FRAME: usize>(
    inbox: &impl Client,
    pending: &mut ReplyBuffer<PENDING, MAX_FRAME>,
    expected_nonce: u64,
    max_iters: usize,
    extract_nonce: impl Fn(&[u8]) -> Option<u64>,
) -> crate::Result<Vec<u8>> {
    // First: see if we already buffered it.
    let mut tmp = [0u8; MAX_FRAME];
    if let Some(n) = pending.take_into(expected_nonce, &mut tmp) {
        return Ok(tmp[..n].to_vec());
    }

    for _ in 0..max_iters {
        match inbox.recv(Wait::NonBlocking) {
            Ok(frame) => {
                if let Some(nonce) = extract_nonce(&frame) {
                    if nonce == expected_nonce {
                        return Ok(frame);
                    }
                    // Buffer for later matching. Ignore buffer errors deterministically.
                    let _ = pending.push(nonce, &frame);
                }
            }
            Err(IpcError::WouldBlock) => {
                // No progress this iteration.
            }
            Err(other) => return Err(other),
        }
    }
    Err(IpcError::Timeout)
}

/// Receive from a shared reply inbox until `expected_nonce` is observed, using an explicit deadline
/// and cooperative yielding (via `budget::Clock`).
///
/// This is the preferred helper for OS code (QEMU/ICOUNT): it avoids both wall-clock sleeps and
/// "spin without yielding" loops.
pub fn recv_match_until<const PENDING: usize, const MAX_FRAME: usize>(
    clock: &impl budget::Clock,
    inbox: &impl Client,
    pending: &mut ReplyBuffer<PENDING, MAX_FRAME>,
    expected_nonce: u64,
    deadline_ns: u64,
    extract_nonce: impl Fn(&[u8]) -> Option<u64>,
) -> crate::Result<Vec<u8>> {
    // First: see if we already buffered it.
    let mut tmp = [0u8; MAX_FRAME];
    if let Some(n) = pending.take_into(expected_nonce, &mut tmp) {
        return Ok(tmp[..n].to_vec());
    }

    budget::retry_ipc_until(clock, deadline_ns, || match inbox.recv(Wait::NonBlocking) {
        Ok(frame) => {
            if let Some(nonce) = extract_nonce(&frame) {
                if nonce == expected_nonce {
                    return Ok(frame);
                }
                let _ = pending.push(nonce, &frame);
            }
            // Not a match; keep receiving until deadline.
            Err(IpcError::WouldBlock)
        }
        Err(IpcError::WouldBlock) => Err(IpcError::WouldBlock),
        Err(other) => Err(other),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loopback_channel;
    use crate::Server as _;

    #[test]
    fn test_nonce_gen_monotonic() {
        let mut g = NonceGen::new(1);
        assert_eq!(g.next_nonce(), 1);
        assert_eq!(g.next_nonce(), 2);
        assert_eq!(g.next_nonce(), 3);
    }

    #[test]
    fn test_reply_buffer_out_of_order_take() {
        let mut buf: ReplyBuffer<4, 16> = ReplyBuffer::new();

        buf.push(2, b"two").unwrap();
        buf.push(1, b"one").unwrap();

        let mut out = [0u8; 16];
        let n1 = buf.take_into(1, &mut out).unwrap();
        assert_eq!(&out[..n1], b"one");

        let n2 = buf.take_into(2, &mut out).unwrap();
        assert_eq!(&out[..n2], b"two");
    }

    #[test]
    fn test_reply_buffer_bounded_drop_is_deterministic() {
        let mut buf: ReplyBuffer<2, 8> = ReplyBuffer::new();
        let mut out = [0u8; 8];

        buf.push(1, b"a").unwrap();
        buf.push(2, b"b").unwrap();
        assert_eq!(buf.drops(), 0);

        // Full; this will evict deterministically (oldest slot via cursor).
        buf.push(3, b"c").unwrap();
        assert_eq!(buf.drops(), 1);

        // One of 1 or 2 is gone; 3 must be present.
        assert!(buf.take_into(3, &mut out).is_some());
        let still_1 = buf.take_into(1, &mut out).is_some();
        let still_2 = buf.take_into(2, &mut out).is_some();
        assert_ne!(still_1, still_2);
    }

    fn nonce_tail(frame: &[u8]) -> Option<u64> {
        if frame.len() < 8 {
            return None;
        }
        let tail = &frame[frame.len() - 8..];
        let mut b = [0u8; 8];
        b.copy_from_slice(tail);
        Some(u64::from_le_bytes(b))
    }

    #[test]
    fn test_recv_match_buffers_out_of_order() {
        let (client, server) = loopback_channel();
        let mut pending: ReplyBuffer<4, 32> = ReplyBuffer::new();

        // Inject out-of-order replies into the shared inbox.
        let mut r2 = b"rsp2".to_vec();
        r2.extend_from_slice(&2u64.to_le_bytes());
        server.send(&r2, Wait::Blocking).unwrap();

        let mut r1 = b"rsp1".to_vec();
        r1.extend_from_slice(&1u64.to_le_bytes());
        server.send(&r1, Wait::Blocking).unwrap();

        let got1 = recv_match_bounded(&client, &mut pending, 1, 32, nonce_tail).unwrap();
        assert_eq!(&got1[..4], b"rsp1");

        // The earlier r2 should be buffered and returned next without receiving more.
        let got2 = recv_match_bounded(&client, &mut pending, 2, 1, nonce_tail).unwrap();
        assert_eq!(&got2[..4], b"rsp2");
    }

    #[test]
    fn test_recv_match_buffers_mixed_protocol_nonces() {
        let (client, server) = loopback_channel();
        let mut pending: ReplyBuffer<4, 64> = ReplyBuffer::new();

        // Inject a "policyd v2 delegated" style reply (nonce=u32 in bytes[4..8]).
        let policyd_nonce: u32 = 0xAABB_CCDD;
        let policyd = [
            b'P',
            b'O',
            2,
            0x85, // OP|0x80 (value doesn't matter for nonce extraction)
            (policyd_nonce & 0xFF) as u8,
            ((policyd_nonce >> 8) & 0xFF) as u8,
            ((policyd_nonce >> 16) & 0xFF) as u8,
            ((policyd_nonce >> 24) & 0xFF) as u8,
            0,
            0,
        ];
        server.send(&policyd, Wait::Blocking).unwrap();

        // Inject an "rngd entropy" style reply (nonce=u32 in bytes[5..9]).
        let rngd_nonce: u32 = 0x1122_3344;
        let mut rngd = Vec::new();
        rngd.extend_from_slice(&[b'R', b'G', 1, 0x81, 0]); // magic, ver, op|0x80, STATUS_OK
        rngd.extend_from_slice(&rngd_nonce.to_le_bytes());
        rngd.extend_from_slice(&[1, 2, 3, 4]); // small payload
        server.send(&rngd, Wait::Blocking).unwrap();

        fn extract(frame: &[u8]) -> Option<u64> {
            if frame.len() == 10 && frame[0] == b'P' && frame[1] == b'O' && frame[2] == 2 {
                return Some(u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]) as u64);
            }
            if frame.len() >= 9 && frame[0] == b'R' && frame[1] == b'G' && frame[2] == 1 {
                return Some(u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]) as u64);
            }
            None
        }

        // First match rngd; this should buffer the policyd reply.
        let got_rngd =
            recv_match_bounded(&client, &mut pending, rngd_nonce as u64, 32, extract).unwrap();
        assert!(got_rngd.starts_with(&[b'R', b'G', 1]));

        // Then match policyd without receiving more; it must come from the buffer.
        let got_pol =
            recv_match_bounded(&client, &mut pending, policyd_nonce as u64, 1, extract).unwrap();
        assert_eq!(got_pol.len(), 10);
        assert_eq!(&got_pol[..3], &[b'P', b'O', 2]);
    }

    #[test]
    fn test_recv_match_times_out_deterministically() {
        let (client, _server) = loopback_channel();
        let mut pending: ReplyBuffer<2, 16> = ReplyBuffer::new();
        let err = recv_match_bounded(&client, &mut pending, 1, 8, nonce_tail).unwrap_err();
        assert_eq!(err, IpcError::Timeout);
    }

    #[test]
    fn test_frame_stash_takes_matching_frame() {
        let mut stash: FrameStash<2, 8> = FrameStash::new();
        stash.push(b"AAAA").unwrap();
        stash.push(b"BBBB").unwrap();
        let mut out = [0u8; 8];
        let n = stash.take_into_where(&mut out, |f| f == b"BBBB").expect("should find BBBB");
        assert_eq!(&out[..n], b"BBBB");
        assert_eq!(stash.drops(), 0);
    }
}
