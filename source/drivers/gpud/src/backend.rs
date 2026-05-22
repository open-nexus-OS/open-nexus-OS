// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use gfx_backend::traits::GfxBackend;
use gfx_backend::types::{Rect, ResourceId};
use gfx_backend::GfxError;
use nexus_gfx::command_buffer::{Command, CommittedBuffer};
use nexus_gfx::fence::Fence;
use nexus_gfx::types::PixelFormat;

use crate::error::GpuDriverError;
use crate::protocol;

/// Wraps a virtio-gpu MMIO device and implements GfxBackend.
/// On real hardware, this would be replaced by a different GfxBackend impl
/// (e.g., MaliGpuBackend, ImaginationGpuBackend) — same trait, different hardware.
pub struct VirtioGpuBackend {
    mmio_base: usize,
    _mmio_len: usize,
    next_resource_id: u32,
    probed: bool,
}

impl VirtioGpuBackend {
    /// Create a new backend. Does NOT probe — call probe() separately.
    pub fn new(mmio_base: usize, mmio_len: usize) -> Self {
        Self { mmio_base, _mmio_len: mmio_len, next_resource_id: 1, probed: false }
    }

    /// Probe the MMIO region for a virtio-gpu device.
    /// Returns Ok if the device is found and initialized.
    pub fn probe(&mut self) -> Result<(), GpuDriverError> {
        // In QEMU/real hardware: read MMIO registers.
        // For host tests: always succeed (mock).
        #[cfg(not(test))]
        {
            let magic = unsafe { core::ptr::read_volatile((self.mmio_base + protocol::VIRTIO_MMIO_MAGIC_VALUE) as *const u32) };
            if magic != protocol::VIRTIO_MMIO_MAGIC {
                return Err(GpuDriverError::DeviceNotFound);
            }
            let device_id = unsafe { core::ptr::read_volatile((self.mmio_base + protocol::VIRTIO_MMIO_DEVICE_ID) as *const u32) };
            if device_id != protocol::VIRTIO_GPU_DEVICE_ID {
                return Err(GpuDriverError::DeviceNotFound);
            }
        }
        self.probed = true;
        Ok(())
    }

    pub fn is_probed(&self) -> bool { self.probed }

    /// Convert PixelFormat to virtio-gpu format constant.
    fn to_gpu_format(fmt: PixelFormat) -> u32 {
        match fmt {
            PixelFormat::Bgra8888 => protocol::VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM,
            PixelFormat::Rgba8888 => protocol::VIRTIO_GPU_FORMAT_R8G8B8A8_UNORM,
        }
    }
}

impl GfxBackend for VirtioGpuBackend {
    fn submit(&mut self, _cmd: CommittedBuffer) -> Result<Fence, GfxError> {
        // In full implementation: translate CommandBuffer commands to virtio-gpu protocol,
        // write to virtqueue, notify device.
        // For v1: return signaled fence (passthrough, no real GPU execution yet).
        if !self.probed {
            return Err(GfxError::DeviceNotFound);
        }
        Ok(Fence::new_signaled())
    }

    fn create_resource(&mut self, w: u32, h: u32, fmt: PixelFormat) -> Result<ResourceId, GfxError> {
        if w == 0 || h == 0 { return Err(GfxError::InvalidArgument); }
        if !self.probed { return Err(GfxError::DeviceNotFound); }
        let id = ResourceId(self.next_resource_id);
        self.next_resource_id += 1;
        // In full implementation: send CREATE_RESOURCE_2D + ATTACH_BACKING via virtqueue.
        let _format = Self::to_gpu_format(fmt);
        Ok(id)
    }

    fn transfer_to_host(&mut self, _res: ResourceId, _rect: Rect) -> Result<(), GfxError> {
        if !self.probed { return Err(GfxError::DeviceNotFound); }
        Ok(()) // stub
    }

    fn set_scanout(&mut self, _res: ResourceId) -> Result<(), GfxError> {
        if !self.probed { return Err(GfxError::DeviceNotFound); }
        Ok(()) // stub
    }

    fn move_cursor(&mut self, _x: i32, _y: i32) -> Result<(), GfxError> {
        if !self.probed { return Err(GfxError::DeviceNotFound); }
        Ok(()) // stub
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_unprobed_rejects_submit() {
        let mut b = VirtioGpuBackend::new(0x10008000, 0x200);
        let empty = nexus_gfx::CommandBuffer::new().commit();
        assert!(b.submit(empty).is_err());
    }

    #[test]
    fn backend_probed_accepts_submit() {
        let mut b = VirtioGpuBackend::new(0x10008000, 0x200);
        b.probe().unwrap();
        let empty = nexus_gfx::CommandBuffer::new().commit();
        assert!(b.submit(empty).is_ok());
    }

    #[test]
    fn create_resource_rejects_zero() {
        let mut b = VirtioGpuBackend::new(0x10008000, 0x200);
        b.probe().unwrap();
        assert!(b.create_resource(0, 64, PixelFormat::Bgra8888).is_err());
    }
}
