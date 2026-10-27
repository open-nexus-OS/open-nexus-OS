// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The lifetime rule of a VMO (RFC-0100 §1, TASK-0333 6a) as a pure predicate: an
//! object is freed when no capability names it and no address space maps it. NOT target-gated
//! so its reject matrix runs on the host; `mm::reclaim` applies it at a reap and at the last
//! `cap_close` of a VMO capability.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host (`tests` below); QEMU `KSELFTEST: vm reclaim (`, `SELFTEST: rss loop ok (`
//! RFC: docs/rfcs/RFC-0100-memory-object-model-v2-commit-on-touch-unified-gpu-memory-pressure.md

/// Whether an object `names` capabilities name and that is (`mapped`) or is not mapped by any
/// address space may be freed.
#[must_use]
pub fn should_reclaim(names: usize, mapped: bool) -> bool {
    names == 0 && !mapped
}

/// Why an object may not be unlocked for purging (RFC-0100 §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PurgeRefusal {
    /// Not a lazy object: an eager or fixed one has no pages to drop one by one.
    Kind,
    /// A device was told its runs (`vmo_runs`) — a GPU binding is that same fact: the device
    /// may read the frames at any time, so the kernel never takes them.
    DeviceBound,
    /// An address space maps it: a purge would pull frames from under a mapping.
    Mapped,
}

/// Whether an object may be unlocked for purging: lazy, never named to a device, mapped nowhere.
pub fn purge_unlock_allowed(
    lazy: bool,
    device_named: bool,
    mapped: bool,
) -> Result<(), PurgeRefusal> {
    if !lazy {
        return Err(PurgeRefusal::Kind);
    }
    if device_named {
        return Err(PurgeRefusal::DeviceBound);
    }
    if mapped {
        return Err(PurgeRefusal::Mapped);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{purge_unlock_allowed, should_reclaim, PurgeRefusal};

    #[test]
    fn an_object_nobody_names_or_maps_is_reclaimed() {
        assert!(should_reclaim(0, false));
    }

    /// Another capability — an owner's, a read-only alias's — keeps the object alive.
    #[test]
    fn test_reject_reclaim_shared_vmo() {
        assert!(!should_reclaim(1, false));
        assert!(!should_reclaim(3, false));
    }

    /// A live mapping keeps the object alive even when no capability names it: freeing under
    /// a mapping would hand recycled frames to the mapper (RFC-0085).
    #[test]
    fn test_reject_reclaim_mapped_vmo() {
        assert!(!should_reclaim(0, true));
        assert!(!should_reclaim(1, true));
    }

    /// A lazy object nobody maps and no device was told of may be unlocked for purging.
    #[test]
    fn a_lazy_unmapped_unbound_object_may_be_purgeable() {
        assert_eq!(purge_unlock_allowed(true, false, false), Ok(()));
    }

    /// A device holds the frames' addresses (`vmo_runs`, a GPU binding): the kernel never takes
    /// them from under it (RFC-0100 §4, ADR-0085).
    #[test]
    fn test_reject_purge_of_bound_vmo() {
        assert_eq!(purge_unlock_allowed(true, true, false), Err(PurgeRefusal::DeviceBound));
        assert_eq!(
            purge_unlock_allowed(true, true, true),
            Err(PurgeRefusal::DeviceBound),
            "the device's hold is refused before the mapping"
        );
    }

    /// A mapped object: a purge would hand recycled frames to the mapper.
    #[test]
    fn test_reject_purge_of_mapped_vmo() {
        assert_eq!(purge_unlock_allowed(true, false, true), Err(PurgeRefusal::Mapped));
    }

    /// An eager or fixed object has no pages to drop one by one.
    #[test]
    fn test_reject_purge_of_eager_vmo() {
        assert_eq!(purge_unlock_allowed(false, false, false), Err(PurgeRefusal::Kind));
    }
}
