// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]
#![allow(unsafe_code)]

//! CONTEXT: The ONE unsafe-bearing updated module (TASK-0179): a read-only
//! byte view over a live `vm_map` window this service created and owns.
//! Bounded on purpose — nothing else in updated touches raw memory.
//! OWNERS: @services-team @security
//! STATUS: Experimental
//! ADR: docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md

/// Read-only view over `va..va+len`.
///
/// SAFETY CONTRACT (upheld by the single caller, `apply_os::MappedSource`):
/// the range is a live mapping returned by `vm_map` on a VMO this service
/// owns; the view is dropped before `vm_unmap`/`vmo_destroy` run (the
/// `MappedSource` Drop order guarantees it); the VMO provider (vfsd) wrote
/// the bytes BEFORE the splice header became visible, and nothing writes
/// them afterwards.
pub(crate) fn ro_slice(va: usize, len: usize) -> &'static [u8] {
    unsafe { core::slice::from_raw_parts(va as *const u8, len) }
}
