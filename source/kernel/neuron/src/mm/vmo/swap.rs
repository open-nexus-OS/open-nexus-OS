// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a lazy object's records leaving for the pager and coming back (RFC-0100 §5,
//! TASK-0333 6f-2, ADR-0088) — the object side of the swap stage. [`export`] copies one record's
//! block out under the table lock and marks the page exported under the seal's counter (the
//! caller seals it with the lock dropped); [`commit`] makes the page swapped once the pager wrote
//! it, releasing the record; [`entry`] is what a page-in looks at, and [`page_in`] decodes the
//! opened record into a fresh frame inside the object's reach and enters it — only if the page is
//! still swapped exactly as the look saw it. A commit of a swapped page is never a zeroed frame:
//! [`parked_on`] answers the token a fault or a syscall parks on. A child module of `vmo`: it
//! reaches the table directly and keeps `vmo.rs` under the ratchet.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional (TASK-0333 6f-2)
//! API_STABILITY: Internal (the syscalls are `SYSCALL_SWAP_EXPORT` 71, `_COMMIT` 72, `_PAGE_IN` 73)
//! TEST_COVERAGE: the page table's swap steps on the host (`crate::lazy`); `KSELFTEST: swap ok
//!   (…)` on every lane and the board
//! INVARIANTS: the codec never runs under the table lock; frames are taken before the table lock
//!   and returned after it; a page is entered only while its swap entry is the one its record
//!   was opened under; a record that does not decode to one page is never entered.

use core::sync::atomic::{AtomicU64, Ordering};

use super::squeeze::DirectMap;
use super::{frame_pool, with, Block, VmoError, VmoId, Within, TABLE};
use crate::lazy::swap::{token, SwapEntry};
use crate::lazy::{PageState, SwapStateError, PAGE};

/// Swapped pages brought back since boot (`MmStats::page_ins`).
static PAGE_INS: AtomicU64 = AtomicU64::new(0);

/// The error a commit of swapped page `index` of `id` answers: the token to park on, and the
/// pager's slot (a swapped page without one is not what the table says: `Damaged`).
pub(super) fn parked_on(id: VmoId, index: usize) -> VmoError {
    let slot = with(id, |o| o.lazy.as_ref().and_then(|p| p.swap_entry(index))?.slot).flatten();
    match (slot, u32::try_from(index)) {
        (Some(slot), Ok(index)) => VmoError::Swapped { token: token(id, index), slot },
        _ => VmoError::Damaged,
    }
}

/// Export page `index` of `id` under `ctr`: its record's block copied into `out` (at least
/// `store::MAX_BLOCK` bytes), the page marked exported. Answers the block's length; `None` when
/// the page holds no record, or one already swapped.
pub fn export(id: VmoId, index: usize, ctr: u64, out: &mut [u8]) -> Option<usize> {
    let mut table = TABLE.lock();
    let pages = table.get_mut(id)?.lazy.as_mut()?;
    let (_, len) = pages.record(index, out, &DirectMap).ok()?;
    pages.export(index, ctr).ok()?;
    Some(len)
}

/// The pager wrote page `index`'s record, sealed under `ctr`, to `slot`: the page is swapped,
/// its record released, the store frames that emptied back in the pool.
pub fn commit(id: VmoId, index: usize, ctr: u64, slot: u32) -> Result<(), SwapStateError> {
    let released = TABLE
        .lock()
        .get_mut(id)
        .and_then(|o| o.lazy.as_mut())
        .ok_or(SwapStateError::NotARecord)?
        .swap_commit(index, ctr, slot, &DirectMap)?;
    for pa in released.frames() {
        frame_pool::free(Block { base: pa, order: 0 });
    }
    Ok(())
}

/// What a page-in looks at: page `index` of `id`'s swap entry, while the page is swapped.
pub fn entry(id: VmoId, index: usize) -> Option<SwapEntry> {
    with(id, |o| {
        let pages = o.lazy.as_ref()?;
        (pages.state(index)? == PageState::Swapped).then(|| pages.swap_entry(index))?
    })
    .flatten()
}

/// Why a page-in did not enter the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageInError {
    /// The page is no longer swapped as the look saw it (or its object is gone).
    Stale,
    /// The block did not decode to one page.
    Damaged,
    /// No frame for the page.
    Exhausted,
}

/// Bring swapped page `index` of `id` back from `block` — the record `entry` names, opened by
/// the caller: a fresh frame inside the object's reach, the block decoded into it and checked to
/// be one page, entered while the page is still swapped as `entry` says.
pub fn page_in(id: VmoId, index: usize, entry: SwapEntry, block: &[u8]) -> Result<(), PageInError> {
    let within: Option<Within> = with(id, |o| o.within).ok_or(PageInError::Stale)?;
    let frame = match within {
        None => frame_pool::alloc(0),
        Some(within) => frame_pool::alloc_within(0, within.ranges()),
    }
    .map_err(|_| PageInError::Exhausted)?;
    // SAFETY: a frame just taken from the pool, reachable by nobody until it is entered.
    let page = unsafe {
        core::slice::from_raw_parts_mut(
            crate::phys::phys_to_virt(frame.base as usize) as *mut u8,
            PAGE,
        )
    };
    if nexus_compress::decompress(block, page) != Ok(PAGE) {
        frame_pool::free(frame);
        return Err(PageInError::Damaged);
    }
    let entered = TABLE
        .lock()
        .get_mut(id)
        .and_then(|o| o.lazy.as_mut())
        .map(|p| p.swap_in(index, entry, frame.base));
    match entered {
        Some(Ok(())) => {
            PAGE_INS.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        _ => {
            frame_pool::free(frame);
            Err(PageInError::Stale)
        }
    }
}

/// Swapped pages brought back since boot.
pub fn page_ins() -> u64 {
    PAGE_INS.load(Ordering::Relaxed)
}

/// Pages swapped now, over every live object (`MmStats` v6).
pub fn swapped() -> u64 {
    let table = TABLE.lock();
    table.objects.iter().flatten().filter_map(|o| o.lazy.as_ref()).map(|p| p.swapped() as u64).sum()
}
