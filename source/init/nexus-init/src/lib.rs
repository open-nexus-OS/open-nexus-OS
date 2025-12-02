#![deny(clippy::all, missing_docs)]
#![allow(unexpected_cfgs)]
#![cfg_attr(feature = "os-payload", no_std)]

//! CONTEXT: Init process selecting Host std backend or OS-lite cooperative bootstrap
//! OWNERS: @init-team @runtime
//! PUBLIC API: touch_schemas(), service_main_loop(), ReadyNotifier
//! DEPENDS_ON: execd, keystored, policyd, samgrd, bundlemgrd, packagefsd, vfsd (os-lite)
//! FEATURES: cfg(all(nexus_env = "os", feature = "os-lite")) selects os_lite; otherwise std_server
//! INVARIANTS: Preserve UART markers; Host path byte-compatible
//! ADR: docs/adr/0017-service-architecture.md
//!
//! The library exposes two backends selected via `nexus_env` and the
//! `os-lite` feature. Host builds keep the original std runtime while the OS
//! variant uses a cooperative bootstrap stub that will gain capabilities in
//! later stages of the migration.

/// Shared os-lite loader facade used by the deprecated init-lite wrapper.
#[cfg(feature = "os-payload")]
pub mod os_payload;

#[cfg(all(
    not(feature = "os-payload"),
    all(nexus_env = "os", feature = "os-lite")
))]
mod os_lite;
#[cfg(all(
    not(feature = "os-payload"),
    all(nexus_env = "os", feature = "os-lite")
))]
pub use os_lite::*;

#[cfg(all(
    not(feature = "os-payload"),
    not(all(nexus_env = "os", feature = "os-lite"))
))]
mod std_server;
#[cfg(all(
    not(feature = "os-payload"),
    not(all(nexus_env = "os", feature = "os-lite"))
))]
pub use std_server::*;
