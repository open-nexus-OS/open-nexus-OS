// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: statefsd service — journaled key-value store for /state (IPC authority)
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests (service bring-up only)
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

#![forbid(unsafe_code)]
#![cfg_attr(
    all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std
)]

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
extern crate alloc;

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
mod os_lite;
#[cfg(all(feature = "os-lite", nexus_env = "os"))]
pub use os_lite::*;

#[cfg(not(all(feature = "os-lite", nexus_env = "os")))]
mod std_server;
#[cfg(not(all(feature = "os-lite", nexus_env = "os")))]
pub use std_server::*;
