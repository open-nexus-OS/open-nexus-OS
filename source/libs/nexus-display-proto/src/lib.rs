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
//! NOT re-encoded here. This crate owns only the thin control frames (the framebuffer
//! grant, attach, legacy damage rect, cursor) and the shared constants. Bulk pixel data
//! never crosses IPC; it lives in the shared framebuffer VMO — gpud's, granted to windowd
//! by capability move ([`grant`]).
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
pub mod control;
pub mod envelope;
/// The framebuffer grant — the display mode and the scanout memory are gpud's (RFC-0098 C7).
pub mod grant;
pub mod layout;
pub use grant::{
    decode_framebuffer_grant, encode_framebuffer_grant, encode_framebuffer_request,
    parse_display_request, FramebufferGrant, FRAMEBUFFER_GRANT_LEN, OP_FRAMEBUFFER_REQUEST,
};
pub mod surface_settings;
pub mod surface_text;
pub mod surface_windows;
pub use surface_windows::{OP_SURFACE_TASKBAR, OP_SURFACE_WINDOWS};

// ── Opcodes (frame byte 0) ───────────────────────────────────────────────────

/// Submit a serialized `CommittedBuffer` animation frame (no scanout update).
pub const OP_SUBMIT_ANIMATION_FRAME: u8 = 1;
/// Move the hardware cursor (deprecated; cursor composites via BlendCursor).
pub const OP_MOVE_CURSOR: u8 = 2;
/// Attach: the framebuffer gpud GRANTED ([`OP_FRAMEBUFFER_REQUEST`]) now holds windowd's first
/// frame — scan it out. `[op, handoff_id: u32 le]`; no capability moves: since RFC-0098 C7 the
/// framebuffer is gpud's, and an attach that carries a cap is refused.
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

/// windowd → gpud: the wallpaper SOURCE plane (VMO plane 0) was rewritten
/// (theme-matched wallpaper swap) — re-upload the wallpaper GL texture from
/// it on the next present. Without this, gpud's one-shot reveal latch keeps
/// the boot wallpaper forever. Request: `[op]`; reply: `[status]`.
pub const OP_WALLPAPER_DIRTY: u8 = 12;

/// windowd → gpud (RFC-0093 §5): "the desktop is complete — wallpaper written,
/// cursor uploaded, first frame presented — reveal it". gpud latches the request
/// and answers it on the ack of the FIRST present at or after it, which carries
/// [`STATUS_REVEALED`] instead of [`STATUS_OK`]. Reveal is a handshake, not a
/// heuristic: no pixel probe, no time cap. Request: `[op]`; reply: `[status]`.
pub const OP_REVEAL: u8 = 13;

/// Bytes in front of the serialized `CommittedBuffer` of a present:
/// `[OP_PRESENT_DAMAGE, seq: u32 le]`. `seq` is strictly increasing per windowd
/// instance and echoed by the present ack, so an ack can only ever credit the
/// present it belongs to.
pub const PRESENT_HEADER_LEN: usize = 5;
/// Present ack: `[status, seq: u32 le]`.
pub const PRESENT_ACK_LEN: usize = 5;
/// Attach ack v2: `[status, handoff_id, seq, mode_w, mode_h, content_x, content_y,
/// content_w, content_h]` (u32 le, then u16 le ×6).
pub const ATTACH_ACK_LEN: usize = 21;

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

// ── Display mode: ONE policy, ONE maximum, ONE authority (RFC-0098 C7, RFC-0093 §5) ─

/// The fixed shared-VMO layout maximum — the RESOURCE BUDGET every display
/// consumer sizes against, not a "default mode". It lived three times (windowd's
/// `DISPLAY_WIDTH/HEIGHT`, gpud's own pair, inputd's fallback); its one home is
/// [`layout`], together with every plane row and byte offset derived from it.
pub use layout::LAYOUT_MAX;

/// Resolve the VISIBLE display mode — gpud's decision, the one authority (RFC-0098 C7).
///
/// Order: the lane's **request** (`/chosen/nexus,display-mode`, read by gpud from its own
/// tree slot, [`parse_display_request`]) wins — it is what the launcher asked for and it is
/// race-free, where a QEMU window may transiently report its unrealized default; else the
/// device's advertised **capability** (virtio display info on QEMU, the monitor's EDID on the
/// board); else `layout_max`. Every candidate is validated non-zero and clamped, so a racy or
/// malicious report can never size the scanout degenerately.
///
/// gpud is the only caller: windowd receives the result with the framebuffer grant
/// ([`OP_FRAMEBUFFER_REQUEST`]) and inputd asks windowd for it. The polled query protocols of
/// before (RFC-0093 §5) stay retired, and so does the kernel's relay of the request.
#[must_use]
pub fn resolve_display_mode(
    request: Option<(u32, u32)>,
    device: Option<(u32, u32)>,
    layout_max: (u32, u32),
) -> (u32, u32) {
    resolve_display_mode_sourced(request, device, layout_max).0
}

/// Which candidate [`resolve_display_mode`] took — gpud names it in its decision marker
/// (`gpud: display mode WxH (<source>)`), so a lane whose request never arrived is visible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeSource {
    /// The lane's request (`/chosen/nexus,display-mode`).
    Request,
    /// The device's advertised capability.
    Device,
    /// Neither: the layout maximum.
    LayoutMax,
}

impl ModeSource {
    /// The marker word.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            ModeSource::Request => "request",
            ModeSource::Device => "device",
            ModeSource::LayoutMax => "maximum",
        }
    }
}

/// [`resolve_display_mode`] with the candidate it took — the one implementation of the policy.
#[must_use]
pub fn resolve_display_mode_sourced(
    request: Option<(u32, u32)>,
    device: Option<(u32, u32)>,
    layout_max: (u32, u32),
) -> ((u32, u32), ModeSource) {
    let sane = |wh: Option<(u32, u32)>| -> Option<(u32, u32)> {
        wh.and_then(|(w, h)| {
            if w == 0 || h == 0 {
                None
            } else {
                Some((w.min(layout_max.0), h.min(layout_max.1)))
            }
        })
    };
    match (sane(request), sane(device)) {
        (Some(mode), _) => (mode, ModeSource::Request),
        (None, Some(mode)) => (mode, ModeSource::Device),
        (None, None) => (layout_max, ModeSource::LayoutMax),
    }
}

// ── Status codes (reply byte 0) ──────────────────────────────────────────────

pub const STATUS_OK: u8 = 0;
pub const STATUS_MALFORMED: u8 = 1;
pub const STATUS_DEVICE_ERROR: u8 = 2;
/// Present ack status of the frame that REVEALED the desktop (the first present
/// at or after [`OP_REVEAL`]). A pixel-backed event: windowd emits
/// `display: first scanout ok` on it, never on a timer.
pub const STATUS_REVEALED: u8 = 3;

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

/// Fixed-rect present frame (the fallback when a `CommittedBuffer` cannot be built):
/// the present header, then `x, y, width, height` as little-endian `u32`
/// (21 bytes). The `seq` slot is left for [`write_present_header`].
pub const DAMAGE_FRAME_LEN: usize = PRESENT_HEADER_LEN + 16;

/// Encode a fixed-rect present (see [`DAMAGE_FRAME_LEN`]).
#[must_use]
pub fn encode_damage_frame(x: u32, y: u32, width: u32, height: u32) -> [u8; DAMAGE_FRAME_LEN] {
    let mut f = [0u8; DAMAGE_FRAME_LEN];
    f[0] = OP_PRESENT_DAMAGE;
    let o = PRESENT_HEADER_LEN;
    f[o..o + 4].copy_from_slice(&x.to_le_bytes());
    f[o + 4..o + 8].copy_from_slice(&y.to_le_bytes());
    f[o + 8..o + 12].copy_from_slice(&width.to_le_bytes());
    f[o + 12..o + 16].copy_from_slice(&height.to_le_bytes());
    f
}

/// Decode a fixed-rect present → `(x, y, width, height)`; `None` when short.
#[must_use]
pub fn decode_damage_frame(frame: &[u8]) -> Option<(u32, u32, u32, u32)> {
    if frame.len() < DAMAGE_FRAME_LEN {
        return None;
    }
    let o = PRESENT_HEADER_LEN;
    let at = |i: usize| u32::from_le_bytes([frame[i], frame[i + 1], frame[i + 2], frame[i + 3]]);
    Some((at(o), at(o + 4), at(o + 8), at(o + 12)))
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

/// Decode the handoff id that immediately follows the opcode/status byte
/// (`frame[1..5]`). Serves both the attach request and the 5-byte status reply.
#[must_use]
pub fn decode_handoff_id(frame: &[u8]) -> Option<u32> {
    if frame.len() < 5 {
        return None;
    }
    Some(u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]))
}

/// Write the present header (`[OP_PRESENT_DAMAGE, seq]`) in front of an already
/// serialized `CommittedBuffer` at `frame[PRESENT_HEADER_LEN..]`.
pub fn write_present_header(frame: &mut [u8], seq: u32) {
    frame[0] = OP_PRESENT_DAMAGE;
    frame[1..PRESENT_HEADER_LEN].copy_from_slice(&seq.to_le_bytes());
}

/// The present's `seq`; `None` for a frame too short to carry a header.
#[must_use]
pub fn decode_present_seq(frame: &[u8]) -> Option<u32> {
    if frame.len() < PRESENT_HEADER_LEN || frame[0] != OP_PRESENT_DAMAGE {
        return None;
    }
    Some(u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]))
}

/// Present ack `[status, seq]`.
#[must_use]
pub fn encode_present_ack(status: u8, seq: u32) -> [u8; PRESENT_ACK_LEN] {
    let mut f = [0u8; PRESENT_ACK_LEN];
    f[0] = status;
    f[1..5].copy_from_slice(&seq.to_le_bytes());
    f
}

/// Decode a present ack → `(status, seq)`; `None` when the frame cannot carry a
/// seq — an ack WITHOUT a seq can never be credited (RFC-0093 §5).
#[must_use]
pub fn decode_present_ack(frame: &[u8]) -> Option<(u8, u32)> {
    if frame.len() < PRESENT_ACK_LEN {
        return None;
    }
    Some((frame[0], u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]])))
}

/// True for the magic-tagged u32 a cursor-upload reply carries in the seq slot.
/// A present seq can never collide with one in practice (2^32 − 3 presents), and
/// the ack window makes a collision harmless anyway: a cursor magic is never an
/// OUTSTANDING seq.
#[must_use]
pub const fn is_cursor_reply_magic(payload: u32) -> bool {
    matches!(payload, CURSOR_REPLY_SW | CURSOR_REPLY_HW | CURSOR_REPLY_GL)
}

/// Attach ack v2 (RFC-0093 §5): what gpud commands onto the scanout for this
/// handoff. `mode` is the VISIBLE mode; windowd cross-checks it against the mode
/// gpud granted with the framebuffer — the ack is evidence, not the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttachAck {
    pub status: u8,
    pub handoff_id: u32,
    pub seq: u32,
    pub mode_w: u16,
    pub mode_h: u16,
    pub content_x: u16,
    pub content_y: u16,
    pub content_w: u16,
    pub content_h: u16,
}

/// Encode an [`AttachAck`].
#[must_use]
pub fn encode_attach_ack(ack: &AttachAck) -> [u8; ATTACH_ACK_LEN] {
    let mut f = [0u8; ATTACH_ACK_LEN];
    f[0] = ack.status;
    f[1..5].copy_from_slice(&ack.handoff_id.to_le_bytes());
    f[5..9].copy_from_slice(&ack.seq.to_le_bytes());
    for (i, v) in
        [ack.mode_w, ack.mode_h, ack.content_x, ack.content_y, ack.content_w, ack.content_h]
            .into_iter()
            .enumerate()
    {
        f[9 + 2 * i..11 + 2 * i].copy_from_slice(&v.to_le_bytes());
    }
    f
}

/// Decode an attach ack; `None` when short or when the mode is degenerate — an
/// attach ack WITHOUT a mode is not an attach ack (RFC-0093 §5).
#[must_use]
pub fn decode_attach_ack(frame: &[u8]) -> Option<AttachAck> {
    if frame.len() < ATTACH_ACK_LEN {
        return None;
    }
    let u16_at = |i: usize| u16::from_le_bytes([frame[i], frame[i + 1]]);
    let ack = AttachAck {
        status: frame[0],
        handoff_id: u32::from_le_bytes([frame[1], frame[2], frame[3], frame[4]]),
        seq: u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]]),
        mode_w: u16_at(9),
        mode_h: u16_at(11),
        content_x: u16_at(13),
        content_y: u16_at(15),
        content_w: u16_at(17),
        content_h: u16_at(19),
    };
    if ack.mode_w == 0 || ack.mode_h == 0 {
        return None;
    }
    Some(ack)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_frame_exact_bytes() {
        let mut f = encode_damage_frame(0x11, 0x2233, 0x44, 0x55);
        assert_eq!(f[0], 4);
        assert_eq!(&f[5..9], &0x11u32.to_le_bytes());
        assert_eq!(&f[9..13], &0x2233u32.to_le_bytes());
        assert_eq!(&f[13..17], &0x44u32.to_le_bytes());
        assert_eq!(&f[17..21], &0x55u32.to_le_bytes());
        write_present_header(&mut f, 7);
        assert_eq!(decode_present_seq(&f), Some(7));
        assert_eq!(decode_damage_frame(&f), Some((0x11, 0x2233, 0x44, 0x55)));
        assert_eq!(decode_damage_frame(&f[..20]), None);
    }

    #[test]
    fn attach_request_roundtrip_and_length_guard() {
        let f = encode_attach_frame(0xABCD_1234);
        assert_eq!(f[0], OP_SET_FRAMEBUFFER_VMO);
        assert_eq!(decode_handoff_id(&f), Some(0xABCD_1234));
        assert_eq!(decode_handoff_id(&[4, 1, 2, 3]), None);
    }

    #[test]
    fn present_header_and_ack_golden_bytes() {
        let mut frame = [0xEEu8; 8];
        write_present_header(&mut frame, 0x0102_0304);
        assert_eq!(&frame[..5], &[OP_PRESENT_DAMAGE, 0x04, 0x03, 0x02, 0x01]);
        assert_eq!(&frame[5..], &[0xEE; 3], "payload after the header is untouched");
        assert_eq!(decode_present_seq(&frame), Some(0x0102_0304));
        let ack = encode_present_ack(STATUS_REVEALED, 9);
        assert_eq!(ack, [STATUS_REVEALED, 9, 0, 0, 0]);
        assert_eq!(decode_present_ack(&ack), Some((STATUS_REVEALED, 9)));
    }

    /// An ack that cannot carry a seq can never credit a present (RFC-0093 §5).
    #[test]
    fn test_reject_present_ack_without_seq() {
        assert_eq!(decode_present_ack(&[STATUS_OK]), None);
        assert_eq!(decode_present_ack(&[STATUS_OK, 1, 2, 3]), None);
        assert_eq!(decode_present_seq(&[OP_PRESENT_DAMAGE, 1, 2]), None);
        assert_eq!(decode_present_seq(&[OP_SET_FRAMEBUFFER_VMO, 1, 2, 3, 4]), None);
    }

    #[test]
    fn attach_ack_roundtrip_golden() {
        let ack = AttachAck {
            status: STATUS_OK,
            handoff_id: 0x11,
            seq: 0x22,
            mode_w: 1280,
            mode_h: 800,
            content_x: 0,
            content_y: 0,
            content_w: 1280,
            content_h: 800,
        };
        let f = encode_attach_ack(&ack);
        assert_eq!(f.len(), ATTACH_ACK_LEN);
        assert_eq!(&f[..9], &[STATUS_OK, 0x11, 0, 0, 0, 0x22, 0, 0, 0]);
        assert_eq!(&f[9..13], &[0x00, 0x05, 0x20, 0x03]); // 1280, 800 le
        assert_eq!(decode_attach_ack(&f), Some(ack));
    }

    /// An attach ack WITHOUT a mode is not an attach ack (RFC-0093 §5).
    #[test]
    fn test_reject_attach_ack_without_mode() {
        let mut ack = encode_attach_ack(&AttachAck {
            status: STATUS_OK,
            handoff_id: 1,
            seq: 1,
            mode_w: 1280,
            mode_h: 800,
            content_x: 0,
            content_y: 0,
            content_w: 1280,
            content_h: 800,
        });
        assert_eq!(decode_attach_ack(&ack[..ATTACH_ACK_LEN - 1]), None, "short");
        ack[9..11].copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(decode_attach_ack(&ack), None, "zero width");
    }

    #[test]
    fn cursor_magics_are_not_presents() {
        for m in [CURSOR_REPLY_SW, CURSOR_REPLY_HW, CURSOR_REPLY_GL] {
            assert!(is_cursor_reply_magic(m));
        }
        assert!(!is_cursor_reply_magic(1));
        assert!(!is_cursor_reply_magic(u32::MAX));
    }
    #[test]
    fn request_wins_over_device() {
        // The GTK race makes the device report the tiny window default; the
        // lane's request is race-free and must win.
        assert_eq!(
            resolve_display_mode(Some((1280, 800)), Some((640, 507)), LAYOUT_MAX),
            (1280, 800)
        );
    }

    #[test]
    fn follows_a_smaller_request() {
        assert_eq!(
            resolve_display_mode(Some((1024, 768)), Some((640, 507)), LAYOUT_MAX),
            (1024, 768)
        );
    }

    #[test]
    fn device_capability_used_without_a_request() {
        assert_eq!(resolve_display_mode(None, Some((1024, 768)), LAYOUT_MAX), (1024, 768));
    }

    #[test]
    fn falls_back_to_layout_max() {
        assert_eq!(resolve_display_mode(None, None, LAYOUT_MAX), LAYOUT_MAX);
    }

    /// The source names the candidate the policy took — including a degenerate request that
    /// fell through to the device (the marker must not claim "request" then).
    #[test]
    fn the_source_names_the_candidate_taken() {
        let r = |req, dev| resolve_display_mode_sourced(req, dev, LAYOUT_MAX);
        assert_eq!(r(Some((1280, 800)), Some((640, 507))), ((1280, 800), ModeSource::Request));
        assert_eq!(r(Some((0, 800)), Some((1024, 768))), ((1024, 768), ModeSource::Device));
        assert_eq!(r(None, None), (LAYOUT_MAX, ModeSource::LayoutMax));
        assert_eq!(ModeSource::LayoutMax.label(), "maximum");
    }

    #[test]
    fn test_reject_degenerate_display_mode() {
        // Zero / degenerate reports are rejected, never sizing the scanout.
        assert_eq!(resolve_display_mode(Some((0, 0)), None, LAYOUT_MAX), LAYOUT_MAX);
        assert_eq!(resolve_display_mode(Some((1280, 0)), Some((0, 800)), LAYOUT_MAX), LAYOUT_MAX);
        // Oversized is clamped to the layout maximum, never enlarged.
        assert_eq!(resolve_display_mode(Some((5000, 5000)), None, LAYOUT_MAX), LAYOUT_MAX);
        assert_eq!(resolve_display_mode(None, Some((99999, 1)), LAYOUT_MAX), (LAYOUT_MAX.0, 1));
    }
}
