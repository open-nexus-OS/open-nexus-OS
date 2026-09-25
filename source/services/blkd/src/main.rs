// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(
    all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"),
    no_std,
    no_main
)]

//! CONTEXT: blkd — the ONE block owner serving partition-scoped block IO over IPC
//! (ADR-0044 end state, TASK-0315; ADR-0067: one owner on every platform, TASK-0246). The
//! server loop is `os_lite.rs`; the pure partition gate is the library's `gate`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/gate.rs` (host); QEMU marker ladder (scripts/qemu-test.sh)
//! ADR: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md,
//!   docs/adr/0067-one-block-owner-backend-selected-by-fdt.md

mod disk_os;
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
