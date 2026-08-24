// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: bootctld host placeholder — the boot-state authority is an
//! OS-path service; host coverage lives in the pure `machine`/`record`
//! modules (tests/record_v2.rs), not behind a mock server.
//! OWNERS: @reliability @runtime
//! STATUS: Placeholder (host builds only)
//! API_STABILITY: Internal
//! TEST_COVERAGE: n/a (see lib header)
//! ADR: docs/adr/0055-bootctld-single-boot-state-authority.md

use core::fmt;

/// Result alias mirrored from the os-lite backend.
pub type LiteResult<T> = Result<T, ServerError>;

/// Errors surfaced by the backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerError {
    /// Host backend intentionally unimplemented.
    Unsupported,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "bootctld unsupported"),
        }
    }
}

/// Ready notifier invoked once the service becomes available.
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    /// Creates a notifier from the provided closure.
    pub fn new<F>(func: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self(Box::new(func))
    }

    /// Signals readiness to the caller.
    pub fn notify(self) {
        (self.0)();
    }
}

/// Schema warmer placeholder for API parity.
pub fn touch_schemas() {}

/// Host stub: the OS path owns the serve loop.
pub fn service_main_loop(_notifier: ReadyNotifier) -> LiteResult<()> {
    Err(ServerError::Unsupported)
}
