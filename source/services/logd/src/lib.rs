// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]
#![allow(unexpected_cfgs)]
#![cfg_attr(
    all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std
)]

//! CONTEXT: logd daemon – bounded RAM journal for structured logs + minimal query/stats
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Host tests in `source/services/logd/tests/`
//!
//! PUBLIC API:
//!   - `journal`: bounded in-memory ring buffer (drop-oldest)
//!   - `protocol`: v1 byte-frame codec (os-lite authoritative)
//!   - `service_main_loop()`: daemon entry loop (backend-specific)
//! ADR: docs/adr/0017-service-architecture.md

#[cfg(all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
extern crate alloc;

pub mod journal;
pub mod protocol;

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod os_lite;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub use os_lite::*;

#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
mod std_server;
#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
pub use std_server::*;
