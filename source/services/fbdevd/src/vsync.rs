// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Deterministic vsync cadence helper for cooperative display scanout.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `fbdevd` host tests.
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VsyncCadence {
    interval_ns: u64,
    last_tick_ns: u64,
}

impl VsyncCadence {
    pub fn new(hz: u16) -> Self {
        let interval_ns = if hz == 0 { 16_666_667 } else { 1_000_000_000_u64 / u64::from(hz) };
        Self { interval_ns, last_tick_ns: 0 }
    }

    pub fn should_tick(&mut self, now_ns: u64) -> bool {
        if now_ns == 0 {
            return false;
        }
        if self.last_tick_ns == 0 {
            self.last_tick_ns = now_ns;
            return true;
        }
        if now_ns.saturating_sub(self.last_tick_ns) >= self.interval_ns {
            self.last_tick_ns = now_ns;
            return true;
        }
        false
    }
}
