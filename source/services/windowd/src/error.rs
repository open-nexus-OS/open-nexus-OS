// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

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
}

pub type Result<T> = core::result::Result<T, WindowdError>;
