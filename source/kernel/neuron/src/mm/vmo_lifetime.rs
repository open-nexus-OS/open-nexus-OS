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

#[cfg(test)]
mod tests {
    use super::should_reclaim;

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
}
