// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]
#![allow(unexpected_cfgs)]
#![cfg_attr(
    all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std
)]

//! CONTEXT: updated daemon – system-set staging and A/B boot control
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable (v1.0)
//! TEST_COVERAGE: QEMU markers only (no host E2E yet)
//! ADR: docs/adr/0024-updates-ab-packaging-architecture.md

#[cfg(all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
extern crate alloc;

/// Bootctl persistence codec (Integrity envelopes, TASK-0025) — cfg-free so
/// the host suite proves the same bytes the OS path writes.
#[cfg(any(feature = "std", feature = "os-lite"))]
pub mod bootctl_state;

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod os_lite;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub use os_lite::*;

#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
mod std_server;
#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
pub use std_server::*;
