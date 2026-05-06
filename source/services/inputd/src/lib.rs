// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `inputd` service crate for bounded merge/config/route logic in TASK-0253.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p inputd -- --nocapture`
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

#![cfg_attr(all(nexus_env = "os", target_os = "none"), no_std)]
#![forbid(unsafe_code)]

mod config;
mod error;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
mod os_lite;
mod route;
mod service;
mod types;

pub use config::{InitialPointerPosition, InputdConfig, QueueCapacity};
pub use error::InputdError;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
pub use os_lite::service_main_loop;
pub use route::RouteTarget;
pub use service::InputdService;
pub use types::{ImeHook, InputDispatch};

#[cfg(not(all(nexus_env = "os", target_os = "none")))]
pub fn run() {
    println!("inputd: ready");
}
