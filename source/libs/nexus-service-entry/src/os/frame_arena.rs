// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the frame arena's SCOPE API — what a service calls to run a frame
//! phase inside a generation (ADR-0065, TASK-0077C). Split out of `lib.rs`
//! under the structure ratchet: this is a facility built ON the allocator, not
//! the allocator, and the seam is the `ALLOCATOR` static it borrows.
//!
//! The rule it implements lives in `crate::generation` (host-proven); the
//! memory lives in the parent module. This file is the guard, the close for a
//! guard someone else owns, the poison tripwire, and the probe's read side.
//!
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `apphost: frame arena` / `heap steady` markers (visible lane);
//!   the rule itself in `crate::generation`'s unit tests

use super::{Region, ALLOCATOR, ARENA_SIZE};

/// A frame generation, open until this guard drops (ADR-0065).
///
/// While it lives, allocations come from the arena instead of the base
/// heap, and the generation opened two frames ago is reset. A guard rather
/// than an open/close pair on purpose: a scope left open by an early return
/// would put a later store write into memory that is about to be reused,
/// and that is the one hazard this design has.
///
/// A service that did not enable `frame-arena`, or one whose arena is not
/// armed, gets a guard that changes nothing.
#[must_use = "the generation closes when this guard drops; binding it to `_` closes it at once"]
pub struct FrameGeneration {
    region: Region,
}

/// Opens the next frame generation for `region`. See [`FrameGeneration`].
///
/// With `frame-arena-poison` the bytes the reset gave up are filled with
/// `0xDE` first — the tripwire that turns "nothing outlives its
/// generation" from a promise into a boot-time detector: a stale reader
/// sees a length or a pointer of `0xDEDE…` and fails at once, instead of
/// quietly reading the frame from two frames ago. It costs one memset of
/// the USED bytes (~48-78 KiB measured), not of the whole half.
pub fn frame_generation(region: Region) -> FrameGeneration {
    ALLOCATOR.ensure_init();
    let reset = ALLOCATOR.inner.lock().gens[region.index()].open();
    super::global_alloc::note_generation();
    #[cfg(feature = "frame-arena-poison")]
    if reset.1 != 0 {
        // SAFETY: the range is the half this allocator owns and just
        // reset; by the invariant nothing references it any more.
        unsafe { core::ptr::write_bytes(reset.0 as *mut u8, 0xDE, reset.1) };
    }
    #[cfg(not(feature = "frame-arena-poison"))]
    let _ = reset;
    FrameGeneration { region }
}

impl Drop for FrameGeneration {
    fn drop(&mut self) {
        close_frame_generation(self.region);
    }
}

/// Closes a generation opened with [`frame_generation`] whose guard was
/// deliberately leaked — the shape a caller needs when the guard's
/// lifetime is owned by someone else's type (the DSL runtime's
/// `FrameScopeGuard`).
pub fn close_frame_generation(region: Region) {
    ALLOCATOR.inner.lock().gens[region.index()].close();
}

/// Arena introspection for the budget probe: `(peak_bytes, spilled, size)`.
///
/// `spilled` is the one that matters: it means a frame did not fit and the
/// base heap served the remainder — the leak this arena exists to end,
/// still happening. The gate asserts it is false.
pub fn arena_stats() -> (usize, bool, usize) {
    ALLOCATOR.ensure_init();
    let bump = ALLOCATOR.inner.lock();
    let peak = bump.gens.iter().map(crate::generation::Generations::peak).sum();
    let spilled = bump.gens.iter().any(crate::generation::Generations::spilled);
    (peak, spilled, ARENA_SIZE)
}

/// Blocks parked on the durable-state free lists — reclaimable memory the
/// probe reports beside the arena's numbers. `0` where the feature is off.
pub fn free_list_stats() -> usize {
    #[cfg(feature = "small-object-free-list")]
    {
        ALLOCATOR.ensure_init();
        ALLOCATOR.inner.lock().lists.free_blocks()
    }
    #[cfg(not(feature = "small-object-free-list"))]
    {
        0
    }
}

/// Generations opened since boot, across both regions — the clock the carve
/// diagnostic and the probe share, so a carve can be placed on the frame axis.
pub fn generation_count() -> usize {
    super::global_alloc::generation_count()
}
