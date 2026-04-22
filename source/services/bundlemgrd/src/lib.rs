#![forbid(unsafe_code)]
#![allow(unexpected_cfgs)]
#![cfg_attr(
    all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std
)]

//! CONTEXT: BundleMgr daemon – bundle install/query/payload via Cap'n Proto IPC
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable (v1.0)
//! TEST_COVERAGE: 3 E2E tests + 11 host unit tests
//!   - E2E: install/query roundtrip, get_payload roundtrip, invalid signature rejection (`tests/e2e/bundlemgrd_roundtrip.rs`)
//!   - Unit: supply-chain allow/deny/tamper/oversize reject paths (`source/services/bundlemgrd/src/std_server.rs`)
//!
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
