// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests for gfx_backend::cpu_mock.
//! OWNERS: @ui @runtime
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md

use gfx_backend::{CpuMockBackend, GfxBackend, GfxError};
use nexus_gfx::PixelFormat;

#[test]
fn create_resource_rejects_zero() {
    let mut b = CpuMockBackend::new(64, 64);
    assert!(b.create_resource(0, 64, PixelFormat::Bgra8888).is_err());
}
#[test]
fn create_resource_succeeds() {
    let mut b = CpuMockBackend::new(64, 64);
    assert_eq!(
        b.create_resource(32, 32, PixelFormat::Bgra8888).unwrap().0,
        1
    );
}
#[test]
fn submit_empty_succeeds() {
    let mut b = CpuMockBackend::new(64, 64);
    use nexus_gfx::CommandBuffer;
    let empty = CommandBuffer::new().commit();
    assert!(b.submit(empty).unwrap().signaled());
}
#[test]
fn draw_tiles_modifies_framebuffer() {
    let mut b = CpuMockBackend::new(64, 64);
    use nexus_gfx::CommandBuffer;
    use nexus_gfx::RenderPassDesc;
    use nexus_gfx::TileRect;
    let mut cmd = CommandBuffer::new();
    {
        let mut enc = cmd.begin_render_pass(RenderPassDesc {
            color_attachments: vec![],
            width: 64,
            height: 64,
        });
        enc.draw_tiles(&[TileRect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        }]);
        enc.end_encoding();
    }
    b.submit(cmd.commit()).unwrap();
    // Framebuffer should have white pixels in top-left 10x10
    assert_eq!(b.framebuffer()[0], 0xff);
    assert_eq!(b.framebuffer()[1], 0xff);
    assert_eq!(b.framebuffer()[2], 0xff);
    assert_eq!(b.framebuffer()[3], 0xff);
}

#[test]
fn test_reject_transfer_outside_resource_bounds() {
    let mut b = CpuMockBackend::new(64, 64);
    let resource = b.create_resource(16, 16, PixelFormat::Bgra8888).unwrap();
    let err = b
        .transfer_to_host(
            resource,
            gfx_backend::types::Rect {
                x: 12,
                y: 0,
                width: 8,
                height: 8,
            },
        )
        .err();
    assert_eq!(err, Some(GfxError::InvalidArgument));
}

#[test]
fn test_reject_unknown_scanout_resource() {
    let mut b = CpuMockBackend::new(64, 64);
    let err = b.set_scanout(gfx_backend::types::ResourceId(99)).err();
    assert_eq!(err, Some(GfxError::InvalidArgument));
}
