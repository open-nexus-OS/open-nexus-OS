// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Typed touch bounds, timestamps, samples, and normalized events.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 4 integration tests in `tests/input_v1_0_host/tests/touch_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use crate::TouchError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TouchTimestampNs(u64);

impl TouchTimestampNs {
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchBounds {
    width: u32,
    height: u32,
}

impl TouchBounds {
    pub fn new(width: u32, height: u32) -> Result<Self, TouchError> {
        if width == 0 || height == 0 {
            return Err(TouchError::InvalidBounds);
        }
        Ok(Self { width, height })
    }

    #[must_use]
    pub const fn width(self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn height(self) -> u32 {
        self.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TouchX(u32);

impl TouchX {
    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TouchY(u32);

impl TouchY {
    #[must_use]
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchPhase {
    Down,
    Move,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawTouchSample {
    timestamp: TouchTimestampNs,
    x: u32,
    y: u32,
    phase: TouchPhase,
}

impl RawTouchSample {
    #[must_use]
    pub const fn new(timestamp: TouchTimestampNs, x: u32, y: u32, phase: TouchPhase) -> Self {
        Self { timestamp, x, y, phase }
    }

    #[must_use]
    pub const fn timestamp(self) -> TouchTimestampNs {
        self.timestamp
    }

    #[must_use]
    pub const fn x(self) -> u32 {
        self.x
    }

    #[must_use]
    pub const fn y(self) -> u32 {
        self.y
    }

    #[must_use]
    pub const fn phase(self) -> TouchPhase {
        self.phase
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchEvent {
    timestamp: TouchTimestampNs,
    x: TouchX,
    y: TouchY,
    phase: TouchPhase,
}

impl TouchEvent {
    #[must_use]
    pub const fn new(timestamp: TouchTimestampNs, x: TouchX, y: TouchY, phase: TouchPhase) -> Self {
        Self { timestamp, x, y, phase }
    }

    #[must_use]
    pub const fn timestamp(self) -> TouchTimestampNs {
        self.timestamp
    }

    #[must_use]
    pub const fn x(self) -> TouchX {
        self.x
    }

    #[must_use]
    pub const fn y(self) -> TouchY {
        self.y
    }

    #[must_use]
    pub const fn phase(self) -> TouchPhase {
        self.phase
    }
}
