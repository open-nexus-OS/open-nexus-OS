// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `touchd` service crate for bounded touch ingest and deterministic proof fixtures.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p touchd -- --nocapture`
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

#![cfg_attr(all(nexus_env = "os", target_os = "none"), no_std)]
#![forbid(unsafe_code)]

mod error;
mod ingest;
mod service;
mod types;

pub use error::TouchdError;
pub use service::TouchdService;
pub use types::{SyntheticTouchMode, TouchDeviceId};

#[cfg(not(all(nexus_env = "os", target_os = "none")))]
pub fn run() {
    println!("touchd: ready");
}
