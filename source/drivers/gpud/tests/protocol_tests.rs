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
