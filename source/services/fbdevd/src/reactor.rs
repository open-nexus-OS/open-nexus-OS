// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Minimal service-local reactor helpers for `fbdevd` live display ticks.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `fbdevd` host contract tests.
//! ADR: docs/adr/0017-service-architecture.md

use crate::vsync::VsyncCadence;
use input_live_protocol::VisibleState;
use windowd::VisibleBootstrapMode;

const VISIBLE_INPUT_PROOF_HEIGHT: u32 = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum ReactorProgress {
    Idle,
    Presented,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum DirtyRows {
    None,
    Full,
    Range { start_y: u32, end_y: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickBudget {
    remaining: u16,
}

impl TickBudget {
    pub const fn new(remaining: u16) -> Self {
        Self { remaining }
    }

    #[must_use]
    pub const fn remaining(self) -> u16 {
        self.remaining
    }

    pub fn take(&mut self) -> bool {
        if self.remaining == 0 {
            return false;
        }
        self.remaining -= 1;
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayReactor {
    cadence: VsyncCadence,
}

impl DisplayReactor {
    pub fn new(hz: u16) -> Self {
        Self {
            cadence: VsyncCadence::new(hz),
        }
    }

    pub fn should_present(&mut self, now_ns: u64, budget: &mut TickBudget) -> bool {
        budget.take() && self.cadence.should_tick(now_ns)
    }
}

pub fn live_dirty_rows(
    previous: VisibleState,
    next: VisibleState,
    mode: VisibleBootstrapMode,
) -> DirtyRows {
    if previous == next {
        return DirtyRows::None;
    }

    let Ok(mode) = mode.validate() else {
        return DirtyRows::Full;
    };
    let mut previous_without_cursor = previous;
    previous_without_cursor.cursor_x = next.cursor_x;
    previous_without_cursor.cursor_y = next.cursor_y;
    if previous_without_cursor != next || !previous.scene_ready || !next.scene_ready {
        return DirtyRows::Full;
    }

    let Some((previous_start, previous_end)) =
        cursor_physical_y_range(previous.cursor_x, previous.cursor_y, mode.height)
    else {
        return DirtyRows::Full;
    };
    let Some((next_start, next_end)) =
        cursor_physical_y_range(next.cursor_x, next.cursor_y, mode.height)
    else {
        return DirtyRows::Full;
    };

    DirtyRows::Range {
        start_y: previous_start.min(next_start),
        end_y: previous_end.max(next_end),
    }
}

fn cursor_physical_y_range(cursor_x: i32, cursor_y: i32, height: u32) -> Option<(u32, u32)> {
    let (Ok(_x), Ok(y)) = (u32::try_from(cursor_x), u32::try_from(cursor_y)) else {
        return None;
    };
    if height == 0 || y >= height {
        return None;
    }
    let extent = ceil_div(height, VISIBLE_INPUT_PROOF_HEIGHT);
    Some((y.min(height), y.saturating_add(extent).min(height)))
}

const fn ceil_div(value: u32, divisor: u32) -> u32 {
    if divisor == 0 {
        return 1;
    }
    let rounded = value.div_ceil(divisor);
    if rounded == 0 {
        1
    } else {
        rounded
    }
}
