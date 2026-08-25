// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Pure virtqueue request-ring logic (TASK-0314) — descriptor
//! free-list, chained allocation, in-flight accounting and used-ring
//! reclamation over an abstract `RingMem` transport. cfg-free so the
//! framing/reuse/wraparound/sequential-regression proofs run as HOST unit
//! tests against a mock device; the MMIO backend implements `RingMem`
//! with volatile accessors over the real queue memory. The v1 driver
//! published descriptor 0 for every request and only compared a used-idx
//! counter — the root of the TASK-0293 long-sequential-read deadlock;
//! here a descriptor is reusable exactly when the device returned its
//! chain head through the used ring, never earlier.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (mock transport)
//! ADR: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md

/// Descriptor chain link flag.
pub const DESC_F_NEXT: u16 = 1;
/// Device-writable descriptor flag.
pub const DESC_F_WRITE: u16 = 2;

/// One virtqueue descriptor (guest view; the transport owns volatility).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Desc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
}

/// Abstract transport into the shared queue memory. The device sees what
/// the implementor writes; `used_*` reads are UNTRUSTED device input —
/// `Ring` validates ids before touching its own state.
pub trait RingMem {
    /// Write descriptor `index` (with `next` already resolved by the ring).
    fn write_desc(&mut self, index: u16, desc: Desc, next: u16);
    /// Publish `head` in the avail ring and bump the avail index.
    fn publish_avail(&mut self, head: u16);
    /// Current used index (device-written, free-running u16).
    fn used_idx(&self) -> u16;
    /// Used element at ring position `slot` → chain-head id.
    fn used_head(&self, slot: u16) -> u32;
}

/// Error surface of the pure ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RingError {
    /// Not enough free descriptors for the requested chain.
    Exhausted,
    /// The device returned an id that is out of range or not in flight —
    /// device-model corruption; the caller must fail the request loudly.
    BadUsedId,
}

const NONE: u16 = u16::MAX;

/// Descriptor free-list + in-flight accounting for a queue of `N`
/// descriptors. `N` ≤ 32768 (u16 ids, `NONE` sentinel).
pub struct Ring<const N: usize> {
    /// Free-list head (`NONE` = empty) threaded through `next_free`.
    free_head: u16,
    next_free: [u16; N],
    /// Chain link written alongside each allocated descriptor so the
    /// chain can be freed when its HEAD comes back through the used ring.
    chain_next: [u16; N],
    /// True while the descriptor belongs to an in-flight chain.
    in_flight: [bool; N],
    /// Next used-ring slot to reclaim (free-running, wraps with u16).
    last_used: u16,
}

impl<const N: usize> Default for Ring<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Ring<N> {
    pub fn new() -> Self {
        let mut next_free = [NONE; N];
        for (i, slot) in next_free.iter_mut().enumerate().take(N.saturating_sub(1)) {
            *slot = (i + 1) as u16;
        }
        Self {
            free_head: if N == 0 { NONE } else { 0 },
            next_free,
            chain_next: [NONE; N],
            in_flight: [false; N],
            last_used: 0,
        }
    }

    /// Free descriptors currently available.
    pub fn free_count(&self) -> usize {
        let mut n = 0;
        let mut cur = self.free_head;
        while cur != NONE {
            n += 1;
            cur = self.next_free[cur as usize];
        }
        n
    }

    /// Allocates a chain for `descs`, writes them through `mem` with the
    /// ring-resolved `next` links, publishes the head. Returns the head id
    /// (the token the used ring hands back on completion).
    pub fn submit(&mut self, mem: &mut impl RingMem, descs: &[Desc]) -> Result<u16, RingError> {
        if descs.is_empty() {
            return Err(RingError::Exhausted);
        }
        // Reserve first — a partial chain must never reach the device.
        let mut ids = [NONE; 8];
        if descs.len() > ids.len() {
            return Err(RingError::Exhausted);
        }
        for slot in ids.iter_mut().take(descs.len()) {
            if self.free_head == NONE {
                // Roll back the partial reservation.
                for &id in ids.iter().take_while(|&&id| id != NONE) {
                    self.push_free(id);
                }
                return Err(RingError::Exhausted);
            }
            let id = self.free_head;
            self.free_head = self.next_free[id as usize];
            *slot = id;
        }
        for (i, desc) in descs.iter().enumerate() {
            let id = ids[i];
            let next = if i + 1 < descs.len() { ids[i + 1] } else { 0 };
            let mut d = *desc;
            if i + 1 < descs.len() {
                d.flags |= DESC_F_NEXT;
            } else {
                d.flags &= !DESC_F_NEXT;
            }
            mem.write_desc(id, d, next);
            self.chain_next[id as usize] = if i + 1 < descs.len() { ids[i + 1] } else { NONE };
            self.in_flight[id as usize] = true;
        }
        mem.publish_avail(ids[0]);
        Ok(ids[0])
    }

    /// Reclaims ONE completed chain from the used ring, if any. Returns the
    /// chain head id. Ids are untrusted device input: out-of-range or
    /// not-in-flight ids surface as `BadUsedId`, never as index panics.
    pub fn complete(&mut self, mem: &impl RingMem) -> Result<Option<u16>, RingError> {
        if mem.used_idx() == self.last_used {
            return Ok(None);
        }
        let slot = self.last_used % (N as u16);
        let head = mem.used_head(slot);
        if head >= N as u32 || !self.in_flight[head as usize] {
            return Err(RingError::BadUsedId);
        }
        // Free the whole chain (head is only marked used by the device
        // when the entire chain finished).
        let mut cur = head as u16;
        while cur != NONE {
            let next = self.chain_next[cur as usize];
            self.in_flight[cur as usize] = false;
            self.chain_next[cur as usize] = NONE;
            self.push_free(cur);
            cur = next;
        }
        self.last_used = self.last_used.wrapping_add(1);
        Ok(Some(head as u16))
    }

    fn push_free(&mut self, id: u16) {
        self.next_free[id as usize] = self.free_head;
        self.free_head = id;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock transport: records descriptor writes and simulates a device
    /// that completes published chains (optionally out of order).
    struct MockMem {
        descs: Vec<(Desc, u16)>,
        avail: Vec<u16>,
        used: Vec<u32>,
        used_idx: u16,
        /// Device-side cursor into `avail`.
        consumed: usize,
    }

    impl MockMem {
        fn new(n: usize) -> Self {
            Self {
                descs: vec![(Desc { addr: 0, len: 0, flags: 0 }, 0); n],
                avail: Vec::new(),
                used: Vec::new(),
                used_idx: 0,
                consumed: 0,
            }
        }
        /// Device completes the next published chain in FIFO order.
        fn device_complete_next(&mut self) {
            assert!(self.consumed < self.avail.len(), "device has nothing to complete");
            let head = self.avail[self.consumed];
            self.consumed += 1;
            let n = self.used.len();
            let ring_len = self.descs.len();
            if n < ring_len {
                self.used.push(head as u32);
            } else {
                self.used[n % ring_len] = head as u32;
            }
            self.used_idx = self.used_idx.wrapping_add(1);
        }
        /// Device completes a SPECIFIC published head (out-of-order).
        fn device_complete_head(&mut self, head: u16) {
            let ring_len = self.descs.len();
            let pos = (self.used_idx as usize) % ring_len;
            if self.used.len() <= pos {
                self.used.resize(pos + 1, 0);
            }
            self.used[pos] = head as u32;
            self.used_idx = self.used_idx.wrapping_add(1);
        }
    }

    impl RingMem for MockMem {
        fn write_desc(&mut self, index: u16, desc: Desc, next: u16) {
            self.descs[index as usize] = (desc, next);
        }
        fn publish_avail(&mut self, head: u16) {
            self.avail.push(head);
        }
        fn used_idx(&self) -> u16 {
            self.used_idx
        }
        fn used_head(&self, slot: u16) -> u32 {
            self.used[(slot as usize) % self.descs.len()]
        }
    }

    fn three_chain(tag: u64) -> [Desc; 3] {
        [
            Desc { addr: tag, len: 16, flags: 0 },
            Desc { addr: tag + 1, len: 4096, flags: DESC_F_WRITE },
            Desc { addr: tag + 2, len: 1, flags: DESC_F_WRITE },
        ]
    }

    #[test]
    fn test_chain_framing_and_links() {
        let mut mem = MockMem::new(8);
        let mut ring: Ring<8> = Ring::new();
        let head = ring.submit(&mut mem, &three_chain(100)).expect("submit");
        assert_eq!(head, 0);
        // First two descriptors carry NEXT; the tail does not.
        let (d0, n0) = mem.descs[0];
        let (d1, n1) = mem.descs[1];
        let (d2, _) = mem.descs[2];
        assert_eq!(d0.flags & DESC_F_NEXT, DESC_F_NEXT);
        assert_eq!(n0, 1);
        assert_eq!(d1.flags & DESC_F_NEXT, DESC_F_NEXT);
        assert_eq!(d1.flags & DESC_F_WRITE, DESC_F_WRITE);
        assert_eq!(n1, 2);
        assert_eq!(d2.flags & DESC_F_NEXT, 0);
        assert_eq!(mem.avail, vec![0]);
    }

    #[test]
    fn test_free_list_reuse_after_complete() {
        let mut mem = MockMem::new(8);
        let mut ring: Ring<8> = Ring::new();
        let h0 = ring.submit(&mut mem, &three_chain(1)).expect("s0");
        assert_eq!(ring.free_count(), 5);
        mem.device_complete_next();
        assert_eq!(ring.complete(&mem).expect("c0"), Some(h0));
        assert_eq!(ring.free_count(), 8);
        // The freed descriptors are reused (LIFO order is fine — ids only).
        let h1 = ring.submit(&mut mem, &three_chain(2)).expect("s1");
        assert!(h1 < 8);
    }

    #[test]
    fn test_in_flight_backpressure() {
        let mut mem = MockMem::new(8);
        let mut ring: Ring<8> = Ring::new();
        // 8 descriptors → two 3-desc chains fit, the third must be refused
        // WITHOUT leaking reservations.
        ring.submit(&mut mem, &three_chain(1)).expect("s0");
        ring.submit(&mut mem, &three_chain(2)).expect("s1");
        assert_eq!(ring.free_count(), 2);
        assert_eq!(ring.submit(&mut mem, &three_chain(3)), Err(RingError::Exhausted));
        assert_eq!(ring.free_count(), 2, "partial reservation must roll back");
        // After one completion the third chain fits again.
        mem.device_complete_next();
        ring.complete(&mem).expect("c0").expect("head");
        ring.submit(&mut mem, &three_chain(3)).expect("s2");
    }

    #[test]
    fn test_used_ring_wraparound_and_long_sequential_regression() {
        // The TASK-0293 hazard: long sequential IO. Drive far past the u16
        // index wrap with QD1 — every id must reclaim cleanly.
        let mut mem = MockMem::new(8);
        let mut ring: Ring<8> = Ring::new();
        for i in 0..70_000u32 {
            let head = ring.submit(&mut mem, &three_chain(i as u64)).expect("submit");
            mem.device_complete_next();
            let done = ring.complete(&mem).expect("complete").expect("one");
            assert_eq!(done, head);
        }
        assert_eq!(ring.free_count(), 8);
    }

    #[test]
    fn test_out_of_order_completion() {
        let mut mem = MockMem::new(16);
        let mut ring: Ring<16> = Ring::new();
        let h0 = ring.submit(&mut mem, &three_chain(1)).expect("s0");
        let h1 = ring.submit(&mut mem, &three_chain(2)).expect("s1");
        // Device finishes the SECOND chain first.
        mem.device_complete_head(h1);
        mem.device_complete_head(h0);
        assert_eq!(ring.complete(&mem).expect("c"), Some(h1));
        assert_eq!(ring.complete(&mem).expect("c"), Some(h0));
        assert_eq!(ring.complete(&mem).expect("c"), None);
        assert_eq!(ring.free_count(), 16);
    }

    #[test]
    fn test_reject_bad_used_ids() {
        let mut mem = MockMem::new(8);
        let mut ring: Ring<8> = Ring::new();
        ring.submit(&mut mem, &three_chain(1)).expect("s0");
        // Device claims an out-of-range id.
        mem.device_complete_head(9);
        assert_eq!(ring.complete(&mem), Err(RingError::BadUsedId));
        // Device claims an id that is not a chain head in flight.
        let mut mem2 = MockMem::new(8);
        let mut ring2: Ring<8> = Ring::new();
        ring2.submit(&mut mem2, &three_chain(1)).expect("s0");
        mem2.device_complete_head(5); // free descriptor, never submitted
        assert_eq!(ring2.complete(&mem2), Err(RingError::BadUsedId));
    }
}
