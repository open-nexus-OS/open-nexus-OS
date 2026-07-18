// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Low-level MMIO transport: the fixed device address-space map (queue / command
//! / response / resource / virgl-backing VA regions), the volatile register
//! accessors, the virtio-gpu control-header constructor, and small alignment
//! helpers. Shared by the virtqueue ring and the GfxBackend command methods.

#![cfg(all(feature = "os-lite", target_os = "none"))]

#[cfg(feature = "virgl")]
use super::virtqueue::PIPELINE_HARVEST_LOGGED;
use super::ResourceRecord;
use super::VirtioGpuBackend;
#[allow(unused_imports)]
use crate::markers::{
    GPUD_CHAIN_BATCH_OK, GPUD_RESOURCE_ATTACH_CMD_FAIL, GPUD_RESOURCE_CAP_QUERY_FAIL,
    GPUD_RESOURCE_CREATED, GPUD_RESOURCE_CREATE_CMD_FAIL, GPUD_RESOURCE_VMO_CREATE_FAIL,
    GPUD_RESOURCE_VMO_MAP_FAIL,
};
use crate::protocol;
use nexus_gfx::backend::error::GfxError;
use nexus_gfx::backend::types::{Rect, ResourceId};
use nexus_gfx::core::types::PixelFormat;

pub(crate) const GPU_QUEUE_VA: usize = 0x2030_0000;
// Control queue command-buffer POOL: `RING_SLOTS` contiguous 4 KiB pages, one
// per in-flight command slot, starting here. The multi-entry ring batches a
// whole present's commands (one buffer each) then completes once — so a textured
// draw whose completion QEMU defers no longer blocks the next command. Pool ends
// at GPU_CMD_VA + RING_SLOTS*4096 = 0x2033_0000 (32 slots), just below the resp pages.
pub(crate) const GPU_CMD_VA: usize = 0x2031_0000;
// Response POOL: `RING_SLOTS` × 256 B response sub-slots (grows with the ring;
// 32 slots = two 4 KiB pages). Slot i's resp is at GPU_RESP_VA + i*256.
pub(crate) const GPU_RESP_VA: usize = 0x2033_0000;
// Cursor virtqueue (queue index 1) — separate VA region so it does not collide
// with the control queue's desc/cmd-pool/resp pages. The hardware cursor overlay is
// the GPU "hot path" for the pointer: MOVE_CURSOR repositions it host-side
// without re-rendering the scene. The cursor queue is single-slot (no batching).
pub(crate) const GPU_CURSOR_QUEUE_VA: usize = 0x2034_0000;
pub(crate) const GPU_CURSOR_CMD_VA: usize = 0x2035_0000;
pub(crate) const GPU_CURSOR_RESP_VA: usize = 0x2035_1000;
pub(crate) const GPU_RESOURCE_BASE_VA: usize = 0x2040_0000;
// 32 MB per resource VA slot. The external framebuffer is now 1280×6400×4 ≈ 31.3 MB
// (4 display planes + surface atlas), so the 16 MB stride would overflow into the
// next slot. 32 MB stride × ≤11 slots stays below GPU_VIRGL_BACKING_BASE_VA.
pub(crate) const GPU_RESOURCE_STRIDE: usize = 0x0200_0000;
/// Fixed display-plane location within the framebuffer resource. The 4-plane
/// layout is: wallpaper(0) / retained(800) / DISPLAY(1600) / blur-cache(2400),
/// with the surface atlas at 3200+. This is a FIXED row — NOT `height/2` — since
/// the resource grew to 6400 rows to host the atlas, but the display plane stays
/// at 1600. Must match windowd's `DISPLAY_ROW_OFFSET`.
pub(crate) const DISPLAY_PLANE_ROW: u32 = 1600;
pub(crate) const DISPLAY_PLANE_HEIGHT: u32 = 800;
/// VA region for virgl 3D resource backings (readback targets) — separate from
/// the 2D resource region so the two allocators never collide.
#[cfg(feature = "virgl")]
pub(crate) const GPU_VIRGL_BACKING_BASE_VA: usize = 0x3800_0000;
#[cfg(feature = "virgl")]
pub(crate) const GPU_VIRGL_BACKING_STRIDE: usize = 0x0100_0000;

pub(crate) fn ctrl_hdr(type_: u32) -> protocol::VirtioGpuCtrlHdr {
    protocol::VirtioGpuCtrlHdr { type_, flags: 0, fence_id: 0, ctx_id: 0, _padding: 0 }
}

pub(crate) fn read_reg(base: usize, offset: usize) -> u32 {
    unsafe { core::ptr::read_volatile((base + offset) as *const u32) }
}

pub(crate) fn write_reg(base: usize, offset: usize, value: u32) {
    unsafe { core::ptr::write_volatile((base + offset) as *mut u32, value) }
}

pub(crate) fn write_u64_pair(base: usize, low_reg: usize, value: u64) {
    write_reg(base, low_reg, value as u32);
    write_reg(base, low_reg + 4, (value >> 32) as u32);
}

pub(crate) const fn align4(value: usize) -> usize {
    (value + 3) & !3
}

pub(crate) const fn align_page(value: usize) -> usize {
    (value + 4095) & !4095
}

impl VirtioGpuBackend {
    /// Submit a `VirtioGpuSubmit3d` header followed by a hand-encoded virgl
    /// command stream on the control queue (one descriptor chain). The response
    /// is validated by `wait_complete` (RESP_OK_NODATA → Ok).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn ctrl_submit_header_tail<T>(
        &mut self,
        hdr: &T,
        tail: &[u8],
    ) -> Result<(), GfxError> {
        let hdr_bytes = unsafe {
            core::slice::from_raw_parts((hdr as *const T).cast::<u8>(), core::mem::size_of::<T>())
        };
        let mmio = self.mmio_base;
        let batch = self.ctrl_batch;
        let queue = self.ctrlq.as_mut().ok_or(GfxError::DeviceNotFound)?;
        if batch {
            queue.enqueue_pair(mmio, hdr_bytes, tail).map(|_| ())
        } else {
            queue.submit_two(mmio, hdr_bytes, tail)
        }
    }

    /// Begin a control-queue command batch: subsequent `ctrl_submit_*` calls
    /// enqueue without waiting. Pair with [`ctrl_batch_end`].
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn ctrl_batch_begin(&mut self) {
        self.ctrl_batch = true;
    }

    /// True while the control ring is more than half busy with previous batches.
    /// The hold-phase splash tick uses this to SKIP re-enqueueing a frame instead
    /// of piling onto deferred completions — enqueueing anyway would park this
    /// single-threaded loop in ring back-pressure and starve the reveal gate's
    /// wall-clock re-evaluation (the serialized-500ms black screen).
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn ctrl_ring_congested(&self) -> bool {
        self.ctrlq.as_ref().map(|q| q.ring.in_flight() * 2 > q.ring.capacity()).unwrap_or(false)
    }

    /// End the batch — **pipelined: does NOT block**. Clears the batch flag and
    /// opportunistically `harvest`s completed slots (the next present would
    /// otherwise reclaim them). The present's own commands stay in flight and are
    /// reaped by the *next* frame's enqueue, so a textured draw whose completion
    /// QEMU defers never blocks this present. Emits the G3c "pipeline flowing"
    /// marker once the first prior-frame batch is reclaimed.
    #[cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]
    pub(crate) fn ctrl_batch_end(&mut self) -> Result<(), GfxError> {
        self.ctrl_batch = false;
        let queue = self.ctrlq.as_mut().ok_or(GfxError::DeviceNotFound)?;
        let before = queue.ring.in_flight();
        queue.harvest();
        if before != 0
            && queue.ring.in_flight() != before
            && !PIPELINE_HARVEST_LOGGED.swap(true, core::sync::atomic::Ordering::Relaxed)
        {
            let _ = nexus_abi::debug_println(crate::markers::GPUD_CHAIN_BATCH_OK);
        }
        Ok(())
    }

    pub(crate) fn create_resource_os(
        &mut self,
        id: ResourceId,
        w: u32,
        h: u32,
        fmt: PixelFormat,
        byte_len: usize,
    ) -> Result<(usize, u64, usize, u32), GfxError> {
        let resource_index = self.alloc_resource_va_index()?;
        let backing_len = align_page(byte_len);
        let backing_va = GPU_RESOURCE_BASE_VA + resource_index * GPU_RESOURCE_STRIDE;
        let backing_vmo = nexus_abi::vmo_create(backing_len).map_err(|_e| {
            let _ = nexus_abi::debug_println(GPUD_RESOURCE_VMO_CREATE_FAIL);
            GfxError::ResourceExhausted
        })?;
        let flags = nexus_abi::page_flags::VALID
            | nexus_abi::page_flags::USER
            | nexus_abi::page_flags::READ
            | nexus_abi::page_flags::WRITE;
        for offset in (0..backing_len).step_by(4096) {
            nexus_abi::vmo_map_page(backing_vmo, backing_va + offset, offset, flags).map_err(
                |_e| {
                    let _ = nexus_abi::debug_println(GPUD_RESOURCE_VMO_MAP_FAIL);
                    GfxError::MmioFault
                },
            )?;
        }
        unsafe { core::ptr::write_bytes(backing_va as *mut u8, 0, backing_len) };
        let mut info = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
        nexus_abi::cap_query(backing_vmo, &mut info).map_err(|_e| {
            let _ = nexus_abi::debug_println(GPUD_RESOURCE_CAP_QUERY_FAIL);
            GfxError::MmioFault
        })?;

        let gpu_format = Self::to_gpu_format(fmt);
        // Debug: emit format and resource_id values
        match gpu_format {
            1 => {
                let _ = nexus_abi::trace_line("gpud: dbg fmt=B8G8R8A8");
            }
            67 => {
                let _ = nexus_abi::trace_line("gpud: dbg fmt=R8G8B8A8");
            }
            _ => {
                let _ = nexus_abi::trace_line("gpud: dbg fmt=UNKNOWN");
            }
        }
        match id.0 {
            0 => {
                let _ = nexus_abi::trace_line("gpud: dbg rid=0");
            }
            1 => {
                let _ = nexus_abi::trace_line("gpud: dbg rid=1");
            }
            _ => {
                let _ = nexus_abi::trace_line("gpud: dbg rid=OTHER");
            }
        }
        let create = protocol::VirtioGpuResourceCreate2d {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_CREATE_RESOURCE_2D),
            resource_id: id.0,
            format: gpu_format,
            width: w,
            height: h,
        };
        self.ctrl_submit_struct(&create).map_err(|e| {
            let _ = nexus_abi::debug_println(GPUD_RESOURCE_CREATE_CMD_FAIL);
            e
        })?;

        let attach = protocol::VirtioGpuResourceAttachBacking {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING),
            resource_id: id.0,
            nr_entries: 1,
        };
        let entry =
            protocol::VirtioGpuMemEntry { addr: info.base, length: byte_len as u32, _padding: 0 };
        self.ctrl_submit_pair(&attach, &entry).map_err(|e| {
            let _ = nexus_abi::debug_println(GPUD_RESOURCE_ATTACH_CMD_FAIL);
            e
        })?;
        let _ = nexus_abi::debug_println(GPUD_RESOURCE_CREATED);
        Ok((backing_va, info.base, backing_len, backing_vmo))
    }

    pub(crate) fn transfer_to_host_os(
        &mut self,
        record: ResourceRecord,
        rect: Rect,
    ) -> Result<(), GfxError> {
        let bytes_per_pixel = 4u64;
        let row_stride = u64::from(record.width)
            .checked_mul(bytes_per_pixel)
            .ok_or(GfxError::ResourceExhausted)?;
        let row_offset =
            u64::from(rect.y).checked_mul(row_stride).ok_or(GfxError::ResourceExhausted)?;
        let col_offset =
            u64::from(rect.x).checked_mul(bytes_per_pixel).ok_or(GfxError::ResourceExhausted)?;
        let offset = row_offset.checked_add(col_offset).ok_or(GfxError::ResourceExhausted)?;
        let cmd = protocol::VirtioGpuTransferToHost2d {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D),
            r: protocol::VirtioGpuRect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
            },
            offset,
            resource_id: record.id.0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&cmd)
    }

    pub(crate) fn flush_rect_os(
        &mut self,
        record: ResourceRecord,
        rect: Rect,
    ) -> Result<(), GfxError> {
        let cmd = protocol::VirtioGpuResourceFlush {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_RESOURCE_FLUSH),
            r: protocol::VirtioGpuRect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
            },
            resource_id: record.id.0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&cmd)
    }

    pub(crate) fn set_scanout_os(&mut self, record: ResourceRecord) -> Result<(), GfxError> {
        let cmd = protocol::VirtioGpuSetScanout {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_SET_SCANOUT),
            r: protocol::VirtioGpuRect { x: 0, y: 0, width: record.width, height: record.height },
            scanout_id: 0,
            resource_id: record.id.0,
        };
        self.ctrl_submit_struct(&cmd)
    }

    pub(crate) fn move_cursor_os(&mut self, x: u32, y: u32) -> Result<(), GfxError> {
        let cmd = protocol::VirtioGpuCursorPos {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_MOVE_CURSOR),
            pos: protocol::VirtioGpuCursorPosData { scanout_id: 0, x, y, _padding: 0 },
            resource_id: 1,
            hot_x: 0,
            hot_y: 0,
            _padding: 0,
        };
        self.cursor_submit_struct(&cmd)
    }

    pub(crate) fn ctrl_submit_struct<T>(&mut self, cmd: &T) -> Result<(), GfxError> {
        let bytes = unsafe {
            core::slice::from_raw_parts((cmd as *const T).cast::<u8>(), core::mem::size_of::<T>())
        };
        self.ctrl_submit_bytes(bytes)
    }

    /// Submit a cursor command (UPDATE_CURSOR / MOVE_CURSOR) on the cursor queue.
    /// Cursor-queue commands carry no response payload (see `submit_no_response`).
    pub(crate) fn cursor_submit_struct<T>(&mut self, cmd: &T) -> Result<(), GfxError> {
        let bytes = unsafe {
            core::slice::from_raw_parts((cmd as *const T).cast::<u8>(), core::mem::size_of::<T>())
        };
        let queue = self.cursorq.as_mut().ok_or(GfxError::DeviceNotFound)?;
        queue.submit_no_response(self.mmio_base, bytes)
    }

    pub(crate) fn ctrl_submit_pair<A, B>(&mut self, a: &A, b: &B) -> Result<(), GfxError> {
        let a_bytes = unsafe {
            core::slice::from_raw_parts((a as *const A).cast::<u8>(), core::mem::size_of::<A>())
        };
        let b_bytes = unsafe {
            core::slice::from_raw_parts((b as *const B).cast::<u8>(), core::mem::size_of::<B>())
        };
        let mmio = self.mmio_base;
        let batch = self.ctrl_batch;
        let queue = self.ctrlq.as_mut().ok_or(GfxError::DeviceNotFound)?;
        if batch {
            queue.enqueue_pair(mmio, a_bytes, b_bytes).map(|_| ())
        } else {
            queue.submit_two(mmio, a_bytes, b_bytes)
        }
    }

    pub(crate) fn ctrl_submit_bytes(&mut self, bytes: &[u8]) -> Result<(), GfxError> {
        let mmio = self.mmio_base;
        let batch = self.ctrl_batch;
        let queue = self.ctrlq.as_mut().ok_or(GfxError::DeviceNotFound)?;
        if batch {
            queue.enqueue_pair(mmio, bytes, &[]).map(|_| ())
        } else {
            queue.submit(mmio, bytes)
        }
    }

    /// Query scanout 0's preferred mode (`GET_DISPLAY_INFO` with a decoded
    /// reply — the ONE control command whose OK carries data). Blocking
    /// round-trip on the control queue; `None` = query failed / scanout
    /// disabled (callers keep their fallback mode).
    pub(crate) fn ctrl_query_display_info(&mut self) -> Option<(u32, u32)> {
        let cmd = protocol::VirtioGpuCtrlHdr {
            type_: protocol::VIRTIO_GPU_CMD_GET_DISPLAY_INFO,
            flags: 0,
            fence_id: 0,
            ctx_id: 0,
            _padding: 0,
        };
        let bytes = unsafe {
            core::slice::from_raw_parts(
                (&cmd as *const protocol::VirtioGpuCtrlHdr).cast::<u8>(),
                core::mem::size_of::<protocol::VirtioGpuCtrlHdr>(),
            )
        };
        let mmio = self.mmio_base;
        let queue = self.ctrlq.as_mut()?;
        queue.submit_display_info_query(mmio, bytes).ok()
    }
}
