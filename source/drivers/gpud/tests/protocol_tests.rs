// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests for gpud::protocol.
//! OWNERS: @ui @runtime
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md

use core::mem;
use gpud::protocol::*;

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

// ---------------------------------------------------------------------------
// Spec validation: virtio-gpu protocol constants must match the virtio-gpu
// specification (Linux include/uapi/linux/virtio_gpu.h). These tests prevent
// copy-paste errors like the 0x0100/0x0101 format-constant bug that caused
// CREATE_RESOURCE_2D to be silently rejected by QEMU.
// ---------------------------------------------------------------------------

#[test]
fn command_types_match_spec() {
    // virtio-gpu spec §5.7.6.3
    assert_eq!(VIRTIO_GPU_CMD_CREATE_RESOURCE_2D, 0x0101);
    assert_eq!(VIRTIO_GPU_CMD_RESOURCE_UNREF, 0x0102);
    assert_eq!(VIRTIO_GPU_CMD_SET_SCANOUT, 0x0103);
    assert_eq!(VIRTIO_GPU_CMD_RESOURCE_FLUSH, 0x0104);
    assert_eq!(VIRTIO_GPU_CMD_TRANSFER_TO_HOST_2D, 0x0105);
    assert_eq!(VIRTIO_GPU_CMD_RESOURCE_ATTACH_BACKING, 0x0106);
    assert_eq!(VIRTIO_GPU_CMD_RESOURCE_DETACH_BACKING, 0x0107);
    assert_eq!(VIRTIO_GPU_CMD_UPDATE_CURSOR, 0x0300);
    assert_eq!(VIRTIO_GPU_CMD_MOVE_CURSOR, 0x0301);
}

#[test]
fn response_types_match_spec() {
    assert_eq!(VIRTIO_GPU_RESP_OK_NODATA, 0x1100);
    assert_eq!(VIRTIO_GPU_RESP_ERR_UNSPEC, 0x1200);
}

#[test]
fn format_constants_match_spec() {
    // virtio-gpu spec enum virtio_gpu_formats
    // B8G8R8A8 = 1, R8G8B8A8 = 67
    assert_eq!(VIRTIO_GPU_FORMAT_B8G8R8A8_UNORM, 1);
    assert_eq!(VIRTIO_GPU_FORMAT_R8G8B8A8_UNORM, 67);
}

#[test]
fn mmio_register_offsets_match_spec() {
    // virtio-mmio spec §4.2.2
    assert_eq!(VIRTIO_MMIO_MAGIC_VALUE, 0x000);
    assert_eq!(VIRTIO_MMIO_VERSION, 0x004);
    assert_eq!(VIRTIO_MMIO_DEVICE_ID, 0x008);
    assert_eq!(VIRTIO_MMIO_VENDOR_ID, 0x00c);
    assert_eq!(VIRTIO_MMIO_QUEUE_SEL, 0x030);
    assert_eq!(VIRTIO_MMIO_QUEUE_NUM_MAX, 0x034);
    assert_eq!(VIRTIO_MMIO_QUEUE_NUM, 0x038);
    assert_eq!(VIRTIO_MMIO_QUEUE_READY, 0x044);
    assert_eq!(VIRTIO_MMIO_QUEUE_NOTIFY, 0x050);
    assert_eq!(VIRTIO_MMIO_STATUS, 0x070);
}

#[test]
fn device_id_match_spec() {
    // virtio spec: GPU device id = 16
    assert_eq!(VIRTIO_GPU_DEVICE_ID, 16);
}

// ---------------------------------------------------------------------------
// Struct size validation: repr(C) sizes must match the wire format expected
// by QEMU's virtio-gpu device implementation.
// ---------------------------------------------------------------------------

#[test]
fn attach_backing_size() {
    // 24 (hdr) + 4 + 4 = 32
    assert_eq!(mem::size_of::<VirtioGpuResourceAttachBacking>(), 32);
}

#[test]
fn mem_entry_size() {
    // 8 (addr) + 4 (length) + 4 (padding) = 16
    assert_eq!(mem::size_of::<VirtioGpuMemEntry>(), 16);
}

#[test]
fn transfer_to_host_2d_size() {
    // 24 (hdr) + 16 (rect) + 8 (offset) + 4 + 4 = 56
    assert_eq!(mem::size_of::<VirtioGpuTransferToHost2d>(), 56);
}

#[test]
fn rect_size() {
    // 4 × u32 = 16
    assert_eq!(mem::size_of::<VirtioGpuRect>(), 16);
}

#[test]
fn cursor_pos_data_size() {
    // 4 × u32 = 16
    assert_eq!(mem::size_of::<VirtioGpuCursorPosData>(), 16);
}
