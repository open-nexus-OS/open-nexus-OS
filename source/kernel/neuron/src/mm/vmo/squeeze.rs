// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a lazy object's pages squeezed and brought back (RFC-0100 §5, TASK-0333 6f-1,
//! ADR-0087) — the object side of the squeeze. [`squeeze_page`] takes one committed page of a
//! parked child's private object: a page of zeroes is uncommitted, a page whose block fits a
//! record is recorded in the object's store (`crate::lazy::store`), any other page is kept; the
//! frame goes back to the caller, who returns it to the pool once no TLB can reach it
//! (`mm::squeeze`). [`inflate`] is the way back, from `vmo::commit` — so every path that commits
//! a page brings a squeezed one back: a fresh frame inside the object's reach, the record copied
//! out under the table lock, decoded outside it, checked to be exactly one page, entered. A child
//! module of `vmo`: it reaches the table directly and keeps `vmo.rs` under the ratchet.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional (TASK-0333 6f-1)
//! API_STABILITY: Internal (the order is `SYSCALL_TASK_SQUEEZE`, 69)
//! TEST_COVERAGE: the page table and the store on the host (`crate::lazy`); `KSELFTEST: task
//!   squeeze ok (…)` on every lane and the board; `SELFTEST: exec child squeeze ok (…)`
//! INVARIANTS: the codec never runs under the table lock (a record's bytes are copied in and
//!   out under it); frames are taken before the table lock and returned after it; the lock order
//!   is the scratch, then the table; a record that does not decode to one page is never entered.

use core::sync::atomic::{AtomicU64, Ordering};

use super::{frame_pool, with, Block, TableLock, VmoError, VmoId, VmoKind, Within, TABLE};
use crate::lazy::store::{PageMemory, Spare, HEADER, MAX_BLOCK};
use crate::lazy::{SqueezeError, PAGE};
use crate::vmo_lifetime::SqueezeFacts;

/// Pages brought back since boot (`MmStats::inflated_pages`).
static INFLATED: AtomicU64 = AtomicU64::new(0);
/// Records that did not decode — named on the console once each, up to a few.
static DAMAGED: AtomicU64 = AtomicU64::new(0);

/// The direct map as the store's memory: every frame is reachable at its kernel address.
struct DirectMap;

impl PageMemory for DirectMap {
    fn read(&self, pa: u64, at: usize, out: &mut [u8]) {
        debug_assert!(at + out.len() <= PAGE);
        // SAFETY: `pa` is a frame of the store (its occupancy word says a record lives there) and
        // the range lies inside it; the caller holds the table lock that owns the store.
        unsafe {
            core::ptr::copy_nonoverlapping(
                crate::phys::phys_to_virt(pa as usize + at) as *const u8,
                out.as_mut_ptr(),
                out.len(),
            );
        }
    }

    fn write(&mut self, pa: u64, at: usize, bytes: &[u8]) {
        debug_assert!(at + bytes.len() <= PAGE);
        // SAFETY: as `read` — a frame the store owns (just taken from `Spare` or holding a free
        // slot of this record), written inside its bounds under the table lock.
        unsafe {
            core::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                crate::phys::phys_to_virt(pa as usize + at) as *mut u8,
                bytes.len(),
            );
        }
    }
}

/// The codec's working memory: the encoder's hash table and one block. Under the kernel lock one
/// squeeze or inflate runs at a time; the lock makes that a fact, not an assumption.
struct Scratch {
    hash: nexus_compress::Table,
    block: [u8; MAX_BLOCK],
}

static SCRATCH: TableLock<Scratch> =
    TableLock::new(Scratch { hash: nexus_compress::Table::new(), block: [0; MAX_BLOCK] });

/// The facts the squeeze rule weighs of the live object `id`, with the ones its child's address
/// space and the task table answer (`None`: no such object).
pub fn facts(
    id: VmoId,
    regions_here: usize,
    mapped_elsewhere: bool,
    named_elsewhere: bool,
) -> Option<SqueezeFacts> {
    with(id, |o| SqueezeFacts {
        lazy: o.kind == VmoKind::Lazy,
        sealed: o.sealed,
        device_named: o.device_named,
        unlocked: o.purgeable,
        regions_here,
        mapped_elsewhere,
        named_elsewhere,
    })
}

/// What one page came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageOutcome {
    /// No frame to take (absent, or a record already).
    Untouched,
    /// A page of zeroes, uncommitted: its frame for the caller.
    Zero(u64),
    /// Recorded in `bytes` (length field and block): its frame for the caller.
    Squeezed { frame: u64, bytes: usize },
    /// Kept as it is: its block was over a record, or the store had no frame for it.
    Kept,
}

/// Squeeze page `index` of `id` — the caller holds the kernel lock and the object is a parked
/// child's alone (`vmo_lifetime::squeeze_allowed`). A store page the record needs takes one of
/// `spare`'s frames.
pub fn squeeze_page(id: VmoId, index: usize, spare: &mut Spare) -> PageOutcome {
    let Some(pa) = with(id, |o| o.lazy.as_ref().and_then(|p| p.frame(index))).flatten() else {
        return PageOutcome::Untouched;
    };
    // SAFETY: the page's frame; the object is a parked child's alone, so nothing writes it while
    // the kernel lock is held; it is read here, never freed (the caller frees it after the
    // shootdown).
    let page = unsafe {
        core::slice::from_raw_parts(crate::phys::phys_to_virt(pa as usize) as *const u8, PAGE)
    };
    if page.iter().all(|&b| b == 0) {
        let mut table = TABLE.lock();
        return match table.get_mut(id).and_then(|o| o.lazy.as_mut()).map(|p| p.uncommit(index)) {
            Some(Ok(frame)) => PageOutcome::Zero(frame),
            _ => PageOutcome::Kept,
        };
    }
    let mut scratch = SCRATCH.lock();
    let Scratch { hash, block } = &mut *scratch;
    let Ok(n) = nexus_compress::compress(page, &mut block[..], hash) else {
        return PageOutcome::Kept;
    };
    let mut table = TABLE.lock();
    let recorded = table
        .get_mut(id)
        .and_then(|o| o.lazy.as_mut())
        .map(|p| p.squeeze(index, &block[..n], spare, &mut DirectMap));
    match recorded {
        Some(Ok(frame)) => PageOutcome::Squeezed { frame, bytes: HEADER + n },
        _ => PageOutcome::Kept,
    }
}

/// Bring page `index` of `id` back (ADR-0087) — `vmo::commit` for a page whose word names a
/// record. A fresh frame inside `within` (any frame when `None`), the record copied out under the
/// table lock, decoded outside it, the decoded length checked to be one page, the word switched,
/// the record released. Answers the page's frame; `None` when the page's word moved on between
/// the look and the act (the caller looks again).
pub(super) fn inflate(
    id: VmoId,
    index: usize,
    within: Option<Within>,
) -> Result<Option<usize>, VmoError> {
    let frame = match within {
        None => frame_pool::alloc(0),
        Some(within) => frame_pool::alloc_within(0, within.ranges()),
    }
    .map_err(|_| VmoError::Exhausted)?;
    let mut scratch = SCRATCH.lock();
    let copied = TABLE
        .lock()
        .get(id)
        .and_then(|o| o.lazy.as_ref())
        .map(|p| p.record(index, &mut scratch.block, &DirectMap));
    let (slot, len) = match copied {
        Some(Ok(record)) => record,
        Some(Err(SqueezeError::NotSqueezed)) => {
            drop(scratch);
            frame_pool::free(frame);
            return Ok(None);
        }
        Some(Err(_)) => {
            drop(scratch);
            frame_pool::free(frame);
            return Err(damaged(id, index));
        }
        None => {
            drop(scratch);
            frame_pool::free(frame);
            return Err(VmoError::NoSuchObject);
        }
    };
    // SAFETY: a frame just taken from the pool, reachable by nobody until it is entered.
    let page = unsafe {
        core::slice::from_raw_parts_mut(
            crate::phys::phys_to_virt(frame.base as usize) as *mut u8,
            PAGE,
        )
    };
    if nexus_compress::decompress(&scratch.block[..len], page) != Ok(PAGE) {
        drop(scratch);
        frame_pool::free(frame);
        return Err(damaged(id, index));
    }
    let entered = TABLE
        .lock()
        .get_mut(id)
        .and_then(|o| o.lazy.as_mut())
        .map(|p| p.inflated(index, slot, frame.base, &DirectMap));
    drop(scratch);
    match entered {
        Some(Ok(released)) => {
            for pa in released.frames() {
                frame_pool::free(Block { base: pa, order: 0 });
            }
            INFLATED.fetch_add(1, Ordering::Relaxed);
            Ok(Some(frame.base as usize))
        }
        Some(Err(SqueezeError::Changed)) => {
            frame_pool::free(frame);
            Ok(None)
        }
        Some(Err(_)) => {
            frame_pool::free(frame);
            Err(damaged(id, index))
        }
        None => {
            frame_pool::free(frame);
            Err(VmoError::NoSuchObject)
        }
    }
}

/// A record that did not decode to one page: named (the first few), the access refused.
fn damaged(id: VmoId, index: usize) -> VmoError {
    if DAMAGED.fetch_add(1, Ordering::Relaxed) < 4 {
        log_error!(target: "mm", "MM: FAIL inflate (vmo={} page={}) record damaged", id, index);
    }
    VmoError::Damaged
}

/// Pages brought back since boot.
pub fn inflated() -> u64 {
    INFLATED.load(Ordering::Relaxed)
}

/// `(pages held as records, bytes the records occupy, frames the stores hold)` over every live
/// object (`MmStats` v5).
pub fn stats() -> (u64, u64, u64) {
    let table = TABLE.lock();
    table.objects.iter().flatten().filter_map(|o| o.lazy.as_ref()).fold(
        (0, 0, 0),
        |(pages, bytes, frames), p| {
            let (_, b, f) = p.store_stats();
            (pages + p.squeezed() as u64, bytes + b as u64, frames + f as u64)
        },
    )
}
