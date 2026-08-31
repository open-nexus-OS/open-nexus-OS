// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]
#![allow(unexpected_cfgs)]
#![cfg_attr(
    all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std
)]

//! CONTEXT: bootctld — the single boot-state authority (TASK-0050,
//! ADR-0055, RFC-0087 §4). Owns the ONE persisted boot record (A/B slot
//! machine + boot targets + one-shot next_boot) at
//! `/state/boot/bootctl.v1`; `updated` is a client for
//! stage/switch/health/rollback, init consumes `next_boot` through the
//! wire (never raw statefs). The machine itself RELOCATED from
//! `userspace/updates` (RFC-0012) — proven semantics, not a rewrite.
//! OWNERS: @reliability @runtime
//! STATUS: Experimental (bring-up)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/record_v2.rs (host); QEMU ladder markers
//!   (`bootctld: ready`, `bootctld: target=…`).
//!
//! PUBLIC API:
//!   - `machine`: BootCtrl / Slot / BootTarget state machine
//!   - `record`: v2 codec + v1/legacy migration (envelope discipline)
//!   - `service_main_loop()`: daemon entry (backend-specific)
//!
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md

#[cfg(all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
extern crate alloc;

pub mod bsb;
pub mod machine;
pub mod record;
pub mod wire;

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod bsb_os;
mod emit_os;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod nxra_gate;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod os_lite;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod persist_os;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod reply;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub use os_lite::*;

#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
mod std_server;
#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
pub use std_server::*;
