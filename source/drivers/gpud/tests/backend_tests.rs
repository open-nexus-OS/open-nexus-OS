// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests for gpud::backend.
//! OWNERS: @ui @runtime
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md

use nexus_gfx::backend::traits::GfxBackend;
use gpud::backend::VirtioGpuBackend;
use nexus_gfx::PixelFormat;

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

#[test]
fn transfer_rejects_out_of_bounds_rect() {
    let mut b = VirtioGpuBackend::new(0x10008000, 0x200);
    b.probe().unwrap();
    let resource = b.create_resource(16, 16, PixelFormat::Bgra8888).unwrap();
    let err = b.transfer_to_host(
        resource,
        nexus_gfx::backend::types::Rect {
            x: 12,
            y: 0,
            width: 8,
            height: 8,
        },
    );
    assert!(err.is_err());
}
