// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Canonical error classes for `windowd` contract and reject paths.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No direct tests
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowdError {
    InvalidDimensions,
    InvalidStride,
    UnsupportedFormat,
    MissingVmoHandle,
    ForgedVmoHandle,
    WrongVmoRights,
    NonSurfaceBuffer,
    SurfaceTooLarge,
    TooManySurfaces,
    TooManyLayers,
    TooManyDamageRects,
    InvalidDamage,
    StaleSurfaceId,
    StaleCommitSequence,
    Unauthorized,
    NoCommittedScene,
    ArithmeticOverflow,
    BufferLengthMismatch,
    MarkerBeforePresentState,
    InvalidDisplayCapability,
    InvalidFrameIndex,
    StalePresentSequence,
    SchedulerQueueFull,
    FenceNotReady,
    InputEventQueueFull,
    NoFocusedSurface,
}

pub type Result<T> = core::result::Result<T, WindowdError>;
