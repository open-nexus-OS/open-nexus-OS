// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

// deny (not forbid): the ONE bounded unsafe surface is `mapmem.rs` — the
// slice view over updated's own live vm_map window (TASK-0179 staging
// source). Everything else stays unsafe-free.
#![deny(unsafe_code)]
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
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod bootctl_client;

/// Verifier-verdict policy (RFC-0089 §4) — cfg-free pure decision logic so
/// the host suite proves the exact mapping the OS verify path executes.
pub mod verify_policy;

/// `updates.manage` gate (TASK-0140) — cfg-free pure decision logic so the
/// host suite proves the exact op classification the OS loop executes.
pub mod manage_gate;

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod policy_client;

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod apply_os;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod delta_os;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod mapmem;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod os_lite;
mod stage_os;
/// TASK-0321 P3: the OS half of bundle-set staging (system volume).
mod volume_os;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub use os_lite::*;

#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
mod std_server;
#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
pub use std_server::*;
