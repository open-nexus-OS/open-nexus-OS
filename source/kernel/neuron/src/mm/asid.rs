// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the ASID allocator of `AddressSpaceManager`, split out of `address_space.rs`
//! (module-size ratchet, TASK-0333 6c-1a): a bitmap over `MAX_ASIDS`, ASID 0 reserved for the
//! kernel, freed ASIDs quarantined until a global TLB epoch flush releases them.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `address_space` tests (`allocate_unique_asids`) + QEMU selftests
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md

use crate::types::Asid;

/// Maximum ASIDs made available by the allocator.
pub(super) const MAX_ASIDS: usize = 256;
const WORD_BITS: usize = core::mem::size_of::<u64>() * 8;
const BITMAP_WORDS: usize = MAX_ASIDS.div_ceil(WORD_BITS);

pub(super) struct AsidAllocator {
    bitmap: [u64; BITMAP_WORDS],
    next: usize,
    /// SMP A2c/A5 quarantine: freed ASIDs stay parked (allocation bit kept
    /// set) until a global TLB epoch flush releases them — another hart may
    /// still hold stale TLB entries tagged with a freed ASID. Until the A5
    /// shootdown lands, quarantined ASIDs leak boundedly (MAX_ASIDS = 256,
    /// a boot cycles well under 50).
    quarantined: [u64; BITMAP_WORDS],
}

impl AsidAllocator {
    pub(super) const fn new() -> Self {
        let mut bitmap = [0u64; BITMAP_WORDS];
        // Reserve ASID 0 for the kernel/global mappings.
        bitmap[0] |= 1;
        Self { bitmap, next: 1, quarantined: [0u64; BITMAP_WORDS] }
    }

    pub(super) fn allocate(&mut self) -> Option<Asid> {
        if let Some(asid) = self.try_allocate() {
            return Some(asid);
        }
        // Pool exhausted: recycle the quarantine behind a GLOBAL epoch
        // shootdown (A5) — every online hart flushes and acks before a
        // recycled ASID can be reused anywhere.
        crate::smp::tlb::shootdown_all();
        self.release_quarantined();
        self.try_allocate()
    }

    fn try_allocate(&mut self) -> Option<Asid> {
        for _ in 0..MAX_ASIDS {
            let index = self.next % MAX_ASIDS;
            let word = index / WORD_BITS;
            let bit = index % WORD_BITS;
            if self.bitmap[word] & (1 << bit) == 0 {
                self.bitmap[word] |= 1 << bit;
                self.next = (index + 1) % MAX_ASIDS;
                return Some(Asid::from_raw(index as u16));
            }
            self.next = (index + 1) % MAX_ASIDS;
        }
        None
    }

    pub(super) fn free(&mut self, asid: Asid) {
        let index = asid.as_raw() as usize;
        if index < MAX_ASIDS {
            let word = index / WORD_BITS;
            let bit = index % WORD_BITS;
            // Park instead of clearing: reuse only after a TLB flush covering
            // all harts that may hold stale entries (see `release_quarantined`;
            // recycled on exhaustion in `allocate`, replaced by the A5 epoch
            // shootdown).
            self.quarantined[word] |= 1 << bit;
        }
    }

    /// Releases all quarantined ASIDs for reuse. Only call after a TLB
    /// invalidation covering every hart that may hold stale entries (interim:
    /// local full flush + boot-hart-only user execution; A5: global epoch).
    fn release_quarantined(&mut self) {
        for word in 0..BITMAP_WORDS {
            self.bitmap[word] &= !self.quarantined[word];
            self.quarantined[word] = 0;
        }
        // Keep ASID 0 permanently reserved for the kernel.
        self.bitmap[0] |= 1;
    }
}
