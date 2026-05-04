// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Typed `inputd` configuration and range validation for TASK-0253.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p inputd -- --nocapture`
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use crate::InputdError;
use key_repeat::{DelayMs, RateHz, RepeatConfig};
use keymaps::LayoutId;
use pointer_accel::PointerAccelConfig;

const MAX_QUEUE_CAPACITY: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueCapacity(usize);

impl QueueCapacity {
    pub fn new(raw: usize) -> Result<Self, InputdError> {
        if raw == 0 || raw > MAX_QUEUE_CAPACITY {
            return Err(InputdError::InvalidQueueCapacity);
        }
        Ok(Self(raw))
    }

    #[must_use]
    pub const fn raw(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitialPointerPosition {
    x: i32,
    y: i32,
}

impl InitialPointerPosition {
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    #[must_use]
    pub const fn x(self) -> i32 {
        self.x
    }

    #[must_use]
    pub const fn y(self) -> i32 {
        self.y
    }
}

#[derive(Debug, Clone, Copy)]
pub struct InputdConfig {
    layout: LayoutId,
    repeat: RepeatConfig,
    pointer_accel: PointerAccelConfig,
    queue_capacity: QueueCapacity,
    initial_pointer: InitialPointerPosition,
}

impl InputdConfig {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        layout_name: &str,
        repeat_delay_ms: u32,
        repeat_rate_hz: u16,
        pointer_threshold: i32,
        pointer_numerator: i32,
        pointer_denominator: i32,
        pointer_max_output: i32,
        queue_capacity: usize,
        initial_pointer_x: i32,
        initial_pointer_y: i32,
    ) -> Result<Self, InputdError> {
        let layout = LayoutId::try_from(layout_name).map_err(InputdError::from)?;
        let repeat = RepeatConfig::new(
            DelayMs::new(repeat_delay_ms).map_err(InputdError::from)?,
            RateHz::new(repeat_rate_hz).map_err(InputdError::from)?,
        )
        .map_err(InputdError::from)?;
        let pointer_accel = PointerAccelConfig::new(
            pointer_threshold,
            pointer_numerator,
            pointer_denominator,
            pointer_max_output,
        )
        .map_err(InputdError::from)?;
        Ok(Self {
            layout,
            repeat,
            pointer_accel,
            queue_capacity: QueueCapacity::new(queue_capacity)?,
            initial_pointer: InitialPointerPosition::new(initial_pointer_x, initial_pointer_y),
        })
    }

    #[must_use]
    pub const fn layout(self) -> LayoutId {
        self.layout
    }

    #[must_use]
    pub const fn repeat(self) -> RepeatConfig {
        self.repeat
    }

    #[must_use]
    pub const fn pointer_accel(self) -> PointerAccelConfig {
        self.pointer_accel
    }

    #[must_use]
    pub const fn queue_capacity(self) -> QueueCapacity {
        self.queue_capacity
    }

    #[must_use]
    pub const fn initial_pointer(self) -> InitialPointerPosition {
        self.initial_pointer
    }
}
