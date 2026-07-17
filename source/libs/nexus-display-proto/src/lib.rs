// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The single source of truth for the **windowd ↔ gpud** display-server
//! wire — the opcodes, status codes, cursor-reply magics, and the small
//! control-frame encoders/decoders. Both ends import these instead of each
//! hand-defining a private copy that the other "mirrors" (the historical double
//! structure: gpud `OP_*` vs windowd `GPU_*_OP`, kept in sync by comment).
//!
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal v1
//!
//! WIRE MODEL (see also `source/drivers/gpud/idl/gpud.capnp`, descriptive only):
//! every IPC frame is a 1-byte [`OpCode`](OP_PRESENT_DAMAGE) followed by an
//! opcode-specific payload. The **hot per-frame stream** ([`OP_PRESENT_DAMAGE`] /
//! [`OP_SUBMIT_ANIMATION_FRAME`]) carries a serialized `nexus_gfx::CommittedBuffer`
//! after the opcode byte — that codec is the SSOT for the command payload and is
//! NOT re-encoded here. This crate owns only the thin control frames (attach,
//! legacy damage rect, cursor) and the shared constants. Bulk pixel data never
//! crosses IPC; it lives in the shared framebuffer VMO (capability move).
//!
//! Why hand-rolled and not Cap'n Proto: the control frames are tiny, fixed, and
//! on the boot/handoff + per-frame paths; Cap'n Proto's segment/pointer framing
//! is larger than these few LE fields and fights the IPC frame budget, with no
//! schema-evolution need across this single-language boundary. Cap'n Proto stays
//! the control-plane (samgr/policyd/…) choice; this data-plane wire is the one
//! shared Rust definition. See `docs/adr/0038-display-wire-ssot-and-capnp-boundary.md`.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

/// ADR-0042 client-surface transport (app process ↔ windowd).
pub mod client_surface;

// ── Opcodes (frame byte 0) ───────────────────────────────────────────────────

/// Submit a serialized `CommittedBuffer` animation frame (no scanout update).
pub const OP_SUBMIT_ANIMATION_FRAME: u8 = 1;
/// Move the hardware cursor (deprecated; cursor composites via BlendCursor).
pub const OP_MOVE_CURSOR: u8 = 2;
/// Attach the shared framebuffer VMO (capability moved in the IPC cap slot).
pub const OP_SET_FRAMEBUFFER_VMO: u8 = 3;
/// Present with damage: opcode + serialized `CommittedBuffer` (preferred) or the
/// legacy fixed 17-byte rect frame ([`encode_damage_frame`]).
pub const OP_PRESENT_DAMAGE: u8 = 4;
/// Upload a cursor sprite (BGRA) for software BlendCursor compositing.
pub const OP_UPLOAD_CURSOR: u8 = 5;
/// Scroll fast path: new absolute atlas source row for the scrollable layer
/// identified by `scroll_id`. Payload: `scroll_id: u32` + `src_row: u32`
/// (little-endian). Generalizes the former chat-only scroll op (TASK-0070
/// Phase 7) — any layer composited with a non-zero `scroll_id` can be
/// re-sampled without a CPU re-render.
pub const OP_SET_LAYER_SCROLL: u8 = 6;
/// Upload a real icon sprite to composite as a GPU layer.
pub const OP_UPLOAD_ICON: u8 = 7;
/// Fill a cursor shape-cache slot WITHOUT arming it. Payload:
/// `[shape_id: u8][w: u32][h: u32][hot_x: u32][hot_y: u32][bgra]`.
/// Reply: single status byte. Arming stays `OP_UPLOAD_CURSOR`; switching a
/// cached shape is `OP_SELECT_CURSOR_SHAPE` — together they turn a pointer
/// shape change from a blocking 4KB re-upload into a 2-byte fire-and-forget.
pub const OP_UPLOAD_CURSOR_SHAPE: u8 = 8;
/// Switch the active cursor sprite to a previously cached shape slot.
/// Payload: `[shape_id: u8]`. Reply: single status byte (fire-and-forget safe).
pub const OP_SELECT_CURSOR_SHAPE: u8 = 9;

/// Track C2 — the unified compositor layer-transform override (the
/// generalization of [`OP_SET_LAYER_SCROLL`]): windowd animates a retained
/// window layer's translate/opacity/scale WITHOUT any re-render or re-upload.
/// gpud RECORDS the override per layer id and re-composites ONCE per drained
/// burst (the scroll coalescing contract); a full present clears the table —
/// windowd bakes the current transform into the encoded layer (snap-back
/// agreement). Frame: `[op, layer_id u32, dx i16, dy i16, opacity u8,
/// scale_pct u16]` = 12 bytes; opacity 255 + scale 100 + 0/0 = identity.
pub const OP_SET_LAYER_TRANSFORM: u8 = 10;

/// Encoded [`OP_SET_LAYER_TRANSFORM`] frame length.
pub const SET_LAYER_TRANSFORM_LEN: usize = 12;

/// Query the VISIBLE display mode gpud resolved at probe
/// (`GET_DISPLAY_INFO` → the device's `xres=`/`yres=`, clamped to the fixed
/// resource budget). windowd asks this ONCE, blocking, BEFORE it builds its
/// config/atlas/framebuffer — the mode must exist before anything sizes to
/// it (the framebuffer-handoff ack would be too late). Request: `[op]`;
/// reply: `[status, w: u16 le, h: u16 le]` = 5 bytes.
pub const OP_GET_DISPLAY_MODE: u8 = 11;

/// windowd → gpud: the wallpaper SOURCE plane (VMO plane 0) was rewritten
/// (theme-matched wallpaper swap) — re-upload the wallpaper GL texture from
/// it on the next present. Without this, gpud's one-shot reveal latch keeps
/// the boot wallpaper forever. Request: `[op]`; reply: `[status]`.
pub const OP_WALLPAPER_DIRTY: u8 = 12;

/// Encoded [`OP_GET_DISPLAY_MODE`] reply length.
pub const DISPLAY_MODE_REPLY_LEN: usize = 5;

/// Encode the [`OP_GET_DISPLAY_MODE`] reply.
#[must_use]
pub fn encode_display_mode_reply(status: u8, w: u16, h: u16) -> [u8; DISPLAY_MODE_REPLY_LEN] {
    let mut f = [0u8; DISPLAY_MODE_REPLY_LEN];
    f[0] = status;
    f[1..3].copy_from_slice(&w.to_le_bytes());
    f[3..5].copy_from_slice(&h.to_le_bytes());
    f
}

/// Decode an [`OP_GET_DISPLAY_MODE`] reply → `(w, h)`; `None` when malformed
/// or the status is not OK.
#[must_use]
pub fn decode_display_mode_reply(frame: &[u8]) -> Option<(u16, u16)> {
    if frame.len() < DISPLAY_MODE_REPLY_LEN || frame[0] != STATUS_OK {
        return None;
    }
    Some((u16::from_le_bytes([frame[1], frame[2]]), u16::from_le_bytes([frame[3], frame[4]])))
}

/// Encode the layer-transform override (see [`OP_SET_LAYER_TRANSFORM`]).
#[must_use]
pub fn encode_set_layer_transform(
    layer_id: u32,
    dx: i16,
    dy: i16,
    opacity: u8,
    scale_pct: u16,
) -> [u8; SET_LAYER_TRANSFORM_LEN] {
    let mut f = [0u8; SET_LAYER_TRANSFORM_LEN];
    f[0] = OP_SET_LAYER_TRANSFORM;
    f[1..5].copy_from_slice(&layer_id.to_le_bytes());
    f[5..7].copy_from_slice(&dx.to_le_bytes());
    f[7..9].copy_from_slice(&dy.to_le_bytes());
    f[9] = opacity;
    f[10..12].copy_from_slice(&scale_pct.to_le_bytes());
    f
}

/// Decode an [`OP_SET_LAYER_TRANSFORM`] frame → `(layer_id, dx, dy, opacity,
/// scale_pct)`; `None` when malformed.
#[must_use]
pub fn decode_set_layer_transform(frame: &[u8]) -> Option<(u32, i16, i16, u8, u16)> {
    if frame.len() < SET_LAYER_TRANSFORM_LEN || frame[0] != OP_SET_LAYER_TRANSFORM {
        return None;
    }
    Some((
        u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]),
        i16::from_le_bytes([frame[5], frame[6]]),
        i16::from_le_bytes([frame[7], frame[8]]),
        frame[9],
        u16::from_le_bytes([frame[10], frame[11]]),
    ))
}
/// Number of cursor shape-cache slots gpud guarantees: 5 pointer shapes
/// (default + 4 resize) + 8 loading-ring frames (the animated wait cursor
/// cycles pre-uploaded slots via the 2-byte SELECT — no per-frame upload).
pub const CURSOR_SHAPE_SLOTS: usize = 16;

// ── Status codes (reply byte 0) ──────────────────────────────────────────────

pub const STATUS_OK: u8 = 0;
pub const STATUS_MALFORMED: u8 = 1;
pub const STATUS_DEVICE_ERROR: u8 = 2;

// ── Cursor-upload reply magics (reply u32 at bytes [1..5]) ───────────────────
//
// Magic-tagged so they are distinguishable from present/attach acks, whose u32
// slot carries a small handoff id.

/// Software cursor accepted (no HW overlay).
pub const CURSOR_REPLY_SW: u32 = 0xC0DE_0000;
/// Hardware cursor overlay armed.
pub const CURSOR_REPLY_HW: u32 = 0xC0DE_0001;
/// virgl GL scanout draws a procedural cursor each present.
pub const CURSOR_REPLY_GL: u32 = 0xC0DE_0002;

// ── Control-frame encoders / decoders ────────────────────────────────────────

/// Legacy fixed present frame: `[OP_PRESENT_DAMAGE, x, y, width, height]`, each
/// coordinate a little-endian `u32` (17 bytes). The preferred present path
/// instead appends a serialized `CommittedBuffer` after the opcode byte.
#[must_use]
pub fn encode_damage_frame(x: u32, y: u32, width: u32, height: u32) -> [u8; 17] {
    let mut f = [0u8; 17];
    f[0] = OP_PRESENT_DAMAGE;
    f[1..5].copy_from_slice(&x.to_le_bytes());
    f[5..9].copy_from_slice(&y.to_le_bytes());
    f[9..13].copy_from_slice(&width.to_le_bytes());
    f[13..17].copy_from_slice(&height.to_le_bytes());
    f
}

/// Framebuffer-attach handoff frame: `[OP_SET_FRAMEBUFFER_VMO, handoff_id]`
/// (`handoff_id` little-endian `u32`, 5 bytes). The VMO capability rides the IPC
/// cap slot, not the frame.
#[must_use]
pub fn encode_attach_frame(handoff_id: u32) -> [u8; 5] {
    let mut f = [0u8; 5];
    f[0] = OP_SET_FRAMEBUFFER_VMO;
    f[1..5].copy_from_slice(&handoff_id.to_le_bytes());
    f
}

/// Status reply frame: `[status, handoff_id]` (`handoff_id` little-endian `u32`,
/// 5 bytes). Fire-and-forget ops instead reply with a single status byte.
#[must_use]
pub fn encode_status_reply(status: u8, handoff_id: u32) -> [u8; 5] {
    let mut f = [0u8; 5];
    f[0] = status;
    f[1..5].copy_from_slice(&handoff_id.to_le_bytes());
    f
}

/// Decode the handoff id that immediately follows the opcode/status byte
/// (`frame[1..5]`). Serves both the attach request and the 5-byte status reply.
#[must_use]
pub fn decode_handoff_id(frame: &[u8]) -> Option<u32> {
    if frame.len() < 5 {
        return None;
    }
    Some(u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]))
}

/// Decode the handoff id trailing a legacy 17-byte present frame
/// (`frame[17..21]`, present only on the 21-byte legacy form).
#[must_use]
pub fn decode_present_handoff_id(frame: &[u8]) -> Option<u32> {
    if frame.len() < 21 {
        return None;
    }
    Some(u32::from_le_bytes([frame[17], frame[18], frame[19], frame[20]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_frame_exact_bytes() {
        let f = encode_damage_frame(0x11, 0x2233, 0x44, 0x55);
        assert_eq!(f[0], 4);
        assert_eq!(&f[1..5], &0x11u32.to_le_bytes());
        assert_eq!(&f[5..9], &0x2233u32.to_le_bytes());
        assert_eq!(&f[9..13], &0x44u32.to_le_bytes());
        assert_eq!(&f[13..17], &0x55u32.to_le_bytes());
    }

    #[test]
    fn attach_and_reply_roundtrip() {
        let f = encode_attach_frame(0xABCD_1234);
        assert_eq!(f[0], OP_SET_FRAMEBUFFER_VMO);
        assert_eq!(decode_handoff_id(&f), Some(0xABCD_1234));
        let r = encode_status_reply(STATUS_OK, 7);
        assert_eq!(r[0], STATUS_OK);
        assert_eq!(decode_handoff_id(&r), Some(7));
    }

    #[test]
    fn handoff_decoders_are_length_guarded() {
        assert_eq!(decode_handoff_id(&[4, 1, 2, 3]), None); // < 5 bytes
        assert_eq!(decode_present_handoff_id(&[0u8; 17]), None); // < 21 bytes
        let mut legacy = [0u8; 21];
        legacy[17..21].copy_from_slice(&0x99u32.to_le_bytes());
        assert_eq!(decode_present_handoff_id(&legacy), Some(0x99));
    }
}
