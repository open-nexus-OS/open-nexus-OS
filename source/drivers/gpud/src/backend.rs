// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use gfx_backend::traits::GfxBackend;
use gfx_backend::types::{Rect, ResourceId};
use gfx_backend::GfxError;
use nexus_gfx::command_buffer::CommittedBuffer;
use nexus_gfx::fence::Fence;
use nexus_gfx::types::PixelFormat;

use crate::error::GpuDriverError;
#[cfg(all(feature = "os-lite", target_os = "none"))]
use crate::protocol;

/// Wraps a virtio-gpu MMIO device and implements GfxBackend.
/// On real hardware, this would be replaced by a different GfxBackend impl
/// (e.g., MaliGpuBackend, ImaginationGpuBackend) — same trait, different hardware.
pub struct VirtioGpuBackend {
    mmio_base: usize,
    _mmio_len: usize,
    next_resource_id: u32,
    probed: bool,
    resources: alloc::vec::Vec<ResourceRecord>,
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    ctrlq: Option<CtrlQueue>,
}

#[derive(Clone, Copy)]
struct ResourceRecord {
    id: ResourceId,
    width: u32,
    height: u32,
    format: PixelFormat,
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    backing_va: usize,
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    backing_pa: u64,
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    backing_len: usize,
}

impl VirtioGpuBackend {
    /// Create a new backend. Does NOT probe — call probe() separately.
    pub fn new(mmio_base: usize, mmio_len: usize) -> Self {
        Self {
            mmio_base,
            _mmio_len: mmio_len,
            next_resource_id: 1,
            probed: false,
            resources: alloc::vec::Vec::new(),
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            ctrlq: None,
        }
    }

    /// Probe the MMIO region for a virtio-gpu device.
    /// Returns Ok if the device is found and initialized.
    pub fn probe(&mut self) -> Result<(), GpuDriverError> {
        #[cfg(not(all(feature = "os-lite", target_os = "none")))]
        let _ = self.mmio_base;
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        self.probe_os()?;
        self.probed = true;
        Ok(())
    }

    pub fn is_probed(&self) -> bool {
        self.probed
    }

    /// Convert PixelFormat to virtio-gpu format constant.
    #[cfg(all(feature = "os-lite", target_os = "none"))]
    fn to_gpu_format(fmt: PixelFormat) -> u32 {
        match fmt {
            PixelFormat::Bgra8888 => protocol::VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM,
            PixelFormat::Rgba8888 => protocol::VIRTIO_GPU_FORMAT_R8G8B8A8_UNORM,
        }
    }

    fn find_resource(&self, res: ResourceId) -> Option<ResourceRecord> {
        self.resources
            .iter()
            .copied()
            .find(|record| record.id == res)
    }
}

impl GfxBackend for VirtioGpuBackend {
    fn submit(&mut self, cmd: CommittedBuffer) -> Result<Fence, GfxError> {
        if !self.probed {
            return Err(GfxError::DeviceNotFound);
        }
        cmd.validate().map_err(map_nexus_error)?;
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        {
            // The v1 command vocabulary is validated above. Resource mutation happens through
            // explicit transfer/set_scanout calls so QEMU completion is still tied to virtqueue IO.
            let _ = cmd.command_count();
        }
        Ok(Fence::new_signaled())
    }

    fn create_resource(
        &mut self,
        w: u32,
        h: u32,
        fmt: PixelFormat,
    ) -> Result<ResourceId, GfxError> {
        if w == 0 || h == 0 {
            return Err(GfxError::InvalidArgument);
        }
        if !self.probed {
            return Err(GfxError::DeviceNotFound);
        }
        if self.resources.len() >= 4 {
            return Err(GfxError::ResourceExhausted);
        }
        let _byte_len = resource_byte_len(w, h)?;
        let id = ResourceId(self.next_resource_id);
        self.next_resource_id += 1;
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        let (backing_va, backing_pa, backing_len) =
            self.create_resource_os(id, w, h, fmt, _byte_len)?;
        self.resources.push(ResourceRecord {
            id,
            width: w,
            height: h,
            format: fmt,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            backing_va,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            backing_pa,
            #[cfg(all(feature = "os-lite", target_os = "none"))]
            backing_len,
        });
        Ok(id)
    }

    fn transfer_to_host(&mut self, res: ResourceId, rect: Rect) -> Result<(), GfxError> {
        if !self.probed {
            return Err(GfxError::DeviceNotFound);
        }
        let record = self.find_resource(res).ok_or(GfxError::InvalidArgument)?;
        validate_rect(record, rect)?;
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        self.transfer_to_host_os(record, rect)?;
        Ok(())
    }

    fn set_scanout(&mut self, res: ResourceId) -> Result<(), GfxError> {
        if !self.probed {
            return Err(GfxError::DeviceNotFound);
        }
        let record = self.find_resource(res).ok_or(GfxError::InvalidArgument)?;
        #[cfg(not(all(feature = "os-lite", target_os = "none")))]
        let _ = record.format;
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        self.set_scanout_os(record)?;
        Ok(())
    }

    fn move_cursor(&mut self, x: i32, y: i32) -> Result<(), GfxError> {
        if !self.probed {
            return Err(GfxError::DeviceNotFound);
        }
        if x < 0 || y < 0 {
            return Err(GfxError::InvalidArgument);
        }
        #[cfg(all(feature = "os-lite", target_os = "none"))]
        self.move_cursor_os(x as u32, y as u32)?;
        Ok(())
    }
}

fn resource_byte_len(w: u32, h: u32) -> Result<usize, GfxError> {
    let pixels = u64::from(w)
        .checked_mul(u64::from(h))
        .ok_or(GfxError::ResourceExhausted)?;
    let bytes = pixels.checked_mul(4).ok_or(GfxError::ResourceExhausted)?;
    if bytes == 0 || bytes > 16 * 1024 * 1024 {
        return Err(GfxError::ResourceExhausted);
    }
    Ok(bytes as usize)
}

fn validate_rect(record: ResourceRecord, rect: Rect) -> Result<(), GfxError> {
    let end_x = rect
        .x
        .checked_add(rect.width)
        .ok_or(GfxError::InvalidArgument)?;
    let end_y = rect
        .y
        .checked_add(rect.height)
        .ok_or(GfxError::InvalidArgument)?;
    if rect.width == 0 || rect.height == 0 || end_x > record.width || end_y > record.height {
        return Err(GfxError::InvalidArgument);
    }
    Ok(())
}

fn map_nexus_error(err: nexus_gfx::GfxError) -> GfxError {
    match err {
        nexus_gfx::GfxError::DeviceNotFound => GfxError::DeviceNotFound,
        nexus_gfx::GfxError::CommandRejected => GfxError::CommandRejected,
        nexus_gfx::GfxError::ResourceExhausted => GfxError::ResourceExhausted,
        nexus_gfx::GfxError::Unsupported => GfxError::Unsupported,
        nexus_gfx::GfxError::InvalidArgument => GfxError::InvalidArgument,
        nexus_gfx::GfxError::MmioFault => GfxError::MmioFault,
    }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
const CTRL_QUEUE_INDEX: u32 = 0;
#[cfg(all(feature = "os-lite", target_os = "none"))]
const CURSOR_QUEUE_INDEX: u32 = 1;
#[cfg(all(feature = "os-lite", target_os = "none"))]
const QUEUE_LEN: usize = 4;
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_QUEUE_VA: usize = 0x2030_0000;
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_CMD_VA: usize = 0x2031_0000;
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_RESP_VA: usize = 0x2031_1000;
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_RESOURCE_BASE_VA: usize = 0x2040_0000;
#[cfg(all(feature = "os-lite", target_os = "none"))]
const GPU_RESOURCE_STRIDE: usize = 0x0100_0000;

#[cfg(all(feature = "os-lite", target_os = "none"))]
#[repr(C)]
#[derive(Clone, Copy)]
struct VqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
#[repr(C)]
struct VqAvail<const N: usize> {
    flags: u16,
    idx: u16,
    ring: [u16; N],
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
#[repr(C)]
#[derive(Clone, Copy)]
struct VqUsedElem {
    id: u32,
    len: u32,
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
#[repr(C)]
struct VqUsed<const N: usize> {
    flags: u16,
    idx: u16,
    ring: [VqUsedElem; N],
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
struct CtrlQueue {
    _queue_vmo: u32,
    _cmd_vmo: u32,
    _resp_vmo: u32,
    desc: *mut VqDesc,
    avail: *mut VqAvail<QUEUE_LEN>,
    used: *mut VqUsed<QUEUE_LEN>,
    cmd_va: usize,
    cmd_pa: u64,
    resp_va: usize,
    resp_pa: u64,
    last_used: u16,
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
impl VirtioGpuBackend {
    fn probe_os(&mut self) -> Result<(), GpuDriverError> {
        if read_reg(self.mmio_base, protocol::VIRTIO_MMIO_MAGIC_VALUE)
            != protocol::VIRTIO_MMIO_MAGIC
        {
            return Err(GpuDriverError::DeviceNotFound);
        }
        if read_reg(self.mmio_base, protocol::VIRTIO_MMIO_DEVICE_ID)
            != protocol::VIRTIO_GPU_DEVICE_ID
        {
            return Err(GpuDriverError::DeviceNotFound);
        }
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS, 0);
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS, 1 | 2);
        write_reg(self.mmio_base, 0x024, 0);
        write_reg(self.mmio_base, 0x020, 0);
        write_reg(self.mmio_base, 0x024, 1);
        write_reg(self.mmio_base, 0x020, 0);
        let status = read_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS);
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS, status | 8);
        if read_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS) & 8 == 0 {
            return Err(GpuDriverError::CommandRejected);
        }
        let ctrlq = CtrlQueue::new(self.mmio_base, CTRL_QUEUE_INDEX)?;
        self.ctrlq = Some(ctrlq);
        let status = read_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS);
        write_reg(self.mmio_base, protocol::VIRTIO_MMIO_STATUS, status | 4);
        Ok(())
    }

    fn create_resource_os(
        &mut self,
        id: ResourceId,
        w: u32,
        h: u32,
        fmt: PixelFormat,
        byte_len: usize,
    ) -> Result<(usize, u64, usize), GfxError> {
        let resource_index = self.resources.len();
        let backing_len = align_page(byte_len);
        let backing_va = GPU_RESOURCE_BASE_VA + resource_index * GPU_RESOURCE_STRIDE;
        let backing_vmo =
            nexus_abi::vmo_create(backing_len).map_err(|_| GfxError::ResourceExhausted)?;
        let flags = nexus_abi::page_flags::VALID
            | nexus_abi::page_flags::USER
            | nexus_abi::page_flags::READ
            | nexus_abi::page_flags::WRITE;
        for offset in (0..backing_len).step_by(4096) {
            nexus_abi::vmo_map_page(backing_vmo, backing_va + offset, offset, flags)
                .map_err(|_| GfxError::MmioFault)?;
        }
        unsafe { core::ptr::write_bytes(backing_va as *mut u8, 0, backing_len) };
        let mut info = nexus_abi::CapQuery {
            kind_tag: 0,
            reserved: 0,
            base: 0,
            len: 0,
        };
        nexus_abi::cap_query(backing_vmo, &mut info).map_err(|_| GfxError::MmioFault)?;

        let create = protocol::VirtioGpuResourceCreate2d {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_CREATE_RESOURCE_2D),
            resource_id: id.0,
            format: Self::to_gpu_format(fmt),
            width: w,
            height: h,
        };
        self.ctrl_submit_struct(&create)?;

        let attach = protocol::VirtioGpuResourceAttachBacking {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING),
            resource_id: id.0,
            nr_entries: 1,
        };
        let entry = protocol::VirtioGpuMemEntry {
            addr: info.base,
            length: byte_len as u32,
            _padding: 0,
        };
        self.ctrl_submit_pair(&attach, &entry)?;
        Ok((backing_va, info.base, backing_len))
    }

    fn transfer_to_host_os(&mut self, record: ResourceRecord, rect: Rect) -> Result<(), GfxError> {
        let cmd = protocol::VirtioGpuTransferToHost2d {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D),
            r: protocol::VirtioGpuRect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
            },
            offset: 0,
            resource_id: record.id.0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&cmd)
    }

    fn set_scanout_os(&mut self, record: ResourceRecord) -> Result<(), GfxError> {
        let cmd = protocol::VirtioGpuSetScanout {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_SET_SCANOUT),
            r: protocol::VirtioGpuRect {
                x: 0,
                y: 0,
                width: record.width,
                height: record.height,
            },
            scanout_id: 0,
            resource_id: record.id.0,
        };
        self.ctrl_submit_struct(&cmd)
    }

    fn move_cursor_os(&mut self, x: u32, y: u32) -> Result<(), GfxError> {
        let cmd = protocol::VirtioGpuCursorPos {
            hdr: ctrl_hdr(protocol::VIRTIO_GPU_CMD_MOVE_CURSOR),
            pos: protocol::VirtioGpuCursorPosData {
                scanout_id: 0,
                x,
                y,
                _padding: 0,
            },
            resource_id: 0,
            hot_x: 0,
            hot_y: 0,
            _padding: 0,
        };
        self.ctrl_submit_struct(&cmd)
    }

    fn ctrl_submit_struct<T>(&mut self, cmd: &T) -> Result<(), GfxError> {
        let bytes = unsafe {
            core::slice::from_raw_parts((cmd as *const T).cast::<u8>(), core::mem::size_of::<T>())
        };
        self.ctrl_submit_bytes(bytes)
    }

    fn ctrl_submit_pair<A, B>(&mut self, a: &A, b: &B) -> Result<(), GfxError> {
        let a_bytes = unsafe {
            core::slice::from_raw_parts((a as *const A).cast::<u8>(), core::mem::size_of::<A>())
        };
        let b_bytes = unsafe {
            core::slice::from_raw_parts((b as *const B).cast::<u8>(), core::mem::size_of::<B>())
        };
        let queue = self.ctrlq.as_mut().ok_or(GfxError::DeviceNotFound)?;
        queue.submit_two(self.mmio_base, a_bytes, b_bytes)
    }

    fn ctrl_submit_bytes(&mut self, bytes: &[u8]) -> Result<(), GfxError> {
        let queue = self.ctrlq.as_mut().ok_or(GfxError::DeviceNotFound)?;
        queue.submit(self.mmio_base, bytes)
    }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
impl CtrlQueue {
    fn new(mmio_base: usize, queue_index: u32) -> Result<Self, GpuDriverError> {
        let q_vmo = nexus_abi::vmo_create(4096).map_err(|_| GpuDriverError::MmioFault)?;
        let cmd_vmo = nexus_abi::vmo_create(4096).map_err(|_| GpuDriverError::MmioFault)?;
        let resp_vmo = nexus_abi::vmo_create(4096).map_err(|_| GpuDriverError::MmioFault)?;
        let flags = nexus_abi::page_flags::VALID
            | nexus_abi::page_flags::USER
            | nexus_abi::page_flags::READ
            | nexus_abi::page_flags::WRITE;
        nexus_abi::vmo_map_page(q_vmo, GPU_QUEUE_VA, 0, flags)
            .map_err(|_| GpuDriverError::MmioFault)?;
        nexus_abi::vmo_map_page(cmd_vmo, GPU_CMD_VA, 0, flags)
            .map_err(|_| GpuDriverError::MmioFault)?;
        nexus_abi::vmo_map_page(resp_vmo, GPU_RESP_VA, 0, flags)
            .map_err(|_| GpuDriverError::MmioFault)?;
        let mut q_info = nexus_abi::CapQuery {
            kind_tag: 0,
            reserved: 0,
            base: 0,
            len: 0,
        };
        let mut cmd_info = nexus_abi::CapQuery {
            kind_tag: 0,
            reserved: 0,
            base: 0,
            len: 0,
        };
        let mut resp_info = nexus_abi::CapQuery {
            kind_tag: 0,
            reserved: 0,
            base: 0,
            len: 0,
        };
        nexus_abi::cap_query(q_vmo, &mut q_info).map_err(|_| GpuDriverError::MmioFault)?;
        nexus_abi::cap_query(cmd_vmo, &mut cmd_info).map_err(|_| GpuDriverError::MmioFault)?;
        nexus_abi::cap_query(resp_vmo, &mut resp_info).map_err(|_| GpuDriverError::MmioFault)?;
        unsafe {
            core::ptr::write_bytes(GPU_QUEUE_VA as *mut u8, 0, 4096);
            core::ptr::write_bytes(GPU_CMD_VA as *mut u8, 0, 4096);
            core::ptr::write_bytes(GPU_RESP_VA as *mut u8, 0, 4096);
        }

        let desc_bytes = core::mem::size_of::<VqDesc>() * QUEUE_LEN;
        let avail_bytes = core::mem::size_of::<VqAvail<QUEUE_LEN>>();
        let used_off = align4(desc_bytes + avail_bytes);
        let desc_va = GPU_QUEUE_VA;
        let avail_va = GPU_QUEUE_VA + desc_bytes;
        let used_va = GPU_QUEUE_VA + used_off;

        write_reg(mmio_base, protocol::VIRTIO_MMIO_QUEUE_SEL, queue_index);
        let max = read_reg(mmio_base, protocol::VIRTIO_MMIO_QUEUE_NUM_MAX);
        if max < QUEUE_LEN as u32 {
            return Err(GpuDriverError::ResourceExhausted);
        }
        write_reg(mmio_base, protocol::VIRTIO_MMIO_QUEUE_NUM, QUEUE_LEN as u32);
        write_u64_pair(mmio_base, protocol::VIRTIO_MMIO_QUEUE_DESC_LOW, q_info.base);
        write_u64_pair(
            mmio_base,
            protocol::VIRTIO_MMIO_QUEUE_DRIVER_LOW,
            q_info.base + desc_bytes as u64,
        );
        write_u64_pair(
            mmio_base,
            protocol::VIRTIO_MMIO_QUEUE_DEVICE_LOW,
            q_info.base + used_off as u64,
        );
        write_reg(mmio_base, protocol::VIRTIO_MMIO_QUEUE_READY, 1);

        Ok(Self {
            _queue_vmo: q_vmo,
            _cmd_vmo: cmd_vmo,
            _resp_vmo: resp_vmo,
            desc: desc_va as *mut VqDesc,
            avail: avail_va as *mut VqAvail<QUEUE_LEN>,
            used: used_va as *mut VqUsed<QUEUE_LEN>,
            cmd_va: GPU_CMD_VA,
            cmd_pa: cmd_info.base,
            resp_va: GPU_RESP_VA,
            resp_pa: resp_info.base,
            last_used: 0,
        })
    }

    fn submit(&mut self, mmio_base: usize, bytes: &[u8]) -> Result<(), GfxError> {
        self.submit_two(mmio_base, bytes, &[])
    }

    fn submit_two(
        &mut self,
        mmio_base: usize,
        first: &[u8],
        second: &[u8],
    ) -> Result<(), GfxError> {
        let total = first
            .len()
            .checked_add(second.len())
            .ok_or(GfxError::ResourceExhausted)?;
        if total == 0 || total > 4096 || core::mem::size_of::<protocol::VirtioGpuCtrlHdr>() > total
        {
            return Err(GfxError::CommandRejected);
        }
        unsafe {
            core::ptr::write_bytes(self.cmd_va as *mut u8, 0, 4096);
            core::ptr::write_bytes(self.resp_va as *mut u8, 0, 4096);
            core::ptr::copy_nonoverlapping(first.as_ptr(), self.cmd_va as *mut u8, first.len());
            if !second.is_empty() {
                core::ptr::copy_nonoverlapping(
                    second.as_ptr(),
                    (self.cmd_va + first.len()) as *mut u8,
                    second.len(),
                );
            }
            core::ptr::write_volatile(
                self.desc.add(0),
                VqDesc {
                    addr: self.cmd_pa,
                    len: total as u32,
                    flags: 1,
                    next: 1,
                },
            );
            core::ptr::write_volatile(
                self.desc.add(1),
                VqDesc {
                    addr: self.resp_pa,
                    len: core::mem::size_of::<protocol::VirtioGpuCtrlHdr>() as u32,
                    flags: 2,
                    next: 0,
                },
            );
            let idx = core::ptr::read_volatile(&(*self.avail).idx);
            core::ptr::write_volatile(&mut (*self.avail).ring[(idx as usize) % QUEUE_LEN], 0);
            core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
            core::ptr::write_volatile(&mut (*self.avail).idx, idx.wrapping_add(1));
        }
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        write_reg(
            mmio_base,
            protocol::VIRTIO_MMIO_QUEUE_NOTIFY,
            CTRL_QUEUE_INDEX,
        );
        self.wait_complete()
    }

    fn wait_complete(&mut self) -> Result<(), GfxError> {
        let start = nexus_abi::nsec().map_err(|_| GfxError::MmioFault)?;
        let deadline = start.saturating_add(500_000_000);
        loop {
            let used_idx = unsafe { core::ptr::read_volatile(&(*self.used).idx) };
            if used_idx != self.last_used {
                self.last_used = used_idx;
                let hdr = unsafe {
                    core::ptr::read_volatile(self.resp_va as *const protocol::VirtioGpuCtrlHdr)
                };
                if hdr.type_ == protocol::VIRTIO_GPU_RESP_OK_NODATA {
                    return Ok(());
                }
                return Err(GfxError::CommandRejected);
            }
            let now = nexus_abi::nsec().map_err(|_| GfxError::MmioFault)?;
            if now >= deadline {
                return Err(GfxError::MmioFault);
            }
            let _ = nexus_abi::yield_();
        }
    }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn ctrl_hdr(type_: u32) -> protocol::VirtioGpuCtrlHdr {
    protocol::VirtioGpuCtrlHdr {
        type_,
        flags: 0,
        fence_id: 0,
        ctx_id: 0,
        _padding: 0,
    }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn read_reg(base: usize, offset: usize) -> u32 {
    unsafe { core::ptr::read_volatile((base + offset) as *const u32) }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn write_reg(base: usize, offset: usize, value: u32) {
    unsafe { core::ptr::write_volatile((base + offset) as *mut u32, value) }
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
fn write_u64_pair(base: usize, low_reg: usize, value: u64) {
    write_reg(base, low_reg, value as u32);
    write_reg(base, low_reg + 4, (value >> 32) as u32);
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
const fn align4(value: usize) -> usize {
    (value + 3) & !3
}

#[cfg(all(feature = "os-lite", target_os = "none"))]
const fn align_page(value: usize) -> usize {
    (value + 4095) & !4095
}
