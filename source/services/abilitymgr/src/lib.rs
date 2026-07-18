// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Ability Manager daemon — the ability-lifecycle broker — the system app-lifecycle authority.
//! OWNERS: @runtime @ui
//! STATUS: Functional (lifecycle core + OS-lite service loop)
//! API_STABILITY: Unstable (v6b bring-up)
//! TEST_COVERAGE: Host unit tests (lifecycle state machine + wire dispatch) + QEMU `abilitymgr: ready` marker
//!
//! PUBLIC API: `lifecycle::Broker`, `wire::dispatch`, `service_main_loop()` (OS), `execute()` (CLI)
//! DEPENDS_ON: nexus_ipc, nexus_abi (OS only)
//! INVARIANTS:
//!   - Deterministic, bounded lifecycle ordering (Created→Started→Foreground/Background→Suspend/Resume→Stop).
//!   - This service is the ONLY app spawner (RFC-0065 / ADR-0036). Spawn-via-execd + resolve-via-bundlemgrd
//!     are wired in P3 (live launch handoff); P2 ships the broker core + service shell + markers.
//!   - No `unwrap`/`expect` in production paths; no blanket `allow(dead_code)`.
//!
//! ADR: docs/adr/0036-ability-lifecycle-vs-process-vs-registry-service-split.md

#![forbid(unsafe_code)]
#![cfg_attr(
    all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std
)]

extern crate alloc;

pub mod caps;
pub mod handoff;
pub mod lifecycle;
pub mod mediation;
pub mod protocol;
pub mod wire;

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
mod os_lite;
#[cfg(all(feature = "os-lite", nexus_env = "os"))]
pub use os_lite::*;

#[cfg(feature = "std")]
mod std_impl;
#[cfg(feature = "std")]
pub use std_impl::*;
