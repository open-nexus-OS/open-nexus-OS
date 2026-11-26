// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: A VMO dies with its last capability (RFC-0100 §1, TASK-0333 6a). Before this, an
//! exiting task's capability table was dropped and its objects stayed in the table with their
//! frames — one holder a cycle, measured (`docs/board/measurements/2026-10-20-memory-lifecycle-
//! baseline/`). The decision is one rule, proven on the host: an object nobody names and nobody
//! maps is freed. Three callers apply it: the reap (the objects the zombie named at its exit and
//! the ones its address space mapped, once the space is gone), the explicit `cap_close` of a
//! VMO capability, and the unmap of a region over it (TASK-0333 6b-3: an object whose last
//! capability closed while it was mapped dies at its last unmap) — `sys_vmo_destroy` keeps
//! RFC-0085's sole-owner rule as the owner's fast path. Since TASK-0330 P4 a fourth: the end
//! of a syscall that held an object for its length (`judge`, from `syscall::api::pinned`). The
//! judge takes no lock but the object table's and runs beside creators and other judges: an
//! object is born with its first capability (`vmo::Born`), so one that nobody holds and
//! nobody maps is garbage whichever object its id names by then, and `vmo::destroy` reads
//! both counts again in the hold that takes it.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host (`should_reclaim`, the `test_reject_*` below); QEMU `KSELFTEST: vm reclaim (`
//!   and `SELFTEST: rss loop ok (`; the board ladder
//! RFC: docs/rfcs/RFC-0100-memory-object-model-v2-commit-on-touch-unified-gpu-memory-pressure.md

use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use super::vmo;

/// Objects freed by this path since boot, and their bytes (`KSELFTEST: vm reclaim (`).
static RECLAIMED_VMOS: AtomicUsize = AtomicUsize::new(0);
static RECLAIMED_BYTES: AtomicUsize = AtomicUsize::new(0);

use crate::vmo_lifetime::should_reclaim;

/// Reclaims named on the console before the line goes quiet (the first ones of a boot name
/// the objects a reader chasing a lost object needs; the fence's total counts the rest).
const NAMED_RECLAIMS: usize = 24;

/// Free every object in `ids` the rule allows; each id judged once; `by` names the caller
/// (`reap`, `close`) on the console. Returns what was freed: objects and bytes (a `Fixed`
/// object frees nothing and counts its length as 0).
///
/// Runs under no lock of the caller's and beside other judges and creators: an id here may
/// already name the next object — but an object is born held (`vmo::Born`), and `destroy`
/// reads the counts in the hold that takes the object, so what goes is an object nobody holds
/// and nobody maps, whichever one the id names by then.
pub fn orphans(ids: &[u32], by: &'static str) -> (usize, usize) {
    let (mut freed, mut bytes) = (0usize, 0usize);
    let mut seen: Vec<u32> = Vec::new();
    for &id in ids {
        if id == 0 || seen.contains(&id) {
            continue;
        }
        seen.push(id);
        // The capabilities held to it, in a table or in a message, and the address spaces
        // that map it are the object's own counts (RFC-0102): no table and no space is asked.
        // `destroy` reads both again in the hold that takes the object.
        if !should_reclaim(vmo::held(id), vmo::mapping::spaces(id) > 0) {
            continue;
        }
        if let Ok(len) = vmo::destroy(id) {
            if RECLAIMED_VMOS.load(Ordering::Relaxed) + freed < NAMED_RECLAIMS {
                // The record after the free, from the kernel's own lines (a service's lines fold
                // in the interactive session): the pool's free frames, the live objects.
                let free = super::frame_pool::stats().map_or(0, |s| s.free);
                let (live, ..) = vmo::stats();
                log_info!(
                    target: "mm",
                    "KINIT: vm reclaim (id={} bytes={} by={} free={} vmos={})",
                    id,
                    len,
                    by,
                    free,
                    live
                );
            }
            freed += 1;
            bytes += len;
        }
    }
    RECLAIMED_VMOS.fetch_add(freed, Ordering::Relaxed);
    RECLAIMED_BYTES.fetch_add(bytes, Ordering::Relaxed);
    (freed, bytes)
}

/// A path let go of memory object `id` — a capability it held, a region that mapped it:
/// with no capability held to it any more, its life is judged now (RFC-0100 §1).
pub fn judge(id: u32, by: &'static str) {
    if id != 0 && vmo::held(id) == 0 {
        orphans(&[id], by);
    }
}

/// Objects and bytes freed by this path since boot.
pub fn stats() -> (usize, usize) {
    (RECLAIMED_VMOS.load(Ordering::Relaxed), RECLAIMED_BYTES.load(Ordering::Relaxed))
}
