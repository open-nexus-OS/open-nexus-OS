#![deny(clippy::all, missing_docs)]
#![allow(unexpected_cfgs)]
// `os-payload` and `os-lite` are used by OS builds. They must remain compatible
// with `--no-default-features` (no std), while still allowing the regular host/std backend to build
// when `std-server` is enabled (including in `--all-features` tool runs).
#![cfg_attr(
    all(
        any(feature = "os-payload", feature = "os-lite"),
        not(feature = "std-server"),
        nexus_env = "os"
    ),
    no_std
)]

//! CONTEXT: Init process selecting Host std backend or OS-lite cooperative bootstrap
//! OWNERS: @runtime
//! STATUS: Functional (host backend std_server; OS deprecated os_lite)
//! API_STABILITY: Stable (v1.0)
//! TEST_COVERAGE: 2 unit tests (std_server) + QEMU markers
//!
//! PUBLIC API: touch_schemas(), service_main_loop(), ReadyNotifier
//! DEPENDS_ON: execd, keystored, policyd, samgrd, bundlemgrd, updated, packagefsd, vfsd
//! INVARIANTS: Preserve UART markers; Host path byte-compatible
//! ADR: docs/adr/0017-service-architecture.md
//!
//! The library exposes two backends selected via `nexus_env` and the
//! `os-lite` feature. Host builds keep the original std runtime while the OS
//! variant uses a cooperative bootstrap stub that will gain capabilities in
//! later stages of the migration.

/// Deterministic capability slot assignment map for early boot validation.
#[cfg(all(feature = "os-payload", nexus_env = "os"))]
pub mod slot_map;

/// Shared os-lite loader facade used by the deprecated init-lite wrapper.
#[cfg(all(feature = "os-payload", nexus_env = "os"))]
pub mod os_payload;

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
mod os_lite;
#[cfg(all(feature = "os-lite", nexus_env = "os"))]
pub use os_lite::*;

#[cfg(all(feature = "std-server", not(all(nexus_env = "os", feature = "os-lite"))))]
mod std_server;
#[cfg(all(feature = "std-server", not(all(nexus_env = "os", feature = "os-lite"))))]
pub use std_server::*;

// Fallback stubs for feature combinations that intentionally omit an init backend
// (e.g. `--no-default-features --features os-payload` used by `init-lite`).
//
// These keep tooling/type-checking stable without pretending that init actually booted services.
#[cfg(not(any(
    all(feature = "os-lite", nexus_env = "os"),
    all(feature = "std-server", not(all(nexus_env = "os", feature = "os-lite")))
)))]
mod stub {
    use core::fmt;

    /// Errors produced by the stub backend.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum InitError {
        /// No init backend is enabled for this build.
        Unsupported,
    }

    impl fmt::Display for InitError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Unsupported => write!(f, "init backend unsupported for this configuration"),
            }
        }
    }

    /// Callback invoked once the init backend reaches a terminal state.
    pub struct ReadyNotifier<F: FnOnce() + Send>(F);

    impl<F: FnOnce() + Send> ReadyNotifier<F> {
        /// Create a new notifier from the supplied closure.
        pub fn new(func: F) -> Self {
            Self(func)
        }

        /// Execute the wrapped callback.
        pub fn notify(self) {
            (self.0)();
        }
    }

    /// Schema warmer placeholder retained for API parity.
    pub fn touch_schemas() {}

    /// Stubbed init loop: always returns `Unsupported`.
    pub fn service_main_loop<F>(_notifier: ReadyNotifier<F>) -> Result<(), InitError>
    where
        F: FnOnce() + Send,
    {
        Err(InitError::Unsupported)
    }
}

#[cfg(not(any(
    all(feature = "os-lite", nexus_env = "os"),
    all(feature = "std-server", not(all(nexus_env = "os", feature = "os-lite")))
)))]
pub use stub::*;
