// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

/// virtio-gpu device ID (MMIO probe).
pub const VIRTIO_GPU_DEVICE_ID: u32 = 16;

/// virtio MMIO register offsets.
pub const VIRTIO_MMIO_MAGIC_VALUE: usize = 0x000;
pub const VIRTIO_MMIO_VERSION: usize = 0x004;
pub const VIRTIO_MMIO_DEVICE_ID: usize = 0x008;
pub const VIRTIO_MMIO_VENDOR_ID: usize = 0x00c;
pub const VIRTIO_MMIO_QUEUE_SEL: usize = 0x030;
pub const VIRTIO_MMIO_QUEUE_NUM_MAX: usize = 0x034;
pub const VIRTIO_MMIO_QUEUE_NUM: usize = 0x038;
pub const VIRTIO_MMIO_QUEUE_ALIGN: usize = 0x03c;
pub const VIRTIO_MMIO_QUEUE_PFN: usize = 0x040;
pub const VIRTIO_MMIO_QUEUE_READY: usize = 0x044;
pub const VIRTIO_MMIO_QUEUE_NOTIFY: usize = 0x050;
pub const VIRTIO_MMIO_STATUS: usize = 0x070;
pub const VIRTIO_MMIO_QUEUE_DESC_LOW: usize = 0x080;
pub const VIRTIO_MMIO_QUEUE_DESC_HIGH: usize = 0x084;
pub const VIRTIO_MMIO_QUEUE_DRIVER_LOW: usize = 0x090;
pub const VIRTIO_MMIO_QUEUE_DRIVER_HIGH: usize = 0x094;
pub const VIRTIO_MMIO_QUEUE_DEVICE_LOW: usize = 0x0a0;
pub const VIRTIO_MMIO_QUEUE_DEVICE_HIGH: usize = 0x0a4;

/// virtio MMIO magic value ("virt").
pub const VIRTIO_MMIO_MAGIC: u32 = 0x74726976;

/// virtio-gpu command types (per virtio-gpu spec §5.7.6.3).
pub const VIRTIO_GPU_CMD_GET_DISPLAY_INFO: u32 = 0x0100;
pub const VIRTIO_GPU_CMD_CREATE_RESOURCE_2D: u32 = 0x0101;
pub const VIRTIO_GPU_CMD_RESOURCE_UNREF: u32 = 0x0102;
pub const VIRTIO_GPU_CMD_SET_SCANOUT: u32 = 0x0103;
pub const VIRTIO_GPU_CMD_RESOURCE_FLUSH: u32 = 0x0104;
pub const VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
pub const VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
pub const VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING: u32 = 0x0107;
pub const VIRTIO_GPU_CMD_UPDATE_CURSOR: u32 = 0x0300;
pub const VIRTIO_GPU_CMD_MOVE_CURSOR: u32 = 0x0301;
pub const VIRTIO_GPU_RESP_OK_NODATA: u32 = 0x1100;
pub const VIRTIO_GPU_RESP_ERR_UNSPEC: u32 = 0x1200;

/// virtio-gpu pixel format constants (per virtio-gpu spec §5.7.6.1).
pub const VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM: u32 = 1;
pub const VIRTIO_GPU_FORMAT_R8G8B8A8_UNORM: u32 = 67;

/// virtio-gpu control header (24 bytes: 4+4+8+4+4).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioGpuCtrlHdr {
    pub type_: u32,
    pub flags: u32,
    pub fence_id: u64,
    pub ctx_id: u32,
    pub _padding: u32,
}

/// CREATE_RESOURCE_2D command.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioGpuResourceCreate2d {
    pub hdr: VirtioGpuCtrlHdr,
    pub resource_id: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
}

/// ATTACH_BACKING command header + memory entries.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioGpuResourceAttachBacking {
    pub hdr: VirtioGpuCtrlHdr,
    pub resource_id: u32,
    pub nr_entries: u32,
}

/// Memory entry for attach_backing.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioGpuMemEntry {
    pub addr: u64,
    pub length: u32,
    pub _padding: u32,
}

/// TRANSFER_TO_HOST_2D command.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioGpuTransferToHost2d {
    pub hdr: VirtioGpuCtrlHdr,
    pub r: VirtioGpuRect,
    pub offset: u64,
    pub resource_id: u32,
    pub _padding: u32,
}

/// Rectangle used in virtio-gpu commands.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioGpuRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// SET_SCANOUT command.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioGpuSetScanout {
    pub hdr: VirtioGpuCtrlHdr,
    pub r: VirtioGpuRect,
    pub scanout_id: u32,
    pub resource_id: u32,
}

/// MOVE_CURSOR command.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioGpuCursorPos {
    pub hdr: VirtioGpuCtrlHdr,
    pub pos: VirtioGpuCursorPosData,
    pub resource_id: u32,
    pub hot_x: u32,
    pub hot_y: u32,
    pub _padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioGpuCursorPosData {
    pub scanout_id: u32,
    pub x: u32,
    pub y: u32,
    pub _padding: u32,
}

/// UPDATE_CURSOR command — sets the cursor image resource and hotspot.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioGpuUpdateCursor {
    pub hdr: VirtioGpuCtrlHdr,
    pub pos: VirtioGpuCursorPosData,
    pub resource_id: u32,
    pub hot_x: u32,
    pub hot_y: u32,
    pub _padding: u32,
}

/// RESOURCE_FLUSH command.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VirtioGpuResourceFlush {
    pub hdr: VirtioGpuCtrlHdr,
    pub r: VirtioGpuRect,
    pub resource_id: u32,
    pub _padding: u64,
}