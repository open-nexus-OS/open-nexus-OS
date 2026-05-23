// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests for nexus_gfx::command_buffer.
//! OWNERS: @ui @runtime
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md

use nexus_gfx::{CommandBuffer, GfxError, Queue, RenderPassDesc, TileRect};

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
        enc.draw_tiles(&[TileRect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        }]);
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
        .try_begin_render_pass(RenderPassDesc {
            color_attachments: vec![],
            width: 0,
            height: 64,
        })
        .err();
    assert_eq!(err, Some(GfxError::InvalidArgument));
}

#[test]
fn test_reject_tile_outside_render_pass() {
    let mut cmd = CommandBuffer::new();
    let mut enc = cmd
        .try_begin_render_pass(RenderPassDesc {
            color_attachments: vec![],
            width: 64,
            height: 64,
        })
        .unwrap();
    let err = enc
        .try_draw_tiles(&[TileRect {
            x: 60,
            y: 0,
            width: 8,
            height: 8,
        }])
        .err();
    assert_eq!(err, Some(GfxError::InvalidArgument));
}

#[test]
fn test_reject_fragment_bytes_over_budget() {
    let mut cmd = CommandBuffer::new();
    let mut enc = cmd
        .try_begin_render_pass(RenderPassDesc {
            color_attachments: vec![],
            width: 64,
            height: 64,
        })
        .unwrap();
    let bytes = vec![0u8; nexus_gfx::command_buffer::MAX_FRAGMENT_BYTES + 1];
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
        enc.try_draw_tiles(&[TileRect {
            x: 0,
            y: 0,
            width: 8,
            height: 8,
        }])
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
