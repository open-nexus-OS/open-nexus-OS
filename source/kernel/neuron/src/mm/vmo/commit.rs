// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a lazy object's page gets its frame (ADR-0077; split out of `vmo.rs` under the
//! structure ratchet, TASK-0330 P3). `commit` is the whole answer — a committed page's frame, an
//! absent page zeroed and entered, a squeezed page inflated, a swapped one refused with what to
//! wait for. `commit_fresh` is the part a fault runs with no lock held at all: an absent page
//! becomes a zeroed frame, anything else is left to the locked path. The frame is taken and
//! zeroed outside the table lock in both; the entry under it decides between two harts that
//! commit the same page — the second frees its frame and uses the first's.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! TEST_COVERAGE: every boot (every `.bss` tail and every stack is lazy), `SELFTEST: vm commit
//!   on touch ok (…)`, the squeeze and swap proofs; two harts on one page: the rank-check lanes
//! INVARIANTS: a frame is zeroed (and cleaned for a non-coherent device) before it is entered;
//!   a page never takes a second frame; the table lock is never held across the memset
//! ADR: docs/adr/0077-memory-is-committed-on-first-touch-the-kernel-touches-user-memory-only-through-a-checked-range.md

use super::{clean, frame_pool, squeeze, swap, with, VmoError, VmoId, Within, TABLE};
use crate::lazy::{CommitError, PageState, PAGE};

/// Page `index` of `id`, committed now if it was not: a frame from the pool, zeroed before it
/// is entered — outside the table lock, which is a leaf and never held across a memset. A
/// squeezed page is brought back instead (ADR-0087, `squeeze::inflate`); a swapped one answers
/// `Swapped` — only the pager brings it back (ADR-0088). A page whose word moved on between
/// the look and the act (another hart committed it) is looked at once more. Answers the page's
/// physical address; an eager object answers its translation.
pub fn commit(id: VmoId, index: usize) -> Result<usize, VmoError> {
    for _ in 0..2 {
        let (state, within, clean) = with(id, |o| match &o.lazy {
            Some(pages) => pages
                .state(index)
                .map(|s| (s, o.within, usize::from(o.clean_block)))
                .ok_or(VmoError::BadLength),
            None => o
                .translate(index * PAGE)
                .map(|pa| (PageState::Frame(pa as u64), None, 0))
                .ok_or(VmoError::BadLength),
        })
        .ok_or(VmoError::NoSuchObject)??;
        let done = match state {
            PageState::Frame(pa) => Some(pa as usize),
            PageState::Record(_) => squeeze::inflate(id, index, within)?,
            PageState::Absent => commit_zeroed(id, index, within, clean)?,
            PageState::Swapped => return Err(swap::parked_on(id, index)),
        };
        if let Some(pa) = done {
            return Ok(pa);
        }
    }
    // A page that moved on twice between a look and its act: the table is not what it says.
    Err(VmoError::Damaged)
}

/// Page `index` of `id` has a frame, or gets a zeroed one now — for a caller that holds no
/// lock at all (`populate::fill_free`). `false`: the page is a record or swapped, the object
/// is gone, or no frame was to be had — all of them the locked path's to act on.
pub fn commit_fresh(id: VmoId, index: usize) -> bool {
    let looked = with(id, |o| {
        let pages = o.lazy.as_ref()?;
        Some((pages.state(index)?, o.within, usize::from(o.clean_block)))
    });
    match looked.flatten() {
        Some((PageState::Frame(_), ..)) => true,
        Some((PageState::Absent, within, clean)) => {
            matches!(commit_zeroed(id, index, within, clean), Ok(Some(_)))
        }
        _ => false,
    }
}

/// Commit absent page `index` to a zeroed frame; `None` when it turned out to be a record. For
/// a non-coherent device (`clean`: its cache block) the zeroes reach memory before the frame is
/// entered — a device read never sees the frame's previous owner (ADR-0092).
fn commit_zeroed(
    id: VmoId,
    index: usize,
    within: Option<Within>,
    clean: usize,
) -> Result<Option<usize>, VmoError> {
    let block = match within {
        None => frame_pool::alloc(0),
        Some(within) => frame_pool::alloc_within(0, within.ranges()),
    }
    .map_err(|_| VmoError::Exhausted)?;
    // SAFETY: a frame just taken from the pool, reachable by nobody else until it is entered.
    unsafe {
        core::ptr::write_bytes(crate::phys::phys_to_virt(block.base as usize) as *mut u8, 0, PAGE);
    }
    clean::frame(block.base as usize, PAGE, clean);
    let entered = match TABLE.lock().get_mut(id).and_then(|o| o.lazy.as_mut()) {
        Some(pages) => pages.commit(index, block.base).map_err(Some),
        None => Err(None),
    };
    if entered.is_err() {
        frame_pool::free(block);
    }
    match entered {
        Ok(()) => Ok(Some(block.base as usize)),
        Err(Some(CommitError::Committed(pa))) => Ok(Some(pa as usize)),
        // Squeezed (or swapped) between the look and the commit: the caller looks again.
        Err(Some(CommitError::Squeezed | CommitError::Swapped)) => Ok(None),
        Err(Some(CommitError::OutOfRange)) => Err(VmoError::BadLength),
        // The page's chunk of the map found no heap above the reserve (ADR-0093, ADR-0091).
        Err(Some(CommitError::NoHeap)) => Err(VmoError::HeapExhausted),
        // The object died between the look and the commit.
        Err(None) => Err(VmoError::NoSuchObject),
    }
}
