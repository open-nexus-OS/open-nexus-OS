// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: selftest-client crate root — dispatch-only after Cut P3-02.
//! OS build (`os-lite` cfg) -> `os_lite::run()`; host build -> `host_lite::run()`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: see `os_lite/mod.rs` (QEMU ladder) + `host_lite.rs` (cargo host slice).
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

#![cfg_attr(
    all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"),
    no_std,
    no_main
)]
#![cfg_attr(
    not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite")),
    forbid(unsafe_code)
)]

#[cfg(not(all(
    nexus_env = "os",
    target_arch = "riscv64",
    target_os = "none",
    feature = "os-lite"
)))]
mod host_lite;
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
mod markers;
// P4-04: marker-literal SSOT shared between host and OS pfads. Pure
// `const &str` table, no platform deps; safe to compile in any target.
mod markers_generated;
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
mod os_lite;
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
mod runtime_mode;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
nexus_service_entry::declare_entry!(os_entry);
#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
fn os_entry() -> core::result::Result<(), ()> {
    // NOTE: do NOT self-lower to Idle here. It is a self-defeating chicken-and-egg:
    // `os_lite::run()` immediately raises this task to Interactive for the proof
    // ladder (see os_lite/mod.rs), but on the strict-priority scheduler an Idle
    // task never gets CPU while the Normal display busy-spinners keep the Normal
    // queue non-empty — so it would never REACH `run()` to raise itself, and the
    // whole ladder (incl. the OTA phase emitting `bundlemgrd: slot a active`)
    // silently never runs. The kernel spawns selftest-client Normal (see
    // `initial_qos_for`) so it is scheduled; `run()` then owns its QoS.
    os_lite::run()
}

#[cfg(not(all(
    nexus_env = "os",
    target_arch = "riscv64",
    target_os = "none",
    feature = "os-lite"
)))]
fn main() {
    if let Err(err) = host_lite::run() {
        eprintln!("selftest: {err:?}");
    }
}
