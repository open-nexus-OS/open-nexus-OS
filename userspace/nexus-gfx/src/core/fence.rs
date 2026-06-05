// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

/// Timeline fence for GPU/backend synchronization.
/// A fence transitions from unsignaled → signaled exactly once.
/// Dropping a signaled fence is safe; dropping an unsignaled fence
/// means the associated work may not be visible yet.
#[must_use = "fence must be awaited to guarantee work completion"]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fence {
    signaled: bool,
}

impl Fence {
    /// Create an already-signaled fence (for synchronous backends).
    #[must_use]
    pub const fn new_signaled() -> Self {
        Self { signaled: true }
    }

    /// Create an unsignaled fence. The backend must call `signal()` when work completes.
    #[must_use]
    pub const fn new_unsignaled() -> Self {
        Self { signaled: false }
    }

    /// Block until the fence is signaled or the timeout expires.
    /// Returns `true` if the fence is signaled.
    /// On CPU backends this returns immediately (work is synchronous).
    #[must_use]
    pub fn wait(&self, _timeout_ns: u64) -> bool {
        self.signaled
    }

    /// Check whether the fence is signaled without blocking.
    #[must_use]
    pub const fn signaled(self) -> bool {
        self.signaled
    }

    /// Mark the fence as signaled. Called by the backend after work completion.
    pub(crate) fn signal(&mut self) {
        self.signaled = true;
    }
}
