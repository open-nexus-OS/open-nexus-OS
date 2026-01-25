#![forbid(unsafe_code)]
#![cfg_attr(
    all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"),
    no_std
)]

//! CONTEXT: Keystored service – anchor key loading and signature verification
//! OWNERS: @runtime
//! STATUS: Functional (host backend; OS stub placeholder)
//! API_STABILITY: Stable (v1.0)
//! TEST_COVERAGE: 1 unit test + 1 E2E test (tests/e2e/host_roundtrip.rs)
//!
//! PUBLIC API: service_main_loop(), daemon_main(), loopback_transport()
//! DEPENDS_ON: nexus_ipc, nexus_idl_runtime (capnp), keystore lib
//! ADR: docs/adr/0017-service-architecture.md

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
extern crate alloc;

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
mod os_stub;

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
pub use os_stub::*;

// full_impl requires idl-capnp and only builds on host targets
#[cfg(all(
    feature = "idl-capnp",
    not(all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"))
))]
mod full_impl;

#[cfg(all(
    feature = "idl-capnp",
    not(all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"))
))]
pub use full_impl::*;
