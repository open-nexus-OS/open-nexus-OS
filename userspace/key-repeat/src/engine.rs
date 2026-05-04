// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Repeat engine state machine for deterministic press/release/tick behavior.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 4 integration tests in `tests/input_v1_0_host/tests/repeat_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use crate::{MonotonicNs, RepeatConfig, RepeatError, RepeatKey};

const MAX_EVENTS_PER_TICK: usize = 64;

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

    pub fn release(&mut self, key: RepeatKey) {
        if self.active.map(|active| active.key) == Some(key) {
            self.active = None;
        }
    }

    pub fn tick(&mut self, now: MonotonicNs) -> Result<Vec<RepeatEvent>, RepeatError> {
        self.observe_time(now)?;

        let mut out = Vec::new();
        if let Some(mut active) = self.active {
            while now.raw() >= active.next_repeat_ns {
                if out.len() == MAX_EVENTS_PER_TICK {
                    return Err(RepeatError::TickBudgetExceeded);
                }
                out.push(RepeatEvent::new(active.key, MonotonicNs::new(active.next_repeat_ns)));
                active.next_repeat_ns += self.config.period_ns();
            }
            self.active = Some(active);
        }
        Ok(out)
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
