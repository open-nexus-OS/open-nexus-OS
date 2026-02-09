// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(test)]
//! CONTEXT: Property-based tests for capability table
//! OWNERS: @kernel-cap-team
//! NOTE: Tests only; no kernel logic. Ensures Rights derivation and slot ops are sound.
//!
//! TEST_SCOPE:
//!   - Capability slot set/get roundtrip behavior
//!   - Rights derivation: subset-only (no escalation)
//!   - Derivation rejects superset rights requests deterministically
//!
//! TEST_SCENARIOS:
//!   - set_and_get_roundtrip(): set then get returns the same capability
//!   - derive_preserves_kind_and_masks(): derived cap preserves kind and intersects rights
//!   - derive_rejects_superset(): requesting rights outside parent returns PermissionDenied

use super::{CapError, CapTable, Capability, CapabilityKind, Rights};
use proptest::prelude::*;

fn arb_rights() -> impl Strategy<Value = Rights> {
    (0u8..16).prop_map(|bits| Rights::from_bits_truncate(bits as u32))
}

fn arb_capability_kind() -> impl Strategy<Value = CapabilityKind> {
    prop_oneof![
        any::<u32>().prop_map(CapabilityKind::Endpoint),
        (any::<usize>(), 1usize..=0x10_0000usize).prop_map(|(base, len)| CapabilityKind::Vmo {
            base: base & !0xfff,
            len: (len & !0xfff).max(0x1000),
        }),
        (any::<usize>(), 1usize..=0x10_0000usize).prop_map(|(base, len)| {
            CapabilityKind::DeviceMmio { base: base & !0xfff, len: (len & !0xfff).max(0x1000) }
        }),
        any::<u32>().prop_map(CapabilityKind::Irq),
    ]
}

proptest! {
    #[test]
    fn set_and_get_roundtrip(slot in 0usize..32, kind in arb_capability_kind(), rights in arb_rights()) {
        let mut table = CapTable::new();
        table.set(slot, Capability { kind, rights }).unwrap();
        prop_assert_eq!(table.get(slot).unwrap(), Capability { kind, rights });
    }

    #[test]
    fn derive_preserves_kind_and_masks(kind in arb_capability_kind(), base_rights in arb_rights(), mask in arb_rights()) {
        let mut table = CapTable::new();
        table.set(0, Capability { kind, rights: base_rights }).unwrap();
        let requested = Rights::from_bits_truncate((base_rights & mask).bits());
        let derived = table.derive(0, requested).unwrap();
        prop_assert_eq!(derived.kind, kind);
        prop_assert_eq!(derived.rights, requested);
    }

    #[test]
    fn derive_rejects_superset(kind in arb_capability_kind(), base_rights in arb_rights(), extra in 1u8..16) {
        let mut table = CapTable::new();
        table.set(1, Capability { kind, rights: base_rights }).unwrap();
        let extra_rights = Rights::from_bits_truncate(extra as u32);
        prop_assume!(!base_rights.contains(extra_rights));
        prop_assert_eq!(table.derive(1, base_rights | extra_rights), Err(CapError::PermissionDenied));
    }
}
