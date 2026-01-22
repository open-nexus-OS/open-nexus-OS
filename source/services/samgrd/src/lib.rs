#![forbid(unsafe_code)]
#![allow(unexpected_cfgs)]
#![cfg_attr(
    all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std
)]

//! CONTEXT: SAMgr daemon – service discovery/registry/heartbeat via Cap'n Proto IPC
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable (v1.0)
//! TEST_COVERAGE: 2 E2E tests (tests/e2e/samgrd_roundtrip.rs)
//!   - register/resolve roundtrip
//!   - unknown service returns not-found
//!
//! PUBLIC API: service_main_loop(), run_with_transport(), loopback_transport()
//! DEPENDS_ON: nexus_ipc, nexus_idl_runtime (capnp), samgr registry lib
//! ADR: docs/adr/0017-service-architecture.md

#[cfg(all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
extern crate alloc;

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod os_lite;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub use os_lite::*;

#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
mod std_server;
#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
pub use std_server::*;
