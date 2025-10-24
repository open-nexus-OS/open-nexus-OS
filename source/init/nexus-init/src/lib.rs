#![deny(clippy::all, missing_docs)]
#![allow(unexpected_cfgs)]

//! CONTEXT: Init process selecting Host std backend or OS-lite cooperative bootstrap
//! OWNERS: @init-team @runtime
//! PUBLIC API: touch_schemas(), service_main_loop(), ReadyNotifier
//! DEPENDS_ON: execd, keystored, policyd, samgrd, bundlemgrd, packagefsd, vfsd (os-lite)
//! FEATURES: cfg(all(nexus_env = "os", feature = "os-lite")) selects os_lite; otherwise std_server
//! INVARIANTS: Preserve UART markers; Host path byte-compatible
//! ADR: docs/adr/0001-runtime-roles-and-boundaries.md
//!
//! The library exposes two backends selected via `nexus_env` and the
//! `os-lite` feature. Host builds keep the original std runtime while the OS
//! variant uses a cooperative bootstrap stub that will gain capabilities in
//! later stages of the migration.

#[cfg(all(nexus_env = "os", feature = "os-lite"))]
mod os_lite;
#[cfg(all(nexus_env = "os", feature = "os-lite"))]
pub use os_lite::*;

#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
mod std_server;
#[cfg(not(all(nexus_env = "os", feature = "os-lite")))]
pub use std_server::*;
