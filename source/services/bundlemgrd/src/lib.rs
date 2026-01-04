#![forbid(unsafe_code)]
#![allow(unexpected_cfgs)]
#![cfg_attr(
    all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std
)]

//! CONTEXT: BundleMgr daemon – bundle install/query/payload via Cap'n Proto IPC
//! OWNERS: @services-team
//! PUBLIC API: service_main_loop(), run_with_transport(), loopback_transport()
//! DEPENDS_ON: nexus_ipc, nexus_idl_runtime (capnp), keystored client, packagefs client
//! INVARIANTS: Separate from SAMgr/Keystore roles; stable readiness prints
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
