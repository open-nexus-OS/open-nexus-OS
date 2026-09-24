// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the kernel's frame pool (TASK-0286 P2b) — the ONE `FrameAllocator`
//! (`crate::frames`, host-proven) over the banks the tree names, built once by
//! the boot hart after the heap exists and before the first page table.
//! Carved out: the tree's reserved ranges, the kernel image and the tree
//! itself — nothing else; the last fixed windows died with P3b. Page tables
//! (P2b), VMOs, process images (P3a), user stacks and init's pages (P3b)
//! allocate here. Exhaustion is an event on the console AND a
//! counter — never a silent `None`.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the allocator is host-tested in `crate::frames`; this wrapper
//!   is proven by every boot (`KINIT: mm frames (…)`) and every page table
//! INVARIANTS: one pool, one lock (leaf: never held across a log line); a
//!   refused free is reported, never swallowed.

use crate::frames::{Block, FrameAllocator, FrameError, Range, Stats};

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
    let mut excluded = [Range { base: 0, size: 0 }; 2];
    let mut n = 0;
    for range in [Some(image), tree].into_iter().flatten() {
        excluded[n] = range;
        n += 1;
    }
    let pool = FrameAllocator::init_from_fdt(&fdt, &excluded[..n]).map_err(InitError::Frames)?;
    let stats = pool.stats();
    *POOL.lock() = Some(pool);
    Ok(stats)
}

/// A block of `1 << order` frames, or the error.
pub fn alloc(order: u8) -> Result<Block, FrameError> {
    locked(order, |pool| pool.alloc(order))
}

/// The largest free block of `order` or below (an object that takes several
/// runs); exhausted only when not a single frame is left.
pub fn alloc_at_most(order: u8) -> Result<Block, FrameError> {
    locked(order, |pool| pool.alloc_at_most(order))
}

/// Run an allocation under the pool lock; an exhaustion is an event
/// (RFC-0087 §1): the allocator counted it, the console hears the 1st, 2nd,
/// 4th, 8th … one — outside the lock, with the numbers.
fn locked(
    order: u8,
    f: impl FnOnce(&mut FrameAllocator) -> Result<Block, FrameError>,
) -> Result<Block, FrameError> {
    let (result, count) = match POOL.lock().as_mut() {
        Some(pool) => {
            let result = f(pool);
            let count = if result.is_err() { pool.stats().exhausted } else { 0 };
            (result, count)
        }
        // No pool yet: every ask is refused, and says so.
        None => (Err(FrameError::Exhausted { order, free: 0 }), 1),
    };
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
    let result = match POOL.lock().as_mut() {
        Some(pool) => pool.free(block),
        None => Err(FrameError::NotOwned),
    };
    if let Err(e) = result {
        log_error!(target: "mm", "MM: frame free refused ({:?}) base=0x{:x} order={}", e, block.base, block.order);
    }
}

/// `len` bytes as the largest blocks that fit, largest first — a run that is
/// 2 MiB-aligned stays a superpage when mapped. A fallback to smaller blocks is
/// the allocator's (`alloc_at_most`), not an exhaustion; on a real one
/// everything taken so far goes back and the error is the pool's (logged).
pub fn alloc_bytes(len: usize) -> Result<alloc::vec::Vec<Block>, FrameError> {
    let mut blocks = alloc::vec::Vec::new();
    let mut remaining = len.div_ceil(crate::frames::FRAME_SIZE as usize);
    while remaining > 0 {
        let order = (remaining.ilog2() as u8).min(crate::frames::MAX_ORDER);
        match alloc_at_most(order) {
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

/// Return every block of a list.
pub fn free_blocks(blocks: &[Block]) {
    for block in blocks {
        free(*block);
    }
}

/// The counters (`None` before init).
pub fn stats() -> Option<Stats> {
    POOL.lock().as_ref().map(FrameAllocator::stats)
}
