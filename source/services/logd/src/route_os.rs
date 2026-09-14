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

/// logd's server on the slots init pins for it (TASK-0324 P4f-1a) — the ONLY source since
/// P7-b (the start-up route ask that preceded it had a clock; asks no longer do).
pub(crate) fn declared_server() -> Option<KernelServer> {
    let slots = nexus_service_topology::slots::logd::SERVER;
    KernelServer::new_with_slots(slots.recv, slots.send).ok()
}
