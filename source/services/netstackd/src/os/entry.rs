// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Netstackd OS entry wiring helpers for modular daemon structure
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered transitively by netstackd host tests and QEMU proofs
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[inline]
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
pub(crate) fn emit_ready_marker() {
    // This marker means the service process reached entry and is alive.
    // Network readiness is proven later by bootstrap markers (iface/ping/dns/tcp).
    let _ = nexus_abi::debug_println("netstackd: ready");
}

#[inline]
#[cfg(not(all(
    nexus_env = "os",
    target_arch = "riscv64",
    target_os = "none",
    feature = "os-lite"
)))]
pub(crate) fn emit_ready_marker() {}
