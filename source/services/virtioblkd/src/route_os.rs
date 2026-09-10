// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]

//! CONTEXT: virtioblkd slot resolution over the init responder (the logd
//! `route_os` pattern): the service's OWN server slots by name, and its
//! @reply inbox — which here doubles as the driver's IRQ notify endpoint
//! (this service makes no outbound calls). Bounded nonce-correlated
//! retries; the caller falls back to the deterministic slots 3/4.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU ladder (`virtioblkd: ready`).
//! ADR: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md

use nexus_ipc::budget::{self, NonceMismatchBudget, RouteRetryOutcome};
use nexus_ipc::KernelServer;

/// ONE route ask on the control channel (RFC-0093 §1: never re-ask — the old bespoke
/// 64-iteration loop re-sent the same query per iteration and filled init's control
/// queue, so the `@ready` announce found no room, 2026-09-09). init answers only once
/// its responder runs (after orchestration), so a short budget keeps the deterministic
/// slot fallback immediate for this wave-0 driver.
fn route_blocking(name: &[u8]) -> Option<(u32, u32)> {
    const CTRL_SEND_SLOT: u32 = 1;
    const CTRL_RECV_SLOT: u32 = 2;
    match budget::route_with_nonce_budgeted(
        name,
        CTRL_SEND_SLOT,
        CTRL_RECV_SLOT,
        core::time::Duration::from_millis(50),
        NonceMismatchBudget::new(8),
    ) {
        RouteRetryOutcome::Success { send_slot, recv_slot } => Some((send_slot, recv_slot)),
        _ => None,
    }
}

pub(crate) fn route_virtioblkd_blocking() -> Option<KernelServer> {
    let (send_slot, recv_slot) = route_blocking(b"virtioblkd")?;
    KernelServer::new_with_slots(recv_slot, send_slot).ok()
}
