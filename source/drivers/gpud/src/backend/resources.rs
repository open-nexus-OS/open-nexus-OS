// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Resource bookkeeping: the `ResourceRecord` lookup, pixel-format mapping,
//! scanout VMO cloning, and the small validation/error-mapping helpers shared
//! by the `GfxBackend` resource methods.

use super::{ResourceRecord, VirtioGpuBackend};
use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::types::{Rect, ResourceId};
use nexus_gfx::core::types::PixelFormat;
#[cfg(all(feature = "os-lite", target_os = "none"))]
use crate::protocol;

impl VirtioGpuBackend {
    /// Convert PixelFormat to virtio-gpu format constant.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub(crate) fn to_gpu_format(fmt: PixelFormat) -> u32 {
        match fmt {
            PixelFormat::Bgra8888 => protocol::VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM,
            PixelFormat::Rgba8888 => protocol::VIRTIO_GPU_FORMAT_R8G8B8A8_UNORM,
        }
    }

    /// Clone the backing VMO of a resource so another service (windowd)
    /// can write composed frames into the scanout framebuffer.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub fn clone_scanout_vmo(&self, res: ResourceId) -> Option<u32> {
        let record = self.find_resource(res)?;
        nexus_abi::cap_clone(record.backing_vmo).ok()
    }

    pub(crate) fn find_resource(&self, res: ResourceId) -> Option<ResourceRecord> {
        self.resources.iter().copied().find(|record| record.id == res)
    }
}

pub(crate) fn resource_byte_len(w: u32, h: u32) -> Result<usize, GfxError> {
    let pixels = u64::from(w).checked_mul(u64::from(h)).ok_or(GfxError::ResourceExhausted)?;
    let bytes = pixels.checked_mul(4).ok_or(GfxError::ResourceExhausted)?;
    if bytes == 0 || bytes > 16 * 1024 * 1024 {
        return Err(GfxError::ResourceExhausted);
    }
    Ok(bytes as usize)
}

pub(crate) fn validate_rect(record: ResourceRecord, rect: Rect) -> Result<(), GfxError> {
    let end_x = rect.x.checked_add(rect.width).ok_or(GfxError::InvalidArgument)?;
    let end_y = rect.y.checked_add(rect.height).ok_or(GfxError::InvalidArgument)?;
    if rect.width == 0 || rect.height == 0 || end_x > record.width || end_y > record.height {
        return Err(GfxError::InvalidArgument);
    }
    Ok(())
}

pub(crate) fn map_nexus_error(err: nexus_gfx::GfxError) -> GfxError {
    match err {
        nexus_gfx::GfxError::DeviceNotFound => GfxError::DeviceNotFound,
        nexus_gfx::GfxError::CommandRejected => GfxError::CommandRejected,
        nexus_gfx::GfxError::ResourceExhausted => GfxError::ResourceExhausted,
        nexus_gfx::GfxError::Unsupported => GfxError::Unsupported,
        nexus_gfx::GfxError::InvalidArgument => GfxError::InvalidArgument,
        nexus_gfx::GfxError::MmioFault => GfxError::MmioFault,
    }
}
