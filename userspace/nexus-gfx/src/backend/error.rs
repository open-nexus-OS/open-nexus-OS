// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Backend-specific error type — mirrors nexus_gfx::GfxError for backend independence.
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Stable

use crate::core::error::GfxError as CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GfxError {
    DeviceNotFound,
    CommandRejected,
    ResourceExhausted,
    Unsupported,
    InvalidArgument,
    MmioFault,
}

impl From<CoreError> for GfxError {
    fn from(value: CoreError) -> Self {
        match value {
            CoreError::DeviceNotFound => Self::DeviceNotFound,
            CoreError::CommandRejected => Self::CommandRejected,
            CoreError::ResourceExhausted => Self::ResourceExhausted,
            CoreError::Unsupported => Self::Unsupported,
            CoreError::InvalidArgument => Self::InvalidArgument,
            CoreError::MmioFault => Self::MmioFault,
        }
    }
}
