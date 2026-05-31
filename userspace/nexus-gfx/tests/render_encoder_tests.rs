// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests for nexus_gfx::render_encoder.
//! OWNERS: @ui @runtime
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md

use nexus_gfx::{CommandBuffer, GfxError, RenderPassDesc, TileRect};

#[test]
fn encoder_records_commands() {
    let mut cmd = CommandBuffer::new();
    {
        let mut enc = cmd.begin_render_pass(RenderPassDesc {
            color_attachments: vec![],
            width: 64,
            height: 64,
        });
        enc.set_fragment_bytes(0, &[42]);
        enc.draw_tiles(&[TileRect { x: 0, y: 0, width: 10, height: 10 }]);
        enc.end_encoding();
    }
    let c = cmd.commit();
    assert_eq!(c.command_count(), 2);
}

#[test]
fn end_encoding_consumes_encoder() {
    let mut cmd = CommandBuffer::new();
    let enc =
        cmd.begin_render_pass(RenderPassDesc { color_attachments: vec![], width: 64, height: 64 });
    enc.end_encoding();
    // enc is consumed — cannot be used after. Type system enforces this.
}

#[test]
fn test_reject_empty_tile_draw() {
    let mut cmd = CommandBuffer::new();
    let mut enc = cmd
        .try_begin_render_pass(RenderPassDesc { color_attachments: vec![], width: 64, height: 64 })
        .unwrap();
    assert_eq!(enc.try_draw_tiles(&[]).err(), Some(GfxError::InvalidArgument));
}
