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

    /// Next GPU-resource VA slot — monotonic, never reused. There is no unmap
    /// primitive, so a released resource's pages stay mapped at its old VA;
    /// handing that VA to a new resource makes `vmo_map_page` fail (remap
    /// refused). `resources.len()` had the same hazard after any removal.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub(crate) fn alloc_resource_va_index(&mut self) -> Result<usize, GfxError> {
        const MAX_RESOURCE_VA_SLOTS: usize = 8;
        let index = self.next_resource_va_index;
        if index >= MAX_RESOURCE_VA_SLOTS {
            return Err(GfxError::ResourceExhausted);
        }
        self.next_resource_va_index = index + 1;
        Ok(index)
    }

    /// Free a dead one-shot resource end-to-end (task #124): detach + unref the
    /// host resource, release the backing VMO back to the kernel arena, drop the
    /// record (its VA slot becomes reusable). Externally-owned backings
    /// (`backing_vmo == 0`, e.g. windowd's framebuffer) skip the VMO release.
    /// Host commands are best-effort on the ordered ring — they land after any
    /// earlier scanout switch, so the resource is never destroyed while shown.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub(crate) fn release_resource(&mut self, res: ResourceId) {
        use super::transport::ctrl_hdr;
        let Some(index) = self.resources.iter().position(|record| record.id == res) else {
            return;
        };
        let record = self.resources.remove(index);
        let detach = protocol::VirtioGpuResourceDetachBacking {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING),
            resource_id: res.0,
            _padding: 0,
        };
        let _ = self.ctrl_submit_struct(&detach);
        let unref = protocol::VirtioGpuResourceUnref {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_RESOURCE_UNREF),
            resource_id: res.0,
            _padding: 0,
        };
        let _ = self.ctrl_submit_struct(&unref);
        if record.backing_vmo != 0 {
            match nexus_abi::vmo_destroy(record.backing_vmo) {
                Ok(()) => {
                    let _ = nexus_abi::debug_println("gpud: resource vmo freed");
                }
                Err(_) => {
                    let _ = nexus_abi::debug_println("gpud: resource vmo free fail");
                }
            }
        }
        if self.scanout_resource == Some(res) {
            self.scanout_resource = None;
        }
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
