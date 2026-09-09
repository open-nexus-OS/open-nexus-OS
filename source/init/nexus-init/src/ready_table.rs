// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: init's readiness table (RFC-0093 §2, ADR-0062). A service is ready when IT says so
//! — `@ready` on its control channel — never when init resumed it. This module is the pure,
//! host-tested bookkeeping behind `init: up <svc>`: a bounded set of ready pids, an announce
//! that rejects unknown senders and double announces, and an exit hook that clears the entry
//! so a restarted instance announces afresh.
//! OWNERS: @runtime
//! STATUS: Production (TASK-0324 P2)
//! API_STABILITY: Internal
//! TEST_COVERAGE: Unit tests below (`cargo test -p nexus-init`)

/// Upper bound on concurrently supervised children (the boot fleet is ~30).
pub const READY_MAX: usize = 64;

/// Why an `@ready` was refused. Every refusal is printed by the responder as
/// `init: FAIL ready … svc=…` — a refused announce never silently disappears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyError {
    /// The announcing pid is not a child init spawned (or its channel is gone).
    UnknownPid,
    /// The pid already announced and has not exited since.
    AlreadyReady,
    /// More live children than [`READY_MAX`] — a bookkeeping bug, never expected.
    TableFull,
}

impl ReadyError {
    /// Stable marker token.
    pub fn label(self) -> &'static str {
        match self {
            ReadyError::UnknownPid => "unknown-pid",
            ReadyError::AlreadyReady => "double-ready",
            ReadyError::TableFull => "table-full",
        }
    }
}

/// Bounded set of pids that have announced `@ready` and not exited since.
#[derive(Debug, Clone)]
pub struct ReadyTable {
    ready: [u32; READY_MAX],
    len: usize,
}

impl Default for ReadyTable {
    fn default() -> Self {
        Self::new()
    }
}

impl ReadyTable {
    /// Empty table.
    pub const fn new() -> Self {
        Self { ready: [0; READY_MAX], len: 0 }
    }

    /// Record `pid`'s `@ready`. `known` says whether `pid` is a live child of init
    /// (the caller derives it from its control-channel list — the channel IS the identity).
    pub fn announce(&mut self, pid: u32, known: bool) -> Result<(), ReadyError> {
        if !known || pid == 0 {
            return Err(ReadyError::UnknownPid);
        }
        if self.is_ready(pid) {
            return Err(ReadyError::AlreadyReady);
        }
        if self.len >= READY_MAX {
            return Err(ReadyError::TableFull);
        }
        self.ready[self.len] = pid;
        self.len += 1;
        Ok(())
    }

    /// `pid` exited: it is no longer ready; a respawned instance announces again.
    pub fn on_exit(&mut self, pid: u32) {
        if let Some(i) = self.ready[..self.len].iter().position(|&p| p == pid) {
            self.ready[i] = self.ready[self.len - 1];
            self.ready[self.len - 1] = 0;
            self.len -= 1;
        }
    }

    /// Whether `pid` has announced and not exited since.
    pub fn is_ready(&self, pid: u32) -> bool {
        self.ready[..self.len].contains(&pid)
    }

    /// Number of ready children.
    pub fn count(&self) -> usize {
        self.len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn announce_then_ready() {
        let mut t = ReadyTable::new();
        assert!(!t.is_ready(7));
        t.announce(7, true).unwrap();
        assert!(t.is_ready(7));
        assert_eq!(t.count(), 1);
    }

    #[test]
    fn test_reject_ready_from_unknown_pid() {
        let mut t = ReadyTable::new();
        assert_eq!(t.announce(7, false), Err(ReadyError::UnknownPid));
        assert_eq!(t.announce(0, true), Err(ReadyError::UnknownPid));
        assert!(!t.is_ready(7));
    }

    #[test]
    fn test_reject_double_ready_without_exit() {
        let mut t = ReadyTable::new();
        t.announce(7, true).unwrap();
        assert_eq!(t.announce(7, true), Err(ReadyError::AlreadyReady));
        // After an exit the restarted instance may announce again.
        t.on_exit(7);
        assert!(!t.is_ready(7));
        t.announce(7, true).unwrap();
    }

    #[test]
    fn exit_of_unknown_pid_is_a_noop() {
        let mut t = ReadyTable::new();
        t.announce(1, true).unwrap();
        t.on_exit(99);
        assert!(t.is_ready(1));
        assert_eq!(t.count(), 1);
    }

    #[test]
    fn test_reject_table_full_is_loud() {
        let mut t = ReadyTable::new();
        for pid in 1..=(READY_MAX as u32) {
            t.announce(pid, true).unwrap();
        }
        assert_eq!(t.announce(READY_MAX as u32 + 1, true), Err(ReadyError::TableFull));
    }
}
