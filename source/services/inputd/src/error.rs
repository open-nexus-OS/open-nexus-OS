// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Stable `inputd` reject taxonomy for config, merge, and route failures.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `cargo test -p inputd -- --nocapture`
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputdError {
    Keymap(keymaps::KeymapError),
    Repeat(key_repeat::RepeatError),
    PointerAccel(pointer_accel::PointerAccelError),
    InvalidQueueCapacity,
    InitialPointerOutOfBounds { x: i32, y: i32 },
    PointerOutOfBounds { x: i32, y: i32 },
    QueueOverflow { capacity: usize },
    Route(windowd::WindowdError),
}

impl InputdError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Keymap(err) => err.code(),
            Self::Repeat(err) => err.code(),
            Self::PointerAccel(err) => err.code(),
            Self::InvalidQueueCapacity => "inputd.queue.capacity.invalid",
            Self::InitialPointerOutOfBounds { .. } => "inputd.pointer.initial_out_of_bounds",
            Self::PointerOutOfBounds { .. } => "inputd.pointer.out_of_bounds",
            Self::QueueOverflow { .. } => "inputd.queue.overflow",
            Self::Route(windowd::WindowdError::StaleSurfaceId) => "inputd.route.stale_surface",
            Self::Route(windowd::WindowdError::Unauthorized) => "inputd.route.unauthorized",
            Self::Route(windowd::WindowdError::NoFocusedSurface) => "inputd.route.no_focused_surface",
            Self::Route(windowd::WindowdError::InputEventQueueFull) => {
                "inputd.route.windowd_queue_full"
            }
            Self::Route(windowd::WindowdError::InvalidPointerPosition) => {
                "inputd.route.pointer.invalid"
            }
            Self::Route(_) => "inputd.route.windowd",
        }
    }
}

impl From<keymaps::KeymapError> for InputdError {
    fn from(value: keymaps::KeymapError) -> Self {
        Self::Keymap(value)
    }
}

impl From<key_repeat::RepeatError> for InputdError {
    fn from(value: key_repeat::RepeatError) -> Self {
        Self::Repeat(value)
    }
}

impl From<pointer_accel::PointerAccelError> for InputdError {
    fn from(value: pointer_accel::PointerAccelError) -> Self {
        Self::PointerAccel(value)
    }
}

impl From<windowd::WindowdError> for InputdError {
    fn from(value: windowd::WindowdError) -> Self {
        Self::Route(value)
    }
}

impl fmt::Display for InputdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Keymap(err) => err.fmt(f),
            Self::Repeat(err) => err.fmt(f),
            Self::PointerAccel(err) => err.fmt(f),
            Self::InvalidQueueCapacity => f.write_str("input queue capacity must be within bounds"),
            Self::InitialPointerOutOfBounds { x, y } => {
                write!(f, "initial pointer position out of bounds: ({x}, {y})")
            }
            Self::PointerOutOfBounds { x, y } => {
                write!(f, "pointer dispatch out of bounds: ({x}, {y})")
            }
            Self::QueueOverflow { capacity } => {
                write!(f, "input dispatch queue exceeded bounded capacity {capacity}")
            }
            Self::Route(err) => write!(f, "windowd route failed: {err:?}"),
        }
    }
}

#[cfg(not(all(nexus_env = "os", target_os = "none")))]
impl std::error::Error for InputdError {}
