// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GfxError {
    DeviceNotFound,
    CommandRejected,
    ResourceExhausted,
    Unsupported,
    InvalidArgument,
    MmioFault,
}

impl From<nexus_gfx::GfxError> for GfxError {
    fn from(value: nexus_gfx::GfxError) -> Self {
        match value {
            nexus_gfx::GfxError::DeviceNotFound => Self::DeviceNotFound,
            nexus_gfx::GfxError::CommandRejected => Self::CommandRejected,
            nexus_gfx::GfxError::ResourceExhausted => Self::ResourceExhausted,
            nexus_gfx::GfxError::Unsupported => Self::Unsupported,
            nexus_gfx::GfxError::InvalidArgument => Self::InvalidArgument,
            nexus_gfx::GfxError::MmioFault => Self::MmioFault,
        }
    }
}
