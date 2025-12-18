// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Deprecated os-lite init backend (cooperative mailbox)
//! OWNERS: @init-team @runtime
//! STATUS: Deprecated (RFC-0002: kernel exec loader is the only supported OS boot path)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: N/A
//!
//! This module exists only to keep older `nexus-init --features os-lite` builds compiling while
//! we converge on the single supported OS bootstrap path:
//!
//! - `init-lite` (thin wrapper) → `nexus-init::os_payload` → kernel `exec` loader
//!
//! Any attempt to use this backend on OS builds MUST fail loudly (UART markers) and must not
//! pretend to have booted services successfully.
//!
//! ADR: docs/adr/0017-service-architecture.md

use core::fmt;

/// Errors produced by this deprecated backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitError {
    /// The os-lite init backend is retired; use the kernel `exec` path (`init-lite` + `os_payload`).
    Deprecated,
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Deprecated => write!(f, "deprecated os-lite init backend"),
        }
    }
}

/// Callback invoked when the bootstrapper reaches a terminal state.
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

/// No-op for parity with the std backend which warms schema caches.
pub fn touch_schemas() {}

/// Bootstrap exactly once.
///
/// This function is intentionally a hard failure: the cooperative os-lite runtime is not the
/// supported OS boot path anymore.
pub fn bootstrap_once<F>(notifier: ReadyNotifier<F>) -> Result<(), InitError>
where
    F: FnOnce() + Send,
{
    emit_line("init: start (os_lite deprecated)");
    emit_line("init: fail os_lite deprecated; use kernel exec loader");
    notifier.notify();
    Err(InitError::Deprecated)
}

/// Main loop entry used by the `nexus-init` binary on OS builds (when incorrectly configured).
pub fn service_main_loop<F>(notifier: ReadyNotifier<F>) -> Result<(), InitError>
where
    F: FnOnce() + Send,
{
    // Never pretend success; return a clear error to the caller.
    bootstrap_once(notifier)
}

fn emit_line(message: &str) {
    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    {
        let _ = nexus_abi::debug_println(message);
    }

    #[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
    {
        // Best-effort fallback (keeps host diagnostics understandable).
        eprintln!("{message}");
    }
}
