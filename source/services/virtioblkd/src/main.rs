// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(
    all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"),
    no_std,
    no_main
)]

//! CONTEXT: virtioblkd — the single virtio-blk owner serving
//! partition-scoped block IO over IPC (ADR-0044 end state, TASK-0315).
//! The v0 proof stub (map window, print marker, park) is replaced by the
//! real server in `os_lite.rs`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (scripts/qemu-test.sh)
//! ADR: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md

mod os_lite;
mod route_os;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
nexus_service_entry::declare_entry!(crate::os_lite::os_entry);

#[cfg(not(all(
    nexus_env = "os",
    target_arch = "riscv64",
    target_os = "none",
    feature = "os-lite"
)))]
fn main() {}
