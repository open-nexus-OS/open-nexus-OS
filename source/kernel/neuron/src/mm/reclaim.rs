// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: A VMO dies with its last capability (RFC-0100 §1, TASK-0333 6a). Before this, an
//! exiting task's capability table was dropped and its objects stayed in the table with their
//! frames — one holder a cycle, measured (`docs/board/measurements/2026-10-20-memory-lifecycle-
//! baseline/`). The decision is one rule, proven on the host: an object nobody names and nobody
//! maps is freed. Three callers apply it: the reap (the objects the zombie named at its exit and
//! the ones its address space mapped, once the space is gone), the explicit `cap_close` of a
//! VMO capability, and nothing else — `sys_vmo_destroy` keeps RFC-0085's sole-owner rule as the
//! owner's fast path.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host (`should_reclaim`, the `test_reject_*` below); QEMU `KSELFTEST: vm reclaim (`
//!   and `SELFTEST: rss loop ok (`; the board ladder
//! RFC: docs/rfcs/RFC-0100-memory-object-model-v2-commit-on-touch-unified-gpu-memory-pressure.md

use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use super::{vm_ops, vmo, AddressSpaceManager};
use crate::task::TaskTable;
use crate::types::Pid;

/// Objects freed by this path since boot, and their bytes (`KSELFTEST: vm reclaim (`).
static RECLAIMED_VMOS: AtomicUsize = AtomicUsize::new(0);
static RECLAIMED_BYTES: AtomicUsize = AtomicUsize::new(0);

use crate::vmo_lifetime::should_reclaim;

/// How many capabilities across every task name `id` (`Vmo` and `VmoRo` alike).
fn names(tasks: &TaskTable, id: u32) -> usize {
    (0..tasks.len() as u32)
        .filter_map(|raw| tasks.caps_of(Pid::from_raw(raw)))
        .map(|caps| caps.vmo_ref_count(id))
        .sum()
}

/// Reclaims named on the console before the line goes quiet (the first ones of a boot name
/// the objects a reader chasing a lost object needs; the fence's total counts the rest).
const NAMED_RECLAIMS: usize = 24;

/// Free every object in `ids` the rule allows; each id judged once; `by` names the caller
/// (`reap`, `close`) on the console. An object between a create's phases is left alone. Returns
/// what was freed: objects and bytes (a `Fixed` object frees nothing and counts its length as 0).
pub fn orphans(
    tasks: &TaskTable,
    spaces: &AddressSpaceManager,
    ids: &[u32],
    by: &'static str,
) -> (usize, usize) {
    let (mut freed, mut bytes) = (0usize, 0usize);
    let mut seen: Vec<u32> = Vec::new();
    for &id in ids {
        if id == 0 || seen.contains(&id) || vmo::is_creating(id) {
            continue;
        }
        seen.push(id);
        if !should_reclaim(names(tasks, id), vm_ops::any_space_maps_vmo(spaces, id)) {
            continue;
        }
        let Some(len) = vmo::with(id, |o| o.len()) else { continue };
        let len = if vmo::is_fixed(id) { 0 } else { len };
        if vmo::destroy(id).is_ok() {
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

/// Objects and bytes freed by this path since boot.
pub fn stats() -> (usize, usize) {
    (RECLAIMED_VMOS.load(Ordering::Relaxed), RECLAIMED_BYTES.load(Ordering::Relaxed))
}
