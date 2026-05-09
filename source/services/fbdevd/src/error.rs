// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Stable error classes for service-owned display scanout.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `fbdevd` host reject tests.
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "fbdevd errors must be handled"]
pub enum FbdevdError {
    InvalidMode,
    InvalidFramebufferCap,
    InvalidRamfbFwCfg,
    FwCfgMap,
    RamfbFileMissing,
    RamfbFileTooSmall,
    FramebufferVmo,
    FrameWrite,
    DmaVmoCreate,
    DmaCapQuery,
    DmaCapInvalid,
    DmaMapPage,
    DmaConfigWrite,
    DmaDescriptorWrite,
    DmaDeviceError,
    DmaTimeout,
    FlushWithoutConfiguredBackend,
    PresentWithoutFrame,
    StaleScanoutGeneration,
    IpcRoute,
    IpcReceive,
}

pub type Result<T> = core::result::Result<T, FbdevdError>;

impl FbdevdError {
    pub const fn label(self) -> &'static str {
        match self {
            Self::InvalidMode => "fbdevd: fail invalid-mode",
            Self::InvalidFramebufferCap => "fbdevd: fail invalid-framebuffer-cap",
            Self::InvalidRamfbFwCfg => "fbdevd: fail fw-cfg-signature",
            Self::FwCfgMap => "fbdevd: fail fw-cfg-map",
            Self::RamfbFileMissing => "fbdevd: fail ramfb-file-missing",
            Self::RamfbFileTooSmall => "fbdevd: fail ramfb-file-too-small",
            Self::FramebufferVmo => "fbdevd: fail framebuffer-vmo",
            Self::FrameWrite => "fbdevd: fail flush",
            Self::DmaVmoCreate => "fbdevd: fail dma-vmo-create",
            Self::DmaCapQuery => "fbdevd: fail dma-cap-query",
            Self::DmaCapInvalid => "fbdevd: fail dma-cap-invalid",
            Self::DmaMapPage => "fbdevd: fail dma-map-page",
            Self::DmaConfigWrite => "fbdevd: fail dma-config-write",
            Self::DmaDescriptorWrite => "fbdevd: fail dma-descriptor-write",
            Self::DmaDeviceError => "fbdevd: fail dma-device",
            Self::DmaTimeout => "fbdevd: fail dma-timeout",
            Self::FlushWithoutConfiguredBackend => "fbdevd: fail flush-unconfigured",
            Self::PresentWithoutFrame => "fbdevd: fail present-without-frame",
            Self::StaleScanoutGeneration => "fbdevd: fail stale-scanout-generation",
            Self::IpcRoute => "fbdevd: fail input-route",
            Self::IpcReceive => "fbdevd: fail input-recv",
        }
    }
}
