// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: What the kernel hands its direct userspace child — init — in fixed capability slots
//! at spawn, besides the bootstrap endpoint: the endpoint factory (RFC-0005 Phase 2), the device
//! tree read-only (RFC-0098 C3), the console ring read-only (RFC-0107 Phase 2) and the bootfs
//! (ADR-0081: its table page read-only, every file a sealed object). Split out of
//! `task/mod.rs` by the structure ratchet. The slot numbers are `nexus-abi`'s contract
//! (`INIT_ENDPOINT_FACTORY_SLOT`, `INIT_DEVICE_TREE_SLOT`, `INIT_CONSOLE_RING_SLOT`,
//! `INIT_BOOTFS_TOC_SLOT`, `INIT_BOOTFS_FILES_SLOT`).
//! OWNERS: @kernel-team @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (the slots are the ABI's)
//! TEST_COVERAGE: every QEMU boot — init discovers devices from slot 2 and blkd keeps the ring
//!   from the alias init pins out of slot 3 (`init: console ring grant ok svc=blkd`)

use crate::cap::{CapTable, CapabilityKind, Rights};

/// The bootstrap task (PID 0) carries the endpoint factory here.
const FACTORY_PARENT_SLOT: usize = 2;
/// init receives a derived copy of the factory here — no PID gating, no cap_transfer timed
/// from outside.
const FACTORY_CHILD_SLOT: usize = 1;
/// init receives the device tree, read-only, here (`boot_fdt::init_alias`).
const DEVICE_TREE_CHILD_SLOT: usize = 2;
/// init receives the console ring, read-only, here (`hal::console_ring::init_alias`), and pins
/// it to the block owner alone.
const CONSOLE_RING_CHILD_SLOT: usize = 3;
/// init receives the bootfs table page, read-only, here (`bootfs::toc_alias`, ADR-0081).
const BOOTFS_TOC_CHILD_SLOT: usize = 4;
/// init receives bootfs file `i`, a sealed read-only object, at this slot plus `i`.
const BOOTFS_FILES_CHILD_SLOT: usize = 32;

/// Fills init's fixed slots from the bootstrap task's table and the kernel's aliases. A missing
/// piece leaves its slot empty: init says so, loudly, never with a guess.
pub(super) fn inject(parent_caps: &CapTable, child_caps: &mut CapTable) {
    if let Ok(factory_cap) = parent_caps.get(FACTORY_PARENT_SLOT) {
        if factory_cap.kind == CapabilityKind::EndpointFactory
            && factory_cap.rights.contains(Rights::MANAGE)
        {
            let _ = child_caps.set(FACTORY_CHILD_SLOT, factory_cap);
        }
    }
    if let Some(alias) = crate::boot_fdt::init_alias() {
        let _ = child_caps.set(DEVICE_TREE_CHILD_SLOT, alias);
    }
    if let Some(ring) = crate::hal::console_ring::init_alias() {
        let _ = child_caps.set(CONSOLE_RING_CHILD_SLOT, ring);
    }
    // ADR-0081: the bootfs table, read-only, and one sealed object per file — init execs the
    // core services from them and hands the launcher its images.
    if let Some(toc) = crate::bootfs::toc_alias() {
        let _ = child_caps.set(BOOTFS_TOC_CHILD_SLOT, toc);
    }
    for (index, file) in crate::bootfs::file_aliases() {
        let _ = child_caps.set(BOOTFS_FILES_CHILD_SLOT + index, file);
    }
}
