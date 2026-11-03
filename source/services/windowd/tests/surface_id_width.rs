// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0112 / TASK-0348 P14 — a restarted compositor seeds its surface ids with the
//! milliseconds since boot, so after a late restart every id is past 65535. Every frame of the
//! display protocol that names a surface must carry the id whole (`u32`), in both directions:
//! a `u16` anywhere would make the first surface after such a restart unreachable — no input,
//! no caret, no text focus, no frame pulse.
//! OWNERS: @ui @runtime

use nexus_display_proto::client_surface::{
    decode_surface_ack, decode_surface_destroy, encode_surface_ack, encode_surface_destroy,
    DamageRect, OP_SURFACE_CREATE, OP_SURFACE_PRESENT, SURFACE_STATUS_OK,
};
use nexus_display_proto::frame_time::{
    decode_surface_frame, decode_surface_frame_req, encode_surface_frame, encode_surface_frame_req,
};
use nexus_display_proto::surface_alloc::{
    decode_allocated, decode_free, encode_allocated, encode_free, Allocated,
};
use nexus_display_proto::surface_buffers::{
    decode_surface_buffers, decode_surface_present, encode_surface_buffers, encode_surface_present,
    Buffers, SURFACE_PRESENT_MAX_LEN,
};
use nexus_display_proto::surface_capture::{
    decode_capture_reply, encode_capture_reply, CaptureReply, CaptureWindow, CAPTURE_OK,
    CAPTURE_WINDOWS_MAX,
};
use nexus_display_proto::surface_caret::{decode_surface_caret, encode_surface_caret, Caret};
use nexus_display_proto::surface_input::{
    decode_surface_input, decode_surface_motion, encode_surface_motion, encode_surface_pointer,
    PointerFacts, INPUT_KIND_MOVE, INPUT_KIND_TAP,
};
use nexus_display_proto::surface_layers::{
    decode_surface_motion as decode_layer_motion, decode_surface_tree,
    encode_surface_motion as encode_layer_motion, encode_surface_tree, SurfaceMotion, SurfaceTree,
    SURFACE_MOTION_MAX_LEN, SURFACE_TREE_MAX_LEN,
};
use nexus_display_proto::surface_motion::MotionBatch;
use nexus_display_proto::surface_text::{
    decode_surface_cursor_hint, decode_surface_text_focus, encode_surface_cursor_hint,
    encode_surface_text_focus, CURSOR_HINT_TEXT, SURFACE_FIELD_TEXT,
};
use nexus_display_proto::STATUS_OK;

/// The ids a compositor restarted late hands out: past `u16`, with the high half set in every
/// pattern a narrowing would hide (a bare 0x1_0000 reads as 0 in a `u16`, the last as 0xFFFF).
const WIDE_IDS: [u32; 4] = [0x1_0000, 0x1_2D05, 0xDEAD_BEEF, u32::MAX];

#[test]
fn a_wide_surface_id_round_trips_through_every_client_frame() {
    for id in WIDE_IDS {
        assert_eq!(
            decode_surface_destroy(&encode_surface_destroy(id)),
            Some(id),
            "destroy {id:#x}"
        );
        assert_eq!(decode_surface_frame_req(&encode_surface_frame_req(id)), Some(id), "frame req");
        assert_eq!(decode_surface_frame(&encode_surface_frame(id)), Some(id), "frame pulse");

        let press = encode_surface_pointer(
            id,
            INPUT_KIND_TAP,
            10,
            20,
            PointerFacts { modifiers: 0, clicks: 1 },
        );
        let (got, kind, _, _, _) = decode_surface_input(&press).expect("a press decodes");
        assert_eq!((got, kind), (id, INPUT_KIND_TAP), "pointer {id:#x}");

        let (motion, len) =
            encode_surface_motion(id, INPUT_KIND_MOVE, 1, 2, &MotionBatch::default())
                .expect("a move with its samples encodes");
        let (got, ..) = decode_surface_motion(&motion[..len]).expect("a move decodes");
        assert_eq!(got, id, "pointer motion {id:#x}");

        let caret = Caret { x: 4, y: 5, w: 2, h: 20, bgra: [0, 0, 0, 0xff], flags: 1 };
        let frame = encode_surface_caret(id, &caret).expect("a caret encodes");
        assert_eq!(decode_surface_caret(&frame).map(|(s, _)| s), Some(id), "caret {id:#x}");

        let focus = encode_surface_text_focus(id, true, SURFACE_FIELD_TEXT, (1, 2, 3, 4));
        assert_eq!(decode_surface_text_focus(&focus).map(|f| f.0), Some(id), "text focus");
        let hint = encode_surface_cursor_hint(id, CURSOR_HINT_TEXT);
        assert_eq!(decode_surface_cursor_hint(&hint), Some((id, CURSOR_HINT_TEXT)), "cursor hint");

        let mut tree = [0u8; SURFACE_TREE_MAX_LEN];
        let n = encode_surface_tree(id, &SurfaceTree::default(), &mut tree);
        let mut out = SurfaceTree::default();
        assert_eq!(decode_surface_tree(&tree[..n], &mut out), Some(id), "tree {id:#x}");

        let mut motion = [0u8; SURFACE_MOTION_MAX_LEN];
        let n = encode_layer_motion(id, &SurfaceMotion::default(), &mut motion);
        let mut out = SurfaceMotion::default();
        assert_eq!(decode_layer_motion(&motion[..n], &mut out), Some(id), "layer motion");

        let mut present = [0u8; SURFACE_PRESENT_MAX_LEN];
        let damage = [DamageRect { x: 0, y: 0, width: 8, height: 8 }];
        let n = encode_surface_present(id, 7, 1, &damage, &mut present);
        let p = decode_surface_present(&present[..n]).expect("a present decodes");
        assert_eq!((p.surface_id, p.seq, p.slot), (id, 7, 1), "present {id:#x}");

        let buffers = Buffers {
            status: SURFACE_STATUS_OK,
            surface_id: id,
            stride: 1024,
            width: 256,
            rows: 64,
            slots: 2,
            cache_block: 64,
        };
        assert_eq!(decode_surface_buffers(&encode_surface_buffers(&buffers)), Some(buffers));

        let created = encode_surface_ack(OP_SURFACE_CREATE, SURFACE_STATUS_OK, id);
        assert_eq!(decode_surface_ack(&created, OP_SURFACE_CREATE), Some((SURFACE_STATUS_OK, id)));
        let presented = encode_surface_ack(OP_SURFACE_PRESENT, SURFACE_STATUS_OK, id);
        assert_eq!(
            decode_surface_ack(&presented, OP_SURFACE_PRESENT),
            Some((SURFACE_STATUS_OK, id))
        );
    }
}

#[test]
fn a_wide_buffer_id_round_trips_through_the_gpud_frames() {
    for id in WIDE_IDS {
        assert_eq!(decode_free(&encode_free(id)), Some(id), "free {id:#x}");
        let a = Allocated {
            cookie: id,
            status: STATUS_OK,
            buffer: id,
            stride: 1024,
            width: 256,
            rows: 64,
            slots: 2,
            cache_block: 64,
        };
        assert_eq!(decode_allocated(&encode_allocated(&a)), Some(a), "allocated {id:#x}");
    }
}

#[test]
fn a_capture_lists_windows_by_their_wide_surface_ids() {
    let windows: Vec<CaptureWindow> =
        WIDE_IDS.iter().map(|&id| CaptureWindow { id, x: 0, y: 0, w: 64, h: 64 }).collect();
    let reply = CaptureReply { status: CAPTURE_OK, nonce: 1, w: 640, h: 480, ..Default::default() };
    let mut frame = [0u8; 512];
    let n = encode_capture_reply(&reply, &windows, &mut frame).expect("a reply encodes");
    let mut out = [CaptureWindow::default(); CAPTURE_WINDOWS_MAX];
    let (_, count) = decode_capture_reply(&frame[..n], &mut out).expect("a reply decodes");
    let ids: Vec<u32> = out[..count].iter().map(|w| w.id).collect();
    assert_eq!(ids, WIDE_IDS.to_vec());
}
