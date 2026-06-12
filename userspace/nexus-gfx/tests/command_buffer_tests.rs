// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests for nexus_gfx::command::buffer.
//! OWNERS: @ui @runtime
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md

use nexus_gfx::{
    CommandBuffer, CommittedBuffer, GfxError, Queue, RenderPassDesc, RgbaColor, TileRect,
};

/// Record a representative GPU-first scene CB (the windowd present frame:
/// blit + glass blur/fill + cursor). Clears first so the buffer is reusable.
fn record_scene(cb: &mut CommandBuffer) {
    cb.clear();
    let mut enc = cb
        .try_begin_render_pass(RenderPassDesc {
            color_attachments: vec![],
            width: 1280,
            height: 800,
        })
        .unwrap();
    enc.try_blit_surface(0, 800, 0, 0, 1280, 64).unwrap();
    let btn = TileRect { x: 1100, y: 24, width: 156, height: 56 };
    enc.try_blur_backdrop(btn, 8, 120).unwrap();
    enc.try_fill_sdf_rounded_rect(btn, 18, RgbaColor::new(235, 245, 255, 200)).unwrap();
    enc.try_fill_sdf_rounded_rect(btn, 18, RgbaColor::new(255, 255, 255, 84)).unwrap();
    enc.try_blend_cursor(100, 100, 24, 24).unwrap();
    enc.end_encoding();
}

/// Regression guard for the `alloc-fail svc=windowd` animation crash: windowd
/// reuses ONE CommandBuffer per frame (clear + record + serialize_into) because
/// its bump allocator never frees. After warmup, recording another frame must
/// not reallocate the command vector — otherwise every animation frame would
/// leak and exhaust the heap.
#[test]
fn command_buffer_reuse_does_not_reallocate() {
    let mut cb = CommandBuffer::new();
    let mut wire = [0u8; 2048];

    record_scene(&mut cb);
    let warm_cap = cb.command_capacity();
    let first_len = cb.serialize_into(&mut wire).unwrap();
    assert!(first_len > 0);

    for _ in 0..1000 {
        record_scene(&mut cb);
        let n = cb.serialize_into(&mut wire).unwrap();
        assert_eq!(n, first_len, "deterministic scene must serialize to a stable size");
        assert_eq!(
            cb.command_capacity(),
            warm_cap,
            "reused CommandBuffer must not grow its allocation per frame"
        );
    }
}

/// Regression guard for the `alloc-fail svc=gpud` crash: gpud reuses ONE
/// CommittedBuffer per present (`reload_from`) instead of `deserialize_from`,
/// because it too runs on a non-freeing bump allocator. Re-parsing a frame must
/// reuse the command vector, not allocate a fresh one each time.
#[test]
fn committed_buffer_reload_does_not_reallocate() {
    // Produce a representative serialized present frame.
    let mut src = CommandBuffer::new();
    record_scene(&mut src);
    let mut wire = [0u8; 2048];
    let len = src.serialize_into(&mut wire).unwrap();

    let mut scene = CommittedBuffer::with_capacity(32);
    scene.reload_from(&wire[..len]).unwrap();
    let warm_cap = scene.command_capacity();
    let warm_count = scene.command_count();
    assert!(warm_count > 0);

    for _ in 0..1000 {
        let consumed = scene.reload_from(&wire[..len]).unwrap();
        assert_eq!(consumed, len);
        assert_eq!(scene.command_count(), warm_count);
        assert_eq!(
            scene.command_capacity(),
            warm_cap,
            "reused CommittedBuffer must not grow its allocation per present"
        );
    }

    // And the reused buffer must decode identically to a fresh deserialize.
    let (fresh, _) = CommittedBuffer::deserialize_from(&wire[..len]).unwrap();
    assert_eq!(scene, fresh);
}

#[test]
fn committed_buffer_is_sealed() {
    let mut cmd = CommandBuffer::new();
    {
        let mut enc = cmd.begin_render_pass(RenderPassDesc {
            color_attachments: vec![],
            width: 64,
            height: 64,
        });
        enc.set_fragment_bytes(0, &[1, 2, 3, 4]);
        enc.draw_tiles(
            &[TileRect { x: 0, y: 0, width: 10, height: 10 }],
            nexus_gfx::command::buffer::RgbaColor::from_u32(0xFFFF_FFFF),
        );
        enc.end_encoding();
    }
    let committed = cmd.commit();
    assert_eq!(committed.command_count(), 2);
}

#[test]
fn command_buffer_deterministic() {
    let mut a = CommandBuffer::new();
    let mut b = CommandBuffer::new();
    {
        let mut ea = a.begin_render_pass(RenderPassDesc {
            color_attachments: vec![],
            width: 64,
            height: 64,
        });
        let mut eb = b.begin_render_pass(RenderPassDesc {
            color_attachments: vec![],
            width: 64,
            height: 64,
        });
        ea.set_fragment_bytes(0, &[1, 2, 3]);
        eb.set_fragment_bytes(0, &[1, 2, 3]);
        ea.end_encoding();
        eb.end_encoding();
    }
    assert_eq!(a.commit(), b.commit());
}

#[test]
fn test_reject_invalid_render_pass_extent() {
    let mut cmd = CommandBuffer::new();
    let err = cmd
        .try_begin_render_pass(RenderPassDesc { color_attachments: vec![], width: 0, height: 64 })
        .err();
    assert_eq!(err, Some(GfxError::InvalidArgument));
}

#[test]
fn test_reject_tile_outside_render_pass() {
    let mut cmd = CommandBuffer::new();
    let mut enc = cmd
        .try_begin_render_pass(RenderPassDesc { color_attachments: vec![], width: 64, height: 64 })
        .unwrap();
    let err = enc
        .try_draw_tiles(
            &[TileRect { x: 60, y: 0, width: 8, height: 8 }],
            nexus_gfx::command::buffer::RgbaColor::from_u32(0xFFFF_FFFF),
        )
        .err();
    assert_eq!(err, Some(GfxError::InvalidArgument));
}

#[test]
fn test_reject_fragment_bytes_over_budget() {
    let mut cmd = CommandBuffer::new();
    let mut enc = cmd
        .try_begin_render_pass(RenderPassDesc { color_attachments: vec![], width: 64, height: 64 })
        .unwrap();
    let bytes = vec![0u8; nexus_gfx::command::buffer::MAX_FRAGMENT_BYTES + 1];
    let err = enc.try_set_fragment_bytes(0, &bytes).err();
    assert_eq!(err, Some(GfxError::ResourceExhausted));
}

#[test]
fn queue_submit_validates_committed_buffers() {
    let mut cmd = CommandBuffer::new();
    {
        let mut enc = cmd
            .try_begin_render_pass(RenderPassDesc {
                color_attachments: vec![],
                width: 64,
                height: 64,
            })
            .unwrap();
        enc.try_draw_tiles(
            &[TileRect { x: 0, y: 0, width: 8, height: 8 }],
            nexus_gfx::command::buffer::RgbaColor::from_u32(0xFFFF_FFFF),
        )
        .unwrap();
    }
    let mut queue = Queue::with_depth(1).unwrap();
    assert!(queue.submit(cmd).unwrap().signaled());
    assert_eq!(queue.in_flight(), 0);
}

#[test]
fn test_reject_zero_queue_depth() {
    assert_eq!(Queue::with_depth(0).err(), Some(GfxError::InvalidArgument));
}

#[test]
fn committed_buffer_serialize_round_trip() {
    let mut cmd = CommandBuffer::new();
    {
        let mut enc = cmd.begin_render_pass(RenderPassDesc {
            color_attachments: vec![],
            width: 1280,
            height: 800,
        });
        enc.set_fragment_bytes(0, &[0u8; 16]);
        enc.draw_tiles(
            &[
                TileRect { x: 960, y: 0, width: 320, height: 800 },
                TileRect { x: 1100, y: 24, width: 156, height: 56 },
            ],
            nexus_gfx::command::buffer::RgbaColor::from_u32(0xFFFF_FFFF),
        );
        enc.end_encoding();
    }
    let committed = cmd.commit();
    assert_eq!(committed.command_count(), 2);

    // Serialize
    let mut buf = [0u8; 256];
    let written = committed.serialize_into(&mut buf).unwrap();
    assert!(written > 0);
    assert!(written <= 256);

    // Deserialize
    let (restored, consumed) = CommittedBuffer::deserialize_from(&buf[..written]).unwrap();
    assert_eq!(consumed, written);
    assert_eq!(restored.command_count(), 2);

    // Verify commands match
    assert_eq!(restored, committed);
}

#[test]
fn committed_buffer_serialize_reject_buffer_too_small() {
    let mut cmd = CommandBuffer::new();
    {
        let mut enc = cmd.begin_render_pass(RenderPassDesc {
            color_attachments: vec![],
            width: 64,
            height: 64,
        });
        enc.set_fragment_bytes(0, &[1, 2, 3, 4]);
        enc.end_encoding();
    }
    let committed = cmd.commit();
    let mut buf = [0u8; 2];
    assert!(committed.serialize_into(&mut buf).is_err());
}
