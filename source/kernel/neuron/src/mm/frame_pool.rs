// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kernel's frame pool (TASK-0286 P2b) — the ONE `FrameAllocator`
//! (`crate::frames`, host-proven) over the banks the tree names, built once by
//! the boot hart after the heap exists and before the first page table.
//! Carved out: the tree's reserved ranges, the kernel image and the tree
//! itself — nothing else; the last fixed windows died with P3b. Page tables
//! (P2b), VMOs, process images (P3a), user stacks and init's pages (P3b)
//! allocate here; a device's DMA memory comes from inside its reach
//! (`alloc_within`, TASK-0246 P1). Exhaustion is an event on the console AND a
//! counter — never a silent `None`.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the allocator is host-tested in `crate::frames`; this wrapper
//!   is proven by every boot (`KINIT: mm frames (…)`) and every page table
//! INVARIANTS: one pool, one lock (leaf: never held across a log line); a
//!   refused free is reported, never swallowed; a hart's magazine (`frames::magazine`) holds
//!   only frames the pool handed out, its lock and the pool's are never held together, and
//!   nobody is refused while a magazine holds a frame.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::frames::magazine::{Magazine, BATCH};
use crate::frames::{Block, FrameAllocator, FrameError, Range, Stats};
use crate::sync::hart_table::{HartTable, SizeError};

#[cfg(debug_assertions)]
type PoolLock<T> = crate::sync::dbg_mutex::DbgMutex<T>;
#[cfg(not(debug_assertions))]
type PoolLock<T> = spin::Mutex<T>;

static POOL: PoolLock<Option<FrameAllocator>> = PoolLock::new(None);

/// Why the pool could not be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitError {
    /// No tree was recorded, or it does not parse.
    NoTree,
    /// The allocator refused the tree (too many banks/holes, a range too large).
    Frames(FrameError),
}

/// Build the pool from the tree. Boot hart, once, heap up, paging on.
pub fn init_from_tree() -> Result<Stats, InitError> {
    let bytes = crate::boot_fdt::bytes().ok_or(InitError::NoTree)?;
    let fdt = nexus_fdt::Fdt::new(bytes).map_err(|_| InitError::NoTree)?;
    let (image_va, image_end_va) = crate::boot_image::range();
    let image_pa = crate::phys::virt_to_phys(image_va);
    let image = Range { base: image_pa as u64, size: (image_end_va - image_va) as u64 };
    let tree =
        crate::boot_fdt::range().map(|(s, e)| Range { base: s as u64, size: (e - s) as u64 });
    // ADR-0081: the bootfs the loader placed past the kernel — its pages are the files'
    // frames for the whole boot, never the pool's.
    let bootfs =
        crate::bootfs::range().map(|(s, e)| Range { base: s as u64, size: (e - s) as u64 });
    let mut excluded = [Range { base: 0, size: 0 }; 3];
    let mut n = 0;
    for range in [Some(image), tree, bootfs].into_iter().flatten() {
        excluded[n] = range;
        n += 1;
    }
    let pool = FrameAllocator::init_from_fdt(&fdt, &excluded[..n]).map_err(InitError::Frames)?;
    let stats = pool.stats();
    // ADR-0085: the watermarks are fractions of what each bank has free at this moment.
    super::pressure_endpoint::init(
        pool.banks().iter().map(|bank| bank.free_frames() as u64),
        stats.free as u64,
    );
    *POOL.lock() = Some(pool);
    Ok(stats)
}

/// A block of `1 << order` frames, or the error. A single frame comes from this hart's
/// magazine (TASK-0330 P3): the pool is asked once in a batch.
pub fn alloc(order: u8) -> Result<Block, FrameError> {
    if order == 0 {
        if let Some(base) = from_magazine() {
            return Ok(Block { base, order: 0 });
        }
    }
    locked(order, |pool| pool.alloc(order))
}

/// One magazine per hart, each behind its own lock: its hart takes from it, and a pool that
/// ran dry takes back what the others hold. Never held together with the pool's lock.
static MAGAZINES: HartTable<PoolLock<Magazine>> = HartTable::new(PoolLock::new(Magazine::new()));
/// Frames a dry pool took back from the magazines.
static FLUSHED: AtomicU64 = AtomicU64::new(0);

/// Sizes the magazines for the machine (the boot hart, before a secondary starts).
pub fn size(harts: usize) -> Result<(), SizeError> {
    MAGAZINES.size(harts, || PoolLock::new(Magazine::new()))
}

/// A frame from this hart's magazine, refilled from the pool when it is empty. `None`: the
/// pool has none to spare — the caller asks it directly, which is where a refusal is counted.
fn from_magazine() -> Option<u64> {
    // A hart before its local block is installed has no magazine yet (the lookup that guesses
    // is the selftest's tripwire, never used here).
    let mine = MAGAZINES.get(crate::smp::cpu_from_hart_local_tp()?.as_index())?;
    if let Some(base) = mine.lock().take() {
        return Some(base);
    }
    let (mut batch, mut taken) = ([0u64; BATCH], 0);
    let free = {
        let mut pool = POOL.lock();
        let pool = pool.as_mut()?;
        // A single frame exists exactly while a frame is free: no ask here is refused.
        while taken < BATCH && pool.free_frames() != 0 {
            let Ok(block) = pool.alloc(0) else { break };
            batch[taken] = block.base;
            taken += 1;
        }
        pool.free_frames()
    };
    super::pressure_endpoint::observe(free as u64);
    let first = *batch[..taken].first()?;
    let kept = mine.lock().refill(&batch[1..taken]);
    // Not kept (another refill of this magazine came first): back to the pool.
    for &base in &batch[1 + kept..taken] {
        free_block(Block { base, order: 0 });
    }
    Some(first)
}

/// Every magazine's frames back into the pool; `true` when there were any.
fn flush_magazines() -> bool {
    let mut any = false;
    for magazine in MAGAZINES.iter() {
        let mut batch = [0u64; BATCH];
        let held = magazine.lock().flush(&mut batch);
        for &base in &batch[..held] {
            free_block(Block { base, order: 0 });
        }
        FLUSHED.fetch_add(held as u64, Ordering::Relaxed);
        any |= held != 0;
    }
    any
}

/// `(harts, frames held, frames handed out, refills, frames a dry pool took back)`.
pub fn magazine_stats() -> (usize, usize, u64, u64, u64) {
    let (mut held, mut served, mut refills) = (0, 0, 0);
    for magazine in MAGAZINES.iter() {
        let magazine = magazine.lock();
        let counts = magazine.counts();
        (held, served, refills) = (held + magazine.held(), served + counts.0, refills + counts.1);
    }
    (MAGAZINES.len(), held, served, refills, FLUSHED.load(Ordering::Relaxed))
}

/// A block of `1 << order` frames inside the physical `windows` (`(start, end)`,
/// a device's DMA reach by CPU address), or the error.
pub fn alloc_within(order: u8, windows: &[(u64, u64)]) -> Result<Block, FrameError> {
    locked(order, |pool| pool.alloc_within(order, windows))
}

/// Run an allocation under the pool lock; an exhaustion is an event
/// (RFC-0087 §1): the allocator counted it, the console hears the 1st, 2nd,
/// 4th, 8th … one — outside the lock, with the numbers. Before anyone is refused the
/// magazines' frames go back and the pool is asked once more.
fn locked(
    order: u8,
    f: impl Fn(&mut FrameAllocator) -> Result<Block, FrameError>,
) -> Result<Block, FrameError> {
    let ask = |again: bool| match POOL.lock().as_mut() {
        Some(pool) => {
            if again {
                pool.retract_exhaustion();
            }
            let result = f(pool);
            let count = if result.is_err() { pool.stats().exhausted } else { 0 };
            (result, count, Some(pool.free_frames()))
        }
        // No pool yet: every ask is refused, and says so.
        None => (Err(FrameError::Exhausted { order, free: 0 }), 1, None),
    };
    let mut asked = ask(false);
    if matches!(asked, (Err(FrameError::Exhausted { .. }), _, Some(_))) && flush_magazines() {
        asked = ask(true);
    }
    let (result, count, free) = asked;
    // ADR-0085: the level is read with the pool lock dropped (the tracker has its own).
    if let Some(free) = free {
        super::pressure_endpoint::observe(free as u64);
    }
    if let Err(FrameError::Exhausted { order, free }) = result {
        if crate::accounting::log_exhaustion(count) {
            log_error!(target: "mm",
                "MM: frames exhausted (want=order{} free={} count={}) event=exhaust.v1 resource=frames action=refused",
                order, free, count);
        }
    }
    result
}

/// Return a block. A refusal (foreign, misaligned, double) is a kernel bug
/// and is reported as one.
pub fn free(block: Block) {
    free_block(block);
}

/// [`free`], also for a magazine's frames on their way back.
fn free_block(block: Block) {
    let (result, free) = match POOL.lock().as_mut() {
        Some(pool) => (pool.free(block), Some(pool.free_frames())),
        None => (Err(FrameError::NotOwned), None),
    };
    if let Some(free) = free {
        super::pressure_endpoint::observe(free as u64);
    }
    if let Err(e) = result {
        log_error!(target: "mm", "MM: frame free refused ({:?}) base=0x{:x} order={}", e, block.base, block.order);
    }
}

/// `len` bytes as the largest blocks that fit, largest first — a run that is
/// 2 MiB-aligned stays a superpage when mapped. A fallback to smaller blocks is
/// the allocator's (`alloc_at_most`), not an exhaustion; on a real one
/// everything taken so far goes back and the error is the pool's (logged).
pub fn alloc_bytes(len: usize) -> Result<Vec<Block>, FrameError> {
    take_bytes(len, |pool, order| pool.alloc_at_most(order))
}

/// `alloc_bytes` inside the physical `windows` (a device's DMA reach).
pub fn alloc_bytes_within(len: usize, windows: &[(u64, u64)]) -> Result<Vec<Block>, FrameError> {
    take_bytes(len, |pool, order| pool.alloc_at_most_within(order, windows))
}

fn take_bytes(
    len: usize,
    take: impl Fn(&mut FrameAllocator, u8) -> Result<Block, FrameError>,
) -> Result<Vec<Block>, FrameError> {
    let mut blocks = Vec::new();
    let mut remaining = len.div_ceil(crate::frames::FRAME_SIZE as usize);
    while remaining > 0 {
        if blocks.len() == blocks.capacity() && !reserve_block_room(&mut blocks) {
            free_blocks(&blocks);
            return Err(FrameError::NoHeap);
        }
        let order = (remaining.ilog2() as u8).min(crate::frames::MAX_ORDER);
        match locked(order, |pool| take(pool, order)) {
            Ok(block) => {
                remaining -= block.frames();
                blocks.push(block);
            }
            Err(e) => {
                free_blocks(&blocks);
                return Err(e);
            }
        }
    }
    Ok(blocks)
}

/// Grows the block list ahead of the next take (ADR-0091): a caller-sized object's list stays
/// above the kernel heap's reserve, so a fragmented pool cannot exhaust the heap — the caller
/// hears `ENOMEM`, the kernel never panics.
fn reserve_block_room(blocks: &mut Vec<Block>) -> bool {
    let more = blocks.capacity().max(8);
    crate::kheap::admits(more * core::mem::size_of::<Block>()) && blocks.try_reserve(more).is_ok()
}

/// Keep the first `keep` frames of the allocated `block` and return the rest to the pool
/// (`FrameAllocator::trim`); the answer is the pieces that tile the kept part. A refusal is a
/// kernel bug (the block is not allocated): it is reported, and the block stays whole.
pub fn trim(block: Block, keep: usize) -> Vec<Block> {
    let (result, free) = match POOL.lock().as_mut() {
        Some(pool) => (pool.trim(block, keep), Some(pool.free_frames())),
        None => (Err(FrameError::NotOwned), None),
    };
    if let Some(free) = free {
        super::pressure_endpoint::observe(free as u64);
    }
    result.unwrap_or_else(|e| {
        log_error!(target: "mm", "MM: frame trim refused ({:?}) base=0x{:x} order={} keep={}", e, block.base, block.order, keep);
        alloc::vec![block]
    })
}

/// Return every block of a list.
pub fn free_blocks(blocks: &[Block]) {
    for block in blocks {
        free(*block);
    }
}

/// The counters (`None` before init). A frame waiting in a magazine is free: nobody owns it.
pub fn stats() -> Option<Stats> {
    let stats = POOL.lock().as_ref().map(FrameAllocator::stats)?;
    Some(Stats { free: stats.free + magazine_stats().1, ..stats })
}
