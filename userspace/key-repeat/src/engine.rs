// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Repeat engine state machine for deterministic press/release/tick behavior.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 5 integration tests in `tests/input_v1_0_host/tests/repeat_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use crate::{MonotonicNs, RepeatConfig, RepeatError, RepeatKey};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepeatEvent {
    key: RepeatKey,
    timestamp: MonotonicNs,
}

impl RepeatEvent {
    #[must_use]
    pub const fn new(key: RepeatKey, timestamp: MonotonicNs) -> Self {
        Self { key, timestamp }
    }

    #[must_use]
    pub const fn key(self) -> RepeatKey {
        self.key
    }

    #[must_use]
    pub const fn timestamp(self) -> MonotonicNs {
        self.timestamp
    }
}

#[derive(Debug, Clone, Copy)]
struct ActiveRepeat {
    key: RepeatKey,
    next_repeat_ns: u64,
}

#[derive(Debug, Clone)]
pub struct RepeatEngine {
    config: RepeatConfig,
    active: Option<ActiveRepeat>,
    last_tick: Option<MonotonicNs>,
}

impl RepeatEngine {
    #[must_use]
    pub const fn new(config: RepeatConfig) -> Self {
        Self { config, active: None, last_tick: None }
    }

    pub fn press(&mut self, key: RepeatKey, now: MonotonicNs) -> Result<(), RepeatError> {
        self.observe_time(now)?;
        self.active = Some(ActiveRepeat {
            key,
            next_repeat_ns: now.raw() + (self.config.delay().raw() as u64 * 1_000_000),
        });
        Ok(())
    }

    /// When the held key repeats next (`None` = no key held) — the owner arms its timer at it
    /// (RFC-0113: a repeat is a kernel timer while a key is held, never a tick).
    #[must_use]
    pub fn next_deadline_ns(&self) -> Option<u64> {
        self.active.map(|active| active.next_repeat_ns)
    }

    pub fn release(&mut self, key: RepeatKey) {
        if self.active.map(|active| active.key) == Some(key) {
            self.active = None;
        }
    }

    /// The held key's repeat due at `now`, if any — ONE at most, stamped with the time it was
    /// due. A consumer that fell behind (a stall past a whole period) gets one repeat now and the
    /// rhythm resumes from it: never a burst of the repeats it slept through (text that jumps by
    /// a dozen letters is worse than a repeat skipped), and never a held key that stops
    /// repeating because too many came due at once.
    pub fn tick(&mut self, now: MonotonicNs) -> Result<Option<RepeatEvent>, RepeatError> {
        self.observe_time(now)?;
        let Some(mut active) = self.active else {
            return Ok(None);
        };
        let due = active.next_repeat_ns;
        if now.raw() < due {
            return Ok(None);
        }
        let period = self.config.period_ns();
        let on_time = due.saturating_add(period);
        active.next_repeat_ns =
            if now.raw() >= on_time { now.raw().saturating_add(period) } else { on_time };
        self.active = Some(active);
        Ok(Some(RepeatEvent::new(active.key, MonotonicNs::new(due))))
    }

    fn observe_time(&mut self, now: MonotonicNs) -> Result<(), RepeatError> {
        if let Some(previous) = self.last_tick {
            if now.raw() < previous.raw() {
                return Err(RepeatError::NonMonotonicTime);
            }
        }
        self.last_tick = Some(now);
        Ok(())
    }
}
