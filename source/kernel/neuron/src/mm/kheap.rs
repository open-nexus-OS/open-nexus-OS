// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The kernel heap (ADR-0093, ADR-0091): the arenas it is made of, how it grows, and
//! its reserve. Arena 0 is the image's `.bss.heap` (8 MiB, there before the frame pool); every
//! later arena is one block of the frame pool, taken when a caller-sized allocation finds the
//! heap short (`admits`) — the heap grows with the system's use, bounded by the pool, never by a
//! constant, and it never shrinks. A caller-sized allocation — a lazy object's page map, an
//! object's block list, a new task, a table's growth — may not eat into the last `RESERVE`
//! bytes: such a size is `ENOMEM` to that caller, never an `ALLOC-FAIL` panic in whatever kernel
//! path allocates next.
//!
//! NOT target-gated (the arenas and the growth schedule are host-proven); the pool side is.
//!
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host unit tests below (`room_admits`, `growth_order`, `Arenas` over two
//!   buffers, `test_reject_*`); `KINIT: kheap (…)`, `KHEAP: grew (…)`, `KSELFTEST: kheap grows
//!   ok (…)`; `test_reject_*` in syscall/api/reject_tests.rs
//! INVARIANTS: the allocator never grows itself (no reentrancy into the pool from inside an
//!   allocation); growth runs in `admits` only, with no kernel lock held by it — the pool's lock
//!   and the heap's are taken one after the other, never nested; an arena is never freed.
//! ADR: docs/adr/0093-tables-grow-bounded-by-the-pool-never-by-a-constant-reserving-costs-nothing-touching-costs-pages.md

use core::alloc::Layout;
use core::ptr::NonNull;

use linked_list_allocator::Heap;

/// Kernel heap bytes kept for the kernel's own bookkeeping.
pub(crate) const RESERVE: usize = 1024 * 1024;
/// Free bytes `admits` wants left above the reserve once it has admitted: the kernel's own
/// allocations, which never ask, find room, and the heap grows before the reserve binds.
#[cfg(target_os = "none")]
const LOW_WATER: usize = RESERVE;
/// The first arena the pool gives: 2^11 frames = 8 MiB, the size of the image's heap. Each
/// growth asks for twice the last, up to the pool's largest block (`frames::MAX_ORDER`).
const FIRST_GROWTH_ORDER: u8 = 11;
/// Arena slots, the image's included: at the doubling schedule 32 slots hold more than three
/// gibibytes — the pool is the bound before the slots are, on every board this kernel runs on.
pub(crate) const MAX_ARENAS: usize = 32;
const PAGE: usize = 4096;

/// Why the heap did not grow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GrowError {
    /// Every arena slot holds an arena.
    Slots,
    /// The pool has no block of the order asked, or no block it gives would hold the ask.
    #[cfg(target_os = "none")]
    Pool,
}

/// The arenas of the kernel heap: one `Heap` each, first fit across them in the order they came.
pub(crate) struct Arenas {
    heaps: [Option<Heap>; MAX_ARENAS],
    len: usize,
    next_order: u8,
}

impl Arenas {
    pub(crate) const fn empty() -> Self {
        const NONE: Option<Heap> = None;
        Self { heaps: [NONE; MAX_ARENAS], len: 0, next_order: FIRST_GROWTH_ORDER }
    }

    /// Adds `[start, start + len)` as the next arena.
    ///
    /// # Safety
    /// The range is memory nothing else uses, valid for the kernel's whole life.
    pub(crate) unsafe fn add(&mut self, start: *mut u8, len: usize) -> Result<(), GrowError> {
        let slot = self.heaps.get_mut(self.len).ok_or(GrowError::Slots)?;
        // SAFETY: the caller's contract, forwarded.
        *slot = Some(unsafe { Heap::new(start, len) });
        self.len += 1;
        Ok(())
    }

    /// First fit across the arenas, in order; `None` when no arena holds `layout`.
    pub(crate) fn allocate_first_fit(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        self.heaps.iter_mut().flatten().find_map(|heap| heap.allocate_first_fit(layout).ok())
    }

    /// Returns `ptr` to the arena that handed it out. A pointer no arena holds is a kernel bug:
    /// nothing is freed.
    ///
    /// # Safety
    /// `ptr` and `layout` are those of a prior `allocate_first_fit`.
    pub(crate) unsafe fn deallocate(&mut self, ptr: NonNull<u8>, layout: Layout) {
        let addr = ptr.as_ptr() as usize;
        let holder = self
            .heaps
            .iter_mut()
            .flatten()
            .find(|heap| (heap.bottom() as usize..heap.top() as usize).contains(&addr));
        if let Some(heap) = holder {
            // SAFETY: the caller's contract; this arena is the one that handed `ptr` out.
            unsafe { heap.deallocate(ptr, layout) };
        }
    }

    /// Bytes handed out, over every arena.
    pub(crate) fn used(&self) -> usize {
        self.heaps.iter().flatten().map(Heap::used).sum()
    }

    /// Bytes free, over every arena (not one free block).
    pub(crate) fn free(&self) -> usize {
        self.heaps.iter().flatten().map(Heap::free).sum()
    }

    /// Arenas the heap is made of.
    pub(crate) fn arenas(&self) -> usize {
        self.len
    }

    /// The order the next growth asks the pool for.
    pub(crate) fn next_growth_order(&self) -> u8 {
        self.next_order
    }

    /// An arena of `order` came: the next ask doubles, up to the pool's largest block.
    pub(crate) fn note_grown(&mut self, order: u8) {
        self.next_order = order.saturating_add(1).min(crate::frames::MAX_ORDER);
    }
}

/// The order of the block the next growth asks the pool for: the schedule's `next`, or more
/// when `bytes` (plus one page for the arena's own metadata) would not fit in that one — a first
/// fit needs them in ONE arena. `None` when no block the pool gives would hold them: that size
/// is refused outright, the pool is not asked.
pub(crate) fn growth_order(next: u8, bytes: usize) -> Option<u8> {
    let frames = bytes.checked_add(PAGE)?.div_ceil(PAGE).checked_next_power_of_two()?;
    let order = u8::try_from(frames.trailing_zeros()).ok()?.max(next);
    (order <= crate::frames::MAX_ORDER).then_some(order)
}

/// Whether `bytes` more fit on a heap with `free` free bytes, above [`RESERVE`].
pub(crate) const fn room_admits(free: usize, bytes: usize) -> bool {
    match free.checked_sub(RESERVE) {
        Some(room) => bytes <= room,
        None => false,
    }
}

/// Kernel heap bytes handed out: a lock and a metadata read, no allocation — the pair the OOM
/// handler snapshots, read at the telemetry fence.
#[cfg(target_os = "none")]
pub(crate) fn used_bytes() -> usize {
    crate::ALLOC.0.lock().used()
}

/// Kernel heap bytes free; `used + free` is the heap's size.
#[cfg(target_os = "none")]
pub(crate) fn free_bytes() -> usize {
    crate::ALLOC.0.lock().free()
}

/// Arenas the heap is made of (1 until the first growth).
#[cfg(target_os = "none")]
pub(crate) fn arena_count() -> usize {
    crate::ALLOC.0.lock().arenas()
}

/// Whether the kernel heap gives `bytes` to a caller-sized allocation now. Short of them above
/// the reserve and the low water, the heap grows once from the pool first (ADR-0093); the
/// allocation itself stays fallible (`try_reserve`), since free bytes are not one free block.
#[cfg(target_os = "none")]
pub(crate) fn admits(bytes: usize) -> bool {
    let free = free_bytes();
    if room_admits(free, bytes.saturating_add(LOW_WATER)) {
        return true;
    }
    match grow(bytes) {
        Ok(()) => room_admits(free_bytes(), bytes),
        // The pool refused: the heap is what it is, and the reserve still rules.
        Err(_) => room_admits(free, bytes),
    }
}

/// One arena more, from the pool. No lock is held on entry: the pool's and the heap's are taken
/// one after the other, and the console hears the growth after both are dropped.
#[cfg(target_os = "none")]
fn grow(bytes: usize) -> Result<(), GrowError> {
    let next = crate::ALLOC.0.lock().next_growth_order();
    let order = growth_order(next, bytes).ok_or(GrowError::Pool)?;
    let block = crate::mm::frame_pool::alloc(order).map_err(|_| GrowError::Pool)?;
    let start = crate::phys::phys_to_virt(block.base as usize) as *mut u8;
    let len = block.size() as usize;
    let (added, arenas, size) = {
        let mut heap = crate::ALLOC.0.lock();
        // SAFETY: a block the pool just gave, reached through the direct map, kept for the
        // kernel's life (an arena is never freed).
        let added = unsafe { heap.add(start, len) };
        if added.is_ok() {
            heap.note_grown(order);
        }
        (added, heap.arenas(), heap.used() + heap.free())
    };
    match added {
        Ok(()) => {
            log_info!(target: "mm", "KHEAP: grew (arena={} bytes={} size={})", arenas, len, size);
            Ok(())
        }
        Err(e) => {
            crate::mm::frame_pool::free(block);
            log_error!(target: "mm", "KHEAP: growth refused ({:?}) arenas={}", e, arenas);
            Err(e)
        }
    }
}

/// Host builds run on the host's allocator: the target heap's reserve never binds there — unless
/// a test says the pool refused (`refusal::PoolRefuses`), so a table's refusal is host-proven.
#[cfg(not(target_os = "none"))]
pub(crate) fn admits(bytes: usize) -> bool {
    !pool_refuses() && room_admits(usize::MAX, bytes)
}

#[cfg(all(not(target_os = "none"), test))]
fn pool_refuses() -> bool {
    refusal::active()
}

#[cfg(all(not(target_os = "none"), not(test)))]
fn pool_refuses() -> bool {
    false
}

/// The host tests' pool: while a `PoolRefuses` lives, `admits` answers `false` on its thread —
/// every table's growth past the pool is then provable without a pool.
#[cfg(all(not(target_os = "none"), test))]
pub(crate) mod refusal {
    use std::cell::Cell;

    thread_local! {
        static REFUSES: Cell<bool> = const { Cell::new(false) };
    }

    /// The pool refuses every growth while this lives.
    pub(crate) struct PoolRefuses;

    impl PoolRefuses {
        pub(crate) fn new() -> Self {
            REFUSES.with(|c| c.set(true));
            Self
        }
    }

    impl Drop for PoolRefuses {
        fn drop(&mut self) {
            REFUSES.with(|c| c.set(false));
        }
    }

    pub(crate) fn active() -> bool {
        REFUSES.with(Cell::get)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer(len: usize) -> *mut u8 {
        Box::leak(vec![0u8; len].into_boxed_slice()).as_mut_ptr()
    }

    #[test]
    fn test_reject_an_allocation_that_eats_into_the_reserve() {
        assert!(!room_admits(RESERVE - 1, 0), "a heap already inside its reserve admits nothing");
        assert!(!room_admits(RESERVE + 4096, 4097), "one byte into the reserve is refused");
        assert!(room_admits(RESERVE + 4096, 4096), "up to the reserve is admitted");
        assert!(room_admits(RESERVE, 0));
        assert!(!room_admits(0, usize::MAX));
    }

    #[test]
    fn the_schedule_doubles_and_an_ask_picks_the_block_that_holds_it() {
        assert_eq!(growth_order(FIRST_GROWTH_ORDER, 1024), Some(11));
        assert_eq!(growth_order(14, 1024), Some(14), "the schedule's next, when the ask fits");
        assert_eq!(growth_order(11, (8 << 20) - PAGE), Some(11), "just fits the 8 MiB block");
        assert_eq!(growth_order(11, 8 << 20), Some(12), "a page of metadata tips it over");
        assert_eq!(growth_order(11, 64 << 20), Some(15), "64 MiB and a page need the largest");
    }

    #[test]
    fn test_reject_a_growth_no_block_of_the_pool_would_hold() {
        assert_eq!(growth_order(11, 128 << 20), None);
        assert_eq!(growth_order(11, usize::MAX), None);
    }

    #[test]
    fn arenas_serve_first_fit_in_order_and_free_into_the_one_that_gave() {
        let mut arenas = Arenas::empty();
        for _ in 0..2 {
            // SAFETY: a leaked buffer nobody else uses.
            unsafe { arenas.add(buffer(64 * 1024), 64 * 1024) }.unwrap();
        }
        assert_eq!(arenas.arenas(), 2);
        let size = arenas.used() + arenas.free();
        let layout = Layout::from_size_align(40 * 1024, 16).unwrap();
        let a = arenas.allocate_first_fit(layout).expect("the first arena holds 40 KiB");
        let b = arenas.allocate_first_fit(layout).expect("the second holds the next 40 KiB");
        assert!(arenas.allocate_first_fit(layout).is_none(), "no arena holds a third");
        assert!(arenas.used() >= 80 * 1024);
        // SAFETY: the two allocations above, with their layout.
        unsafe {
            arenas.deallocate(a, layout);
            arenas.deallocate(b, layout);
        }
        assert_eq!(arenas.used(), 0);
        assert_eq!(arenas.used() + arenas.free(), size, "an arena is never freed");
    }

    #[test]
    fn test_reject_an_arena_past_the_slots() {
        let mut arenas = Arenas::empty();
        for _ in 0..MAX_ARENAS {
            // SAFETY: a leaked buffer nobody else uses.
            unsafe { arenas.add(buffer(4096), 4096) }.unwrap();
        }
        // SAFETY: as above.
        assert_eq!(unsafe { arenas.add(buffer(4096), 4096) }, Err(GrowError::Slots));
        assert_eq!(arenas.arenas(), MAX_ARENAS);
    }

    #[test]
    fn the_next_order_follows_the_arena_that_came() {
        let mut arenas = Arenas::empty();
        assert_eq!(arenas.next_growth_order(), FIRST_GROWTH_ORDER);
        arenas.note_grown(11);
        assert_eq!(arenas.next_growth_order(), 12);
        arenas.note_grown(crate::frames::MAX_ORDER);
        assert_eq!(arenas.next_growth_order(), crate::frames::MAX_ORDER, "capped at the largest");
    }

    #[test]
    fn test_reject_every_growth_while_the_pool_refuses() {
        assert!(admits(1 << 30), "the host's pool is the host");
        let refuses = refusal::PoolRefuses::new();
        assert!(!admits(1));
        drop(refuses);
        assert!(admits(1));
    }
}
