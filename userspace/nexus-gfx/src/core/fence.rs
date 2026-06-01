// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

/// Timeline fence for GPU synchronization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fence {
    signaled: bool,
}

impl Fence {
    pub fn new_signaled() -> Self {
        Self { signaled: true }
    }
    pub fn new_unsignaled() -> Self {
        Self { signaled: false }
    }

    /// Wait for the fence with an optional timeout (nanoseconds). Returns true if signaled.
    pub fn wait(&self, _timeout_ns: u64) -> bool {
        self.signaled
    }

    /// Check if the fence is signaled without blocking.
    pub fn signaled(&self) -> bool {
        self.signaled
    }

    #[allow(dead_code)]
    pub(crate) fn signal(&mut self) {
        self.signaled = true;
    }
}
