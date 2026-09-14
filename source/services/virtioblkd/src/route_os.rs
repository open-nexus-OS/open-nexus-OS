// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]

//! CONTEXT: virtioblkd slot resolution over the init responder (the logd
//! `route_os` pattern): the service's OWN server slots by name. Bounded
//! nonce-correlated retries; the caller falls back to its declared server
//! slots (TASK-0324 P4f-1b).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU ladder (`virtioblkd: ready`).
//! ADR: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md

use nexus_ipc::KernelServer;

/// virtioblkd's server on the slots init pins for it (TASK-0324 P4f-1b) — the ONLY source.
/// The start-up route ask that stood here (250 ms budget, then this fallback) is gone with
/// P7-b: asks have no clock, and init was blocked querying bundlemgrd — which waited for
/// this block plane — while this driver waited for init's answer.
pub(crate) fn declared_server() -> Option<KernelServer> {
    let slots = nexus_service_topology::slots::virtioblkd::SERVER;
    KernelServer::new_with_slots(slots.recv, slots.send).ok()
}
