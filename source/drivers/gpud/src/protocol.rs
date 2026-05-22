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
pub const VIRTIO_MMIO_QUEUE_NOTIFY: usize = 0x050;
pub const VIRTIO_MMIO_STATUS: usize = 0x070;

/// virtio MMIO magic value ("virt").
pub const VIRTIO_MMIO_MAGIC: u32 = 0x74726976;

/// virtio-gpu command types.
pub const VIRTIO_GPU_CMD_CREATE_RESOURCE_2D: u32 = 0x0102;
pub const VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
pub const VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D: u32 = 0x0108;
pub const VIRTIO_GPU_CMD_SET_SCANOUT: u32 = 0x010a;
pub const VIRTIO_GPU_CMD_UPDATE_CURSOR: u32 = 0x0301;
pub const VIRTIO_GPU_CMD_MOVE_CURSOR: u32 = 0x0302;

/// virtio-gpu pixel format constants.
pub const VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM: u32 = 0x0100;
pub const VIRTIO_GPU_FORMAT_R8G8B8A8_UNORM: u32 = 0x0101;

/// virtio-gpu control header (8 * 4 = 32 bytes).
#[repr(C)]
pub struct VirtioGpuCtrlHdr {
    pub type_: u32,
    pub flags: u32,
    pub fence_id: u64,
    pub ctx_id: u32,
    pub _padding: u32,
}

/// CREATE_RESOURCE_2D command.
#[repr(C)]
pub struct VirtioGpuResourceCreate2d {
    pub hdr: VirtioGpuCtrlHdr,
    pub resource_id: u32,
    pub format: u32,
    pub width: u32,
    pub height: u32,
}

/// ATTACH_BACKING command header + memory entries.
#[repr(C)]
pub struct VirtioGpuResourceAttachBacking {
    pub hdr: VirtioGpuCtrlHdr,
    pub resource_id: u32,
    pub nr_entries: u32,
}

/// Memory entry for attach_backing.
#[repr(C)]
pub struct VirtioGpuMemEntry {
    pub addr: u64,
    pub length: u32,
    pub _padding: u32,
}

/// TRANSFER_TO_HOST_2D command.
#[repr(C)]
pub struct VirtioGpuTransferToHost2d {
    pub hdr: VirtioGpuCtrlHdr,
    pub r: VirtioGpuRect,
    pub offset: u64,
    pub resource_id: u32,
    pub _padding: u32,
}

/// Rectangle used in virtio-gpu commands.
#[repr(C)]
pub struct VirtioGpuRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// SET_SCANOUT command.
#[repr(C)]
pub struct VirtioGpuSetScanout {
    pub hdr: VirtioGpuCtrlHdr,
    pub r: VirtioGpuRect,
    pub scanout_id: u32,
    pub resource_id: u32,
}

/// MOVE_CURSOR command.
#[repr(C)]
pub struct VirtioGpuCursorPos {
    pub hdr: VirtioGpuCtrlHdr,
    pub pos: VirtioGpuCursorPosData,
    pub resource_id: u32,
    pub hot_x: u32,
    pub hot_y: u32,
    pub _padding: u32,
}

#[repr(C)]
pub struct VirtioGpuCursorPosData {
    pub scanout_id: u32,
    pub x: u32,
    pub y: u32,
    pub _padding: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem;

    #[test]
    fn ctrl_hdr_size() {
        // virtio-gpu spec: 4+4+8+4+4 = 24 bytes
        assert_eq!(mem::size_of::<VirtioGpuCtrlHdr>(), 24);
    }

    #[test]
    fn create_resource_2d_size() {
        assert_eq!(mem::size_of::<VirtioGpuResourceCreate2d>(), 40);
    }

    #[test]
    fn set_scanout_size() {
        assert_eq!(mem::size_of::<VirtioGpuSetScanout>(), 48);
    }

    #[test]
    fn move_cursor_size() {
        // 24 (hdr) + 16 (pos) + 4 + 4 + 4 + 4 = 56
        assert_eq!(mem::size_of::<VirtioGpuCursorPos>(), 56);
    }

    #[test]
    fn device_id_is_16() {
        assert_eq!(VIRTIO_GPU_DEVICE_ID, 16);
    }

    #[test]
    fn magic_value() {
        assert_eq!(VIRTIO_MMIO_MAGIC, 0x74726976); // "virt"
    }
}
