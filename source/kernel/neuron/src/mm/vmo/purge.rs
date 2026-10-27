// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: purgeable VMOs (RFC-0100 §4, TASK-0333 6e-3, ADR-0085). An owner UNLOCKS a lazy
//! object it can rebuild — a cache — and from then on the kernel may drop its committed pages
//! when the pool's level reaches `warn` (`purge_unlocked`, run where the level change is
//! delivered, never inside the pool's lock); the owner LOCKS it before the next use and learns
//! whether the content went (every page is zero again on its next touch). An unlocked object
//! is untouchable: `vm_map`, `vmo_write`, `vmo_read` and `vmo_runs` refuse it, so a purge never
//! pulls a frame from under a mapping or a device. The unlock is refused for an eager or fixed
//! object (no pages to drop one by one), for one a device was told the runs of — a GPU binding
//! is that same fact (`test_reject_purge_of_bound_vmo`) — and for one any space maps; the rule
//! is `crate::vmo_lifetime::purge_unlock_allowed`, host-tested. A child module of `vmo`: it
//! reaches the table directly and keeps `vmo.rs` under the ratchet.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional (TASK-0333 6e-3)
//! API_STABILITY: Internal (the syscall is `SYSCALL_VMO_PURGEABLE`, 68)
//! TEST_COVERAGE: the rule on the host (`vmo_lifetime`); `KSELFTEST: vmo purge ok (…)` on every
//!   QEMU boot — the unlock, the purge at the level change, the lock that says purged, the three
//!   refusals, a locked object kept; `mm_stats` v4's `own_purgeable_bytes` / `purged_pages`
//! INVARIANTS: frames leave the table under its lock and go back to the pool with it dropped

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use super::{frame_pool, Block, VmoId, VmoKind, TABLE};
use crate::vmo_lifetime::{purge_unlock_allowed, PurgeRefusal};

/// Pages dropped by purges since boot (`MmStats::purged_pages`).
static PURGED_PAGES: AtomicU64 = AtomicU64::new(0);

/// Why an unlock was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PurgeError {
    /// No live object has this id.
    NoSuchObject,
    /// The rule refused it.
    Refused(PurgeRefusal),
}

/// Unlock `id`: its pages may be dropped from here on. The caller says whether any space maps
/// it (`vm_ops::any_space_maps_vmo`); the object says the rest. Idempotent — a second unlock
/// keeps what a purge in between recorded.
pub fn unlock(id: VmoId, mapped: bool) -> Result<(), PurgeError> {
    let mut table = TABLE.lock();
    let object = table.get_mut(id).ok_or(PurgeError::NoSuchObject)?;
    purge_unlock_allowed(object.kind == VmoKind::Lazy, object.device_named, mapped)
        .map_err(PurgeError::Refused)?;
    if !object.purgeable {
        object.purgeable = true;
        object.purged = false;
    }
    Ok(())
}

/// Lock `id`: the object is the owner's again. Answers whether a purge took its content since
/// the unlock (`true`: every page is zero on its next touch — rebuild). A locked object answers
/// `false`.
pub fn lock(id: VmoId) -> Result<bool, PurgeError> {
    let mut table = TABLE.lock();
    let object = table.get_mut(id).ok_or(PurgeError::NoSuchObject)?;
    let purged = object.purgeable && object.purged;
    object.purgeable = false;
    object.purged = false;
    Ok(purged)
}

/// Whether the live object `id` is unlocked (untouchable until its lock).
pub fn is_unlocked(id: VmoId) -> bool {
    super::with(id, |o| o.purgeable).unwrap_or(false)
}

/// Drop every unlocked object's committed pages; the frames go back to the pool with the table
/// lock dropped. Returns the pages dropped. Run where a level change is delivered (ADR-0085).
pub fn purge_unlocked() -> usize {
    let frames: Vec<u64> = {
        let mut table = TABLE.lock();
        let mut frames = Vec::new();
        for object in table.objects.iter_mut().flatten().filter(|o| o.purgeable) {
            if let Some(pages) = object.lazy.as_mut() {
                let dropped = pages.purge();
                if !dropped.is_empty() {
                    object.purged = true;
                }
                frames.extend(dropped);
            }
        }
        frames
    };
    for &pa in &frames {
        frame_pool::free(Block { base: pa, order: 0 });
    }
    PURGED_PAGES.fetch_add(frames.len() as u64, Ordering::Relaxed);
    frames.len()
}

/// Pages dropped by purges since boot.
pub fn purged_pages() -> u64 {
    PURGED_PAGES.load(Ordering::Relaxed)
}

/// Committed bytes of the unlocked objects among those `names` says the caller names
/// (`MmStats::own_purgeable_bytes`: what a purge may take from this caller at `warn`).
pub fn unlocked_bytes_named(names: impl Fn(VmoId) -> bool) -> u64 {
    let table = TABLE.lock();
    table
        .objects
        .iter()
        .enumerate()
        .filter_map(|(i, slot)| slot.as_ref().map(|o| (i as VmoId + 1, o)))
        .filter(|(id, o)| o.purgeable && names(*id))
        .map(|(_, o)| o.committed() as u64)
        .sum()
}
