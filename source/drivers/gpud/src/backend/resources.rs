// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Resource bookkeeping: the `ResourceRecord` lookup, pixel-format mapping,
//! scanout VMO cloning, and the small validation/error-mapping helpers shared
//! by the `GfxBackend` resource methods.

use super::{ResourceRecord, VirtioGpuBackend};
#[cfg(all(feature = "os-lite", target_os = "none"))]
use crate::protocol;
use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::types::{Rect, ResourceId};
use nexus_gfx::core::types::PixelFormat;

/// How many 32 MiB slots the GPU-resource VA region holds.
/// 8 × 32 MiB from `GPU_RESOURCE_BASE_VA` ends at 0x3040_0000, below
/// `GPU_VIRGL_BACKING_BASE_VA` (0x3800_0000).
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) const MAX_RESOURCE_VA_SLOTS: usize = 8;

/// A VA range reserved for one resource mapping (TASK-0309).
///
/// The point is that a caller cannot map outside it by accident. Before this,
/// `alloc_resource_va_index` returned a bare index, the caller computed
/// `base + index * stride` and then looped over the resource's byte length —
/// with nothing relating the two. A resource larger than one slot walked into
/// its neighbour and the first sign of trouble was the KERNEL refusing a
/// remap, tens of megabytes in, as an opaque `vmo_map_page fail`.
#[cfg(all(feature = "os-lite", target_os = "none"))]
#[derive(Clone, Copy)]
pub(crate) struct VaWindow {
    /// First slot index (diagnostics only; the VA is authoritative).
    pub(crate) index: usize,
    pub(crate) base_va: usize,
    /// Reserved span in bytes — `slots * GPU_RESOURCE_STRIDE`.
    pub(crate) len: usize,
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
impl VaWindow {
    /// VA for the page at `offset`, or a REAL error when that offset is not
    /// inside this reservation.
    ///
    /// This is the guard the user asked for: the mapping loop can no longer
    /// run off the end of its window and let the kernel be the one to notice.
    /// A full region now fails at the allocator, and an over-long mapping
    /// fails here — both by name, with numbers, before a single page is
    /// touched outside the reservation.
    pub(crate) fn page_va(&self, offset: usize) -> Result<usize, GfxError> {
        if offset >= self.len {
            crate::diag::kv_line(
                b"va window overrun",
                &[
                    (b"idx", self.index as u64),
                    (b"base", self.base_va as u64),
                    (b"win", self.len as u64),
                    (b"off", offset as u64),
                ],
            );
            return Err(GfxError::ResourceExhausted);
        }
        Ok(self.base_va + offset)
    }
}

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

    /// Next GPU-resource VA slot for a mapping of `byte_len` — monotonic,
    /// never reused. There is no unmap primitive, so a released resource's
    /// pages stay mapped at its old VA; handing that VA to a new resource
    /// makes `vmo_map_page` fail (remap refused). `resources.len()` had the
    /// same hazard after any removal.
    ///
    /// SIZE-AWARE since TASK-0309. The framebuffer is 46.9 MiB against a
    /// 32 MiB [`GPU_RESOURCE_STRIDE`], so it does not fit one slot — it spans
    /// two. The allocator used to hand out one index regardless of size, and
    /// the caller mapped straight past the slot boundary into whatever came
    /// next. That only ever "worked" because the framebuffer happened to land
    /// in slot 0 with nothing above it yet; the moment anything claimed slot 0
    /// first, the framebuffer landed in slot 1, ran into slot 2, and the
    /// kernel refused the remap 11.8 MiB in — with the display chain dead and
    /// a bare "vmo_map_page fail" as the only clue.
    ///
    /// Returns the RESERVED WINDOW, not a bare index: the caller maps through
    /// [`VaWindow::page_va`], which refuses an out-of-window offset up front
    /// instead of walking into a neighbour and letting the kernel discover it.
    ///
    /// [`GPU_RESOURCE_STRIDE`]: super::transport::GPU_RESOURCE_STRIDE
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    pub(crate) fn alloc_resource_va_index(
        &mut self,
        byte_len: usize,
    ) -> Result<VaWindow, GfxError> {
        use super::transport::{GPU_RESOURCE_BASE_VA, GPU_RESOURCE_STRIDE};
        // Slots this mapping occupies — 0 bytes still takes one, so the index
        // stays monotonic and a zero-sized resource can never alias the next.
        let slots = byte_len.div_ceil(GPU_RESOURCE_STRIDE).max(1);
        let index = self.next_resource_va_index;
        let end = index.checked_add(slots).ok_or(GfxError::ResourceExhausted)?;
        if end > MAX_RESOURCE_VA_SLOTS {
            crate::diag::kv_line(
                b"va region full",
                &[
                    (b"idx", index as u64),
                    (b"need", slots as u64),
                    (b"max", MAX_RESOURCE_VA_SLOTS as u64),
                    (b"len", byte_len as u64),
                ],
            );
            return Err(GfxError::ResourceExhausted);
        }
        self.next_resource_va_index = end;
        Ok(VaWindow {
            index,
            base_va: GPU_RESOURCE_BASE_VA + index * GPU_RESOURCE_STRIDE,
            len: slots * GPU_RESOURCE_STRIDE,
        })
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
