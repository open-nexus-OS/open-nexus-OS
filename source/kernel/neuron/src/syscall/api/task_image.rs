// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: returns a finished task's process image to the user VMO arena.
//! `exec` allocates every PT_LOAD segment, the stack and the bootstrap
//! metadata pages from `VMO_POOL` and records them on the task
//! (`image_allocs::ImageAllocs`); this module is the ONLY place that hands
//! them back. Until it existed the arena was bump-only for process images: a
//! session that opened and closed a handful of apps exhausted it and the
//! next launch died silently (RFC-0075 8e — the reported crash).
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `ImageAllocs` host unit tests (image_allocs.rs) + interactive
//!   proof — an open/close app storm no longer exhausts the arena
//!   (`VMO-POOL exhausted` never fires; reclaim visible under
//!   `NEXUS_LOG=exec=debug` as `IMAGE-RECLAIM`).
//! INVARIANTS:
//!   - Called only for a task that is already a Zombie (purged from the
//!     scheduler, never dispatched again), so no hart executes these pages.
//!   - Only USER image/stack ranges are returned — never page tables, never
//!     kernel mappings; a hart idling with a stale `satp` keeps running
//!     kernel text, which is not part of any returned range.

use super::vmo::VMO_POOL;
use crate::image_allocs::ImageAllocs;
use crate::task::TaskTable;

/// Returns every recorded range to the arena. Reports bytes reclaimed.
/// A rejected range (bounds/overlap) is logged and left allocated rather
/// than retried — a wrong free would corrupt the arena.
pub fn release_image(allocs: &ImageAllocs) -> usize {
    let mut bytes = 0usize;
    let mut rejected = 0usize;
    for (base, len) in allocs.iter() {
        if VMO_POOL.lock().free(base, len).is_ok() {
            bytes = bytes.saturating_add(len);
        } else {
            rejected += 1;
        }
    }
    if rejected != 0 || allocs.untracked() != 0 {
        // Honest accounting: memory we could NOT return stays allocated.
        log_error!(
            "IMAGE-RECLAIM incomplete: rejected={} untracked={} reclaimed=0x{:x}",
            rejected,
            allocs.untracked(),
            bytes
        );
    } else if bytes != 0 {
        // RFC-0068: process teardown is a perpetual runtime event → DEBUG.
        log_debug!(target: "exec", "IMAGE-RECLAIM 0x{:x} bytes ({} ranges)", bytes, allocs.len());
    }
    bytes
}

/// Exits the current task and returns its process image to the arena — the
/// single funnel every exit path uses (`sys_exit` and the trap handler's
/// fault exits), so no teardown can forget the memory.
pub fn exit_current_and_release(tasks: &mut TaskTable, status: i32) {
    let allocs = tasks.exit_current(status);
    let _ = release_image(&allocs);
}
