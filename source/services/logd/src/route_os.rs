// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: logd server-endpoint routing (split out of `os_lite.rs` under
//! the structure ratchet, TASK-0049C). Resolves logd's own server slots via
//! the init responder with bounded nonce-correlated retries; the serve loop
//! falls back to the deterministic slots 3/4 the declarative arm provisions.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU ladder (`logd: ready`).
//! ADR: docs/adr/0017-service-architecture.md

use nexus_ipc::KernelServer;

/// ONE route ask on the control channel (RFC-0093 §1: never re-ask — the old bespoke
/// 64-iteration loop re-sent the same query per iteration and filled init's control
/// queue, so the `@ready` announce found no room, 2026-09-09). Short budget: init's
/// responder answers only after orchestration, the deterministic slot fallback stays.
pub(crate) fn route_logd_blocking() -> Option<KernelServer> {
    match nexus_ipc::budget::route_with_nonce_budgeted(
        b"logd",
        nexus_service_topology::CTRL_SLOTS.send,
        nexus_service_topology::CTRL_SLOTS.recv,
        core::time::Duration::from_millis(50),
        nexus_ipc::budget::NonceMismatchBudget::new(8),
    ) {
        nexus_ipc::budget::RouteRetryOutcome::Success { send_slot, recv_slot } => {
            KernelServer::new_with_slots(recv_slot, send_slot).ok()
        }
        _ => None,
    }
}

/// logd's server on the slots init pins for it (TASK-0324 P4f-1a) — the fallback when the
/// route ask above runs out of budget. It used to be a literal `new_with_slots(3, 4)`.
pub(crate) fn declared_server() -> Option<KernelServer> {
    let slots = nexus_service_topology::slots::logd::SERVER;
    KernelServer::new_with_slots(slots.recv, slots.send).ok()
}
