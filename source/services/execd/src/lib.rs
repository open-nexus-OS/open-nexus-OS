#![forbid(unsafe_code)]
#![allow(unexpected_cfgs)]
#![cfg_attr(
    all(
        feature = "os-lite",
        nexus_env = "os",
        target_arch = "riscv64",
        target_os = "none"
    ),
    no_std
)]

//! CONTEXT: execd daemon – payload executor and service spawner
//! OWNERS: @services-team
//! PUBLIC API: service_main_loop(), exec helpers
//! DEPENDS_ON: nexus_ipc, nexus_loader (host), nexus_abi (os-lite stubs)
//! ADR: docs/adr/0017-service-architecture.md

#[cfg(all(
    feature = "os-lite",
    nexus_env = "os",
    target_arch = "riscv64",
    target_os = "none"
))]
extern crate alloc;

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod os_lite;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub use os_lite::*;

#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
mod std_server;
#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
pub use std_server::*;
