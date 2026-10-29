// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the pager's books (RFC-0100 §5, ADR-0088, TASK-0333 6f-2) — which slots of the
//! `swap` partition hold whose sealed record, and a slot's bytes. One slot is 4 KiB (eight
//! sectors) and holds one sealed record: its length, then the bytes the kernel sealed — memd
//! never sees anything but ciphertext, and a record opens in the kernel alone, as the page and
//! the export it was sealed for. A slot is taken at the export, kept when the commit holds,
//! freed when the page came back, when the commit was refused (a touch or a later export
//! superseded it), when the kernel says its object died, or when a page-in finds it stale; a
//! slot whose record did not open stays taken until its object dies (every later touch of that
//! page meets the same bytes and dies — the page cannot come back). Pure: the OS loop owns the
//! disk and the syscalls.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal (the slot layout is the pager's own; it dies with the boot's key)
//! TEST_COVERAGE: tests/contract.rs (`slots_*`, `test_reject_*` on the slot layout)

use alloc::vec::Vec;

use nexus_abi::SWAP_MAX_SEALED;

/// Bytes of one slot.
pub const SLOT_BYTES: usize = 4096;
/// Sectors of one slot (the block protocol's 512-byte sectors).
pub const SECTORS_PER_SLOT: u64 = 8;
/// A slot's head: the sealed record's length (`u16le`).
pub const SLOT_HEAD: usize = 2;

/// One taken slot: whose page it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotUse {
    /// The slot.
    pub slot: u32,
    /// The process the record was exported from (prefetched when it thaws).
    pub pid: u32,
    /// The page's swap token (commit and page-in name it).
    pub token: u64,
}

/// The slots of the partition: a bitmap of the taken ones and their owners.
#[derive(Clone, Debug, Default)]
pub struct Slots {
    taken: Vec<u64>,
    owners: Vec<SlotUse>,
    capacity: u32,
}

impl Slots {
    /// `capacity` slots, none taken.
    #[must_use]
    pub fn new(capacity: u32) -> Self {
        let words = (capacity as usize).div_ceil(64);
        Self { taken: alloc::vec![0; words], owners: Vec::new(), capacity }
    }

    /// Slots the partition holds.
    #[must_use]
    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Slots taken.
    #[must_use]
    pub fn used(&self) -> usize {
        self.owners.len()
    }

    /// The lowest free slot, taken for `pid`'s page `token`; `None` when the partition is full.
    pub fn take(&mut self, pid: u32, token: u64) -> Option<u32> {
        let (w, word) = self.taken.iter().enumerate().find(|(_, w)| **w != u64::MAX)?;
        let slot = (w * 64) as u32 + (!word).trailing_zeros();
        if slot >= self.capacity {
            return None;
        }
        self.taken[w] |= 1 << (slot % 64);
        self.owners.push(SlotUse { slot, pid, token });
        Some(slot)
    }

    /// Frees `slot`; answers whose it was (`None`: it was free).
    pub fn release(&mut self, slot: u32) -> Option<SlotUse> {
        let at = self.owners.iter().position(|u| u.slot == slot)?;
        self.taken[slot as usize / 64] &= !(1 << (slot % 64));
        Some(self.owners.swap_remove(at))
    }

    /// Whose page `slot` holds.
    #[must_use]
    pub fn owner(&self, slot: u32) -> Option<SlotUse> {
        self.owners.iter().find(|u| u.slot == slot).copied()
    }

    /// The slots `pid`'s pages are in (a thaw pages them back), lowest first.
    #[must_use]
    pub fn held_by(&self, pid: u32) -> Vec<SlotUse> {
        let mut held: Vec<SlotUse> = self.owners.iter().filter(|u| u.pid == pid).copied().collect();
        held.sort_unstable_by_key(|u| u.slot);
        held
    }
}

/// A slot's bytes for the sealed record `sealed`: its length, the bytes, zeroes after. `None`
/// for a record longer than the seal's bound.
pub fn encode_slot(sealed: &[u8], out: &mut [u8; SLOT_BYTES]) -> Option<()> {
    if sealed.len() > SWAP_MAX_SEALED {
        return None;
    }
    let len = u16::try_from(sealed.len()).ok()?;
    out[..SLOT_HEAD].copy_from_slice(&len.to_le_bytes());
    out[SLOT_HEAD..SLOT_HEAD + sealed.len()].copy_from_slice(sealed);
    out[SLOT_HEAD + sealed.len()..].fill(0);
    Some(())
}

/// The sealed record a slot holds; `None` when its length is past the seal's bound (the slot
/// was never written, or the disk changed it — the kernel would refuse it anyway).
#[must_use]
pub fn decode_slot(slot: &[u8]) -> Option<&[u8]> {
    let len = usize::from(u16::from_le_bytes(slot.get(..SLOT_HEAD)?.try_into().ok()?));
    if len == 0 || len > SWAP_MAX_SEALED {
        return None;
    }
    slot.get(SLOT_HEAD..SLOT_HEAD + len)
}
