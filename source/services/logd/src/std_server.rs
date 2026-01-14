// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

//! CONTEXT: logd host backend (std) – placeholder until Cap'n Proto server wiring lands
//! OWNERS: @runtime
//! STATUS: Placeholder
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//! ADR: docs/adr/0017-service-architecture.md

use std::fmt;

/// Result alias surfaced by the logd host backend.
pub type LiteResult<T> = core::result::Result<T, ServerError>;

/// Ready notifier invoked once logd finishes initialization.
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

/// Errors surfaced by the host backend.
#[derive(Debug)]
pub enum ServerError {
    /// Placeholder error until the backend is implemented.
    Unsupported,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "logd unsupported"),
        }
    }
}

impl std::error::Error for ServerError {}

/// Touches schemas for parity with other services (no-op for now).
pub fn touch_schemas() {}

/// Placeholder host loop.
pub fn service_main_loop(notifier: ReadyNotifier) -> LiteResult<()> {
    notifier.notify();
    Err(ServerError::Unsupported)
}
