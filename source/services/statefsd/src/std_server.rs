//! CONTEXT: statefsd host stub (std backend placeholder)
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//! ADR: docs/adr/0023-statefs-persistence-architecture.md

use core::fmt;

/// Result alias surfaced by the std stub.
pub type LiteResult<T> = Result<T, ServerError>;

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

/// Errors surfaced by the std stub.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerError {
    /// Stub implementation only.
    Unsupported,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => write!(f, "statefsd unsupported"),
        }
    }
}

/// Schema warmer placeholder for API parity.
pub fn touch_schemas() {}

/// Host stub service loop.
pub fn service_main_loop(_notifier: ReadyNotifier) -> LiteResult<()> {
    Err(ServerError::Unsupported)
}
