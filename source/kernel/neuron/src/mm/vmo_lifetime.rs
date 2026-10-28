// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The lifetime rule of a VMO (RFC-0100 §1, TASK-0333 6a) as a pure predicate: an
//! object is freed when no capability names it and no address space maps it. NOT target-gated
//! so its reject matrix runs on the host; `mm::reclaim` applies it at a reap and at the last
//! `cap_close` of a VMO capability. Beside it the two rules of what the kernel may take from an
//! object while it lives: the purge's unlock (§4, 6e-3) and the squeeze's (§5, 6f-1, ADR-0087 —
//! only a frozen child's private lazy object).
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host (`tests` below); QEMU `KSELFTEST: vm reclaim (`, `SELFTEST: rss loop ok (`,
//!   `KSELFTEST: task squeeze ok (`
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

/// What the squeeze rule weighs of one object of the child being squeezed (RFC-0100 §5,
/// ADR-0087).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SqueezeFacts {
    /// A lazy object (only its pages are one word each).
    pub lazy: bool,
    /// Sealed (ADR-0080): an image — clean, shared, never compressed.
    pub sealed: bool,
    /// A device was told its runs (a GPU binding is the same fact).
    pub device_named: bool,
    /// Unlocked for purging (RFC-0100 §4): the kernel's to drop, not to keep.
    pub unlocked: bool,
    /// Regions of the child's space that map it.
    pub regions_here: usize,
    /// Another address space maps it.
    pub mapped_elsewhere: bool,
    /// A capability outside the child's process names it.
    pub named_elsewhere: bool,
}

/// Why an object of the child is not squeezed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SqueezeRefusal {
    /// Not a lazy object.
    Kind,
    /// A sealed image.
    Sealed,
    /// A device may read or write its frames at any time.
    DeviceBound,
    /// Unlocked for purging.
    Unlocked,
    /// Mapped twice in the child's space, or by another space: a squeeze would leave a PTE to a
    /// frame that went back to the pool.
    Shared,
    /// Another process can reach it through a capability: its memory is not the child's alone.
    Named,
}

/// Whether a squeeze may take an object's pages: the frozen child's alone — lazy, not sealed,
/// never told to a device, not unlocked, mapped by exactly one region of the child's space and
/// nowhere else, named by no capability outside the child's process.
pub fn squeeze_allowed(facts: &SqueezeFacts) -> Result<(), SqueezeRefusal> {
    if !facts.lazy {
        return Err(SqueezeRefusal::Kind);
    }
    if facts.sealed {
        return Err(SqueezeRefusal::Sealed);
    }
    if facts.device_named {
        return Err(SqueezeRefusal::DeviceBound);
    }
    if facts.unlocked {
        return Err(SqueezeRefusal::Unlocked);
    }
    if facts.regions_here != 1 || facts.mapped_elsewhere {
        return Err(SqueezeRefusal::Shared);
    }
    if facts.named_elsewhere {
        return Err(SqueezeRefusal::Named);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        purge_unlock_allowed, should_reclaim, squeeze_allowed, PurgeRefusal, SqueezeFacts,
        SqueezeRefusal,
    };

    /// A frozen app's heap: lazy, its own, mapped once.
    const HEAP: SqueezeFacts = SqueezeFacts {
        lazy: true,
        sealed: false,
        device_named: false,
        unlocked: false,
        regions_here: 1,
        mapped_elsewhere: false,
        named_elsewhere: false,
    };

    #[test]
    fn a_frozen_childs_private_lazy_object_is_squeezed() {
        assert_eq!(squeeze_allowed(&HEAP), Ok(()));
    }

    /// The shared image and every eager object stay as they are.
    #[test]
    fn test_reject_squeeze_of_an_eager_or_sealed_object() {
        assert_eq!(
            squeeze_allowed(&SqueezeFacts { lazy: false, ..HEAP }),
            Err(SqueezeRefusal::Kind)
        );
        assert_eq!(
            squeeze_allowed(&SqueezeFacts { sealed: true, ..HEAP }),
            Err(SqueezeRefusal::Sealed)
        );
    }

    /// A device holds the frames' addresses: the kernel never takes them from under it.
    #[test]
    fn test_reject_squeeze_of_a_device_bound_object() {
        assert_eq!(
            squeeze_allowed(&SqueezeFacts { device_named: true, ..HEAP }),
            Err(SqueezeRefusal::DeviceBound)
        );
        assert_eq!(
            squeeze_allowed(&SqueezeFacts { unlocked: true, ..HEAP }),
            Err(SqueezeRefusal::Unlocked)
        );
    }

    /// Mapped twice in the child's space or by anyone else: the squeeze clears one PTE, the other
    /// would still reach a frame the pool got back.
    #[test]
    fn test_reject_squeeze_of_a_shared_object() {
        let twice = SqueezeFacts { regions_here: 2, ..HEAP };
        assert_eq!(squeeze_allowed(&twice), Err(SqueezeRefusal::Shared));
        let elsewhere = SqueezeFacts { mapped_elsewhere: true, ..HEAP };
        assert_eq!(squeeze_allowed(&elsewhere), Err(SqueezeRefusal::Shared));
        let unmapped = SqueezeFacts { regions_here: 0, ..HEAP };
        assert_eq!(squeeze_allowed(&unmapped), Err(SqueezeRefusal::Shared));
    }

    /// A capability outside the child's process reaches the object: not the child's alone.
    #[test]
    fn test_reject_squeeze_of_an_object_another_process_names() {
        assert_eq!(
            squeeze_allowed(&SqueezeFacts { named_elsewhere: true, ..HEAP }),
            Err(SqueezeRefusal::Named)
        );
    }

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
