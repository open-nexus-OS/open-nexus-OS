// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the `GlobalAlloc` glue — where every allocation of an OS service
//! is routed (ADR-0065). Split out of `lib.rs` under the structure ratchet:
//! `Bump` is the POLICY (a cursor, an arena, free lists); this is the trait
//! that asks it, in a fixed order:
//!
//! 1. an open frame generation (`Region::Emit` / `Region::Layout`) — recycled
//!    wholesale two frames later, never freed;
//! 2. a parked block of the request's size class — durable state that was
//!    freed and comes back for its class;
//! 3. the bump, carving a class-rounded block so it can be reused later.
//!
//! `dealloc` mirrors it: an arena block is NOT ours to park (its reset would
//! overwrite a list's promise), a small durable block goes to its class, and
//! the bump never reclaims. `alloc_zeroed` zeroes anything reused — only
//! untouched `.bss` from the bump is zero for free.
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the rules in `crate::generation` / `crate::freelist` (host);
//!   the wiring by every OS boot and the `apphost: heap steady` marker

use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};

use super::{log_alloc_corruption, log_alloc_failure, log_alloc_zeroed, Bump, ALLOCATOR};
use crate::cursor_tripwire::check_cursor_monotone;

/// Bytes the bump carved for class-sized blocks (working-set growth: a class
/// whose lists were empty) and for everything else (large or over-aligned) —
/// so the probe can say WHICH kind of growth a moving base heap is.
static CLASS_CARVED: AtomicUsize = AtomicUsize::new(0);
static LARGE_CARVED: AtomicUsize = AtomicUsize::new(0);

/// Frame generations opened since boot. Non-zero means the base heap is
/// expected to be STEADY, so every further carve is worth one line — and the
/// count places that carve on the frame axis the probe reports in.
static GENERATIONS: AtomicUsize = AtomicUsize::new(0);

pub(super) fn note_generation() {
    GENERATIONS.fetch_add(1, Ordering::Relaxed);
}

pub(super) fn generation_count() -> usize {
    GENERATIONS.load(Ordering::Relaxed)
}

fn note_carve(class: bool, bytes: usize, layout: Layout) {
    if class {
        CLASS_CARVED.fetch_add(bytes, Ordering::Relaxed);
    } else {
        LARGE_CARVED.fetch_add(bytes, Ordering::Relaxed);
    }
    // A carve AFTER the first frame is the base heap growing in a session —
    // rare by construction once durable state frees, and exactly what a
    // reader chasing a slow leak needs: the size and alignment, alloc-free.
    let generation = GENERATIONS.load(Ordering::Relaxed);
    if bytes != 0 && generation != 0 {
        use crate::debug_write::{debug_write_byte, debug_write_bytes, debug_write_hex};
        debug_write_bytes(b"alloc-carve gen=0x");
        debug_write_hex(generation);
        debug_write_bytes(b" svc=");
        crate::debug_write::debug_write_str(super::service_name());
        debug_write_bytes(b" size=0x");
        debug_write_hex(layout.size());
        debug_write_bytes(b" align=0x");
        debug_write_hex(layout.align());
        debug_write_bytes(if class { b" class=1" } else { b" class=0" });
        debug_write_byte(b'\n');
    }
}

/// `(class_bytes, large_bytes)` carved from the bump since boot.
pub fn carve_stats() -> (usize, usize) {
    (CLASS_CARVED.load(Ordering::Relaxed), LARGE_CARVED.load(Ordering::Relaxed))
}

pub(super) struct GlobalAllocator;

unsafe impl GlobalAlloc for GlobalAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATOR.ensure_init();
        let mut bump = ALLOCATOR.inner.lock();
        // The open generation answers first; outside a scope this is a
        // single branch and the base heap serves exactly as before.
        if let Some(ptr) = bump.arena_alloc(layout) {
            return ptr;
        }
        // Durable state: a parked block of this class first; otherwise
        // carve one the class can reuse later (TASK-0077C P3).
        let (class, layout) = Bump::class_layout(layout);
        if let Some(ptr) = bump.reuse(class) {
            return ptr;
        }
        let heap_start = bump.start;
        let heap_end = bump.end;
        let cur_before = bump.current;
        let mut ptr = bump.alloc(layout);
        let cur_after = bump.current;
        note_carve(class.is_some(), cur_after - cur_before, layout);
        if ptr.is_null() && layout.size() == 0 {
            ptr = bump.current as *mut u8;
        }
        let exhausted = ptr.is_null() && layout.size() != 0;
        drop(bump);
        check_cursor_monotone(cur_after);
        if exhausted {
            log_alloc_failure(
                "alloc",
                layout.size(),
                layout.align(),
                heap_start,
                heap_end,
                cur_before,
                cur_after,
            );
            return ptr;
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATOR.ensure_init();
        let mut bump = ALLOCATOR.inner.lock();
        // The open generation answers first — but it must be ZEROED here.
        // The base heap below can skip that because a never-freeing bump
        // only ever hands out untouched `.bss`, which is already zero; the
        // arena REUSES memory, so without this it would hand back the
        // frame from two frames ago.
        if let Some(ptr) = bump.arena_alloc(layout) {
            ptr::write_bytes(ptr, 0, layout.size());
            return ptr;
        }
        let requested = layout.size();
        let (class, layout) = Bump::class_layout(layout);
        if let Some(ptr) = bump.reuse(class) {
            // A block that was FREED holds its old bytes (or poison).
            ptr::write_bytes(ptr, 0, requested);
            return ptr;
        }
        let heap_start = bump.start;
        let heap_end = bump.end;
        let cur_before = bump.current;
        let mut ptr = bump.alloc(layout);
        let cur_after = bump.current;
        if ptr.is_null() && layout.size() == 0 {
            ptr = bump.current as *mut u8;
        }
        let exhausted = ptr.is_null() && layout.size() != 0;
        drop(bump);
        check_cursor_monotone(cur_after);
        if exhausted {
            log_alloc_failure(
                "alloc_zeroed",
                layout.size(),
                layout.align(),
                heap_start,
                heap_end,
                cur_before,
                cur_after,
            );
            log_alloc_zeroed(layout.size(), layout.align(), ptr as usize);
            return ptr;
        }

        if !ptr.is_null() && layout.size() != 0 {
            let addr = ptr as usize;
            let end = addr.checked_add(layout.size()).unwrap_or(usize::MAX);
            if addr < heap_start || end > heap_end {
                log_alloc_corruption(
                    addr,
                    layout.size(),
                    heap_start,
                    heap_end,
                    cur_before,
                    cur_after,
                );
                panic!("alloc_zeroed returned pointer outside heap range");
            }
            // Task #14 root cause lived here: a debug shim moved ptr/size
            // into s5/s6 around this write WITHOUT declaring the clobber.
            // Depending on register allocation (i.e. on the binary
            // layout), it destroyed live caller values — the wild follow-up
            // stores reset the bump cursor and overlapped live
            // allocations ("impossible" empty/à-la-carte corruption in
            // f32/alloc-heavy code). Plain write_bytes is all this needs.
            ptr::write_bytes(ptr, 0, layout.size());
        }
        log_alloc_zeroed(layout.size(), layout.align(), ptr as usize);
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // The bump never reclaims. Two things do (ADR-0065): a frame
        // arena block is reclaimed by its generation's reset — it must
        // NOT be parked on a list, or the list would hand out memory the
        // next reset overwrites — and a small durable block goes back to
        // its size class, if this service opted in.
        if layout.size() == 0 || Bump::in_arena(ptr as usize) {
            return;
        }
        #[cfg(feature = "small-object-free-list")]
        if let Some(class) = crate::freelist::class_for(layout.size(), layout.align()) {
            ALLOCATOR.ensure_init();
            // SAFETY: `ptr` was carved by `alloc` for exactly this class
            // (same `class_for`, same rounding), and the caller is done
            // with it — the `GlobalAlloc` contract.
            unsafe {
                ALLOCATOR.inner.lock().lists.push(
                    class,
                    ptr as usize,
                    cfg!(feature = "frame-arena-poison"),
                );
            }
        }
        #[cfg(not(feature = "small-object-free-list"))]
        let _ = (ptr, layout);
    }
}

#[global_allocator]
static GLOBAL: GlobalAllocator = GlobalAllocator;
