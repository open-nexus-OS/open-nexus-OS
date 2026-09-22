// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: provisioning the SoC glue owner (RFC-0106, TASK-0245B P2): `socd`
//! receives the read-only tree (its plans are read from it) and every provider
//! window the tree lists — granted by compatible, policy-gated
//! (`device.mmio.syscon`), into the slot of the provider's kind
//! (`SYSCON_MMIO_SLOTS`, the index `nexus_soc::ProviderKind` fixes and socd maps).
//! A tree without providers (QEMU virt) grants nothing; socd then answers every
//! consumer `NotNeeded`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU ladder — `socd: ready (…)` in every profile; the board
//!   lane proves the six grants
//! RFC: docs/rfcs/RFC-0106-soc-glue-one-owner-clocks-resets-power-pinmux.md

use crate::bootstrap::core_plane::{grant_mmio_with_wait, GrantStats};
use crate::bootstrap::diag::iw;
use crate::os_payload::{debug_write_bytes, Result, Rights};
use crate::service_topology::ServiceId;

/// Pin the tree alias and grant every provider window to `socd`.
pub(crate) fn provision(
    socd_pid: u32,
    grant_stats: &GrantStats,
    pol_route: (u32, u32),
    init_wire: &mut nexus_event::SpanTally,
    init_fold: bool,
) -> Result<()> {
    let pinned = crate::bootstrap::declared_slots::pin_named(
        socd_pid,
        ServiceId::Socd,
        crate::service_topology::NamedSlot::DeviceTree,
        nexus_abi::INIT_DEVICE_TREE_SLOT,
        Rights::MAP,
    );
    if pinned.is_some() && iw(init_wire, init_fold, "init:socd") {
        debug_write_bytes(b"init: device tree grant ok svc=socd\n");
    }
    for (kind, window) in crate::bootstrap::device_tree::providers() {
        grant_mmio_with_wait(
            grant_stats,
            pol_route,
            socd_pid,
            "socd",
            "device.mmio.syscon",
            window,
            nexus_service_topology::SYSCON_MMIO_SLOTS[kind as usize],
        )?;
    }
    Ok(())
}
