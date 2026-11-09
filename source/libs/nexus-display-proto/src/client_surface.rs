// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The client-surface frames an app process speaks to windowd (ADR-0042, v2 by
//! RFC-0114): `SURFACE_CREATE` (the size, the intent, the store — answered by
//! `OP_SURFACE_BUFFERS` with the buffer gpud made for the app), `SURFACE_PRESENT` (the slot the
//! app drew, seq + bounded damage rects — `crate::surface_buffers`), `SURFACE_DESTROY`. Acks
//! return over the app's dedicated event channel. Fixed-layout, length-guarded codecs — no
//! allocation.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests below + windowd `client_surface` host tests
//!
//! ENVELOPE: these frames arrive on windowd's SERVER endpoint, which speaks
//! the `[b'I', b'N', version, op]`-header family (input-live-protocol).
//! Ops share that space; MUST NOT collide with input ops 1–4 (pinned by a
//! windowd unit test). Text ops 21/22: `crate::surface_text` (23 reserved).

// The envelope itself lives in `crate::envelope` (one home for "is this frame
// mine?"); re-exported here so `client_surface::ENVELOPE_*` stays the path.
pub(crate) use crate::envelope::{has_op, header, HEADER_LEN};
pub use crate::envelope::{is_client_envelope, ENVELOPE_MAGIC0, ENVELOPE_MAGIC1, ENVELOPE_VERSION};
// The surface's buffer and the present of a slot (RFC-0114 §3): their own module.
pub use crate::surface_buffers::{
    decode_surface_buffers, decode_surface_present, encode_surface_buffers, encode_surface_present,
    Buffers, SurfacePresent, OP_SURFACE_BUFFERS, SURFACE_BUFFERS_FRAME_LEN,
    SURFACE_PRESENT_MAX_LEN,
};
// The intent, the content rect and the app's window size (RFC-0117): their own module.
pub use crate::surface_intent::{
    decode_surface_intent, decode_surface_rect, encode_surface_intent, encode_surface_rect,
    SurfaceIntent, WindowSize, SURFACE_INTENT_FRAME_LEN, SURFACE_RECT_FRAME_LEN, WINDOW_SIZE_LEN,
};

/// Creates an app surface (`SurfaceCreate`). No capability rides with it (RFC-0114): windowd
/// has gpud make the surface's buffer for the app and answers `OP_SURFACE_BUFFERS`.
pub const OP_SURFACE_CREATE: u8 = 8;
/// Presents a slot (`crate::surface_buffers::SurfacePresent`): `surface_id:u32, seq:u32,
/// slot:u8, count:u8, (x:u16,y:u16,w:u16,h:u16) * count`. Its ack releases the other slots.
pub const OP_SURFACE_PRESENT: u8 = 9;
/// Destroys a surface. Payload: `surface_id:u32`.
pub const OP_SURFACE_DESTROY: u8 = 10;
/// windowd → app: an input event routed to the surface (R3). Payload:
/// `surface_id:u32, kind:u8, x:u16, y:u16` — surface-LOCAL body pixels.
pub const OP_SURFACE_INPUT: u8 = 11;
/// execd → windowd: attaches the app's DEDICATED event channel. The message
/// MOVES the channel's SEND capability; windowd retains it and delivers ALL
/// app-bound frames (input events + surface acks) on it. Replaces the shared
/// `window_rsp` delivery, which raced with inputd's ack drain — a tap sent on
/// the shared channel could be consumed by ANY receiver (the R3 "buttons do
/// nothing" bug). Header-only frame.
pub const OP_SURFACE_EVENTS: u8 = 12;
/// app → windowd: the app's WINDOW INTENT, sent BEFORE `SURFACE_CREATE` so the
/// WM can compose the frame + geometry and hand back the content rect the app
/// sizes its surface VMO to. Payload: `style:u8, level:u8, mode:u8,
/// resizable:u8` (the `WIN_*` tags). windowd stores it for the pending surface
/// and answers `OP_SURFACE_RECT` on the event channel. See
/// docs/dev/ui/patterns/windowing/window-intent.md (`chrome = intent ⟂ policy`).
pub const OP_SURFACE_INTENT: u8 = 13;
/// windowd → app: the content rect the WM composed for the app's intent under
/// the active windowing policy (the app owns no geometry). Payload:
/// `x:u16, y:u16, w:u16, h:u16`. Sent once before create (initial size) and
/// again on every mode/resize change (the general geometry channel — no
/// separate "query display mode" op).
pub const OP_SURFACE_RECT: u8 = 14;

/// RETIRED (RFC-0109, number reserved forever — never reuse): the standalone
/// material-region list. A surface's regions ride its structure now
/// (`crate::surface_layers::OP_SURFACE_TREE`), with its scroll layers and patches.
pub const OP_SURFACE_LAYERS: u8 = 15;

/// Material kinds of a region (`LayerDesc`, carried by `OP_SURFACE_TREE`).
pub const MATERIAL_OPAQUE: u8 = 0;
pub const MATERIAL_GLASS: u8 = 1;
/// Glass levels — one blur bucket per backdrop (RFC-0116): panel (above the windows) and window
/// regular, thin and ultraThin (ids 1/2: the old card/subtle buckets) for standalone pieces, dock
/// thick. Id 4 (`GLASS_OVERLAY`) is retired, never reuse: a floating surface is opaque.
pub const GLASS_PANEL: u8 = 0;
pub const GLASS_THIN: u8 = 1;
pub const GLASS_ULTRA_THIN: u8 = 2;
pub const GLASS_WINDOW: u8 = 3;
pub const GLASS_DOCK: u8 = 5;

/// Max material regions per surface (bounds the structure; a real shell uses a
/// handful: topbar, dock, launcher panel, a few cards).
pub const MAX_SURFACE_LAYERS: usize = 16;

/// One material-tagged region of a client surface (base-layer pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LayerDesc {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    /// `MATERIAL_*`.
    pub material: u8,
    /// `GLASS_*` (only meaningful when `material == MATERIAL_GLASS`).
    pub glass_level: u8,
    /// Corner radius in px (0 = square).
    pub radius: u8,
    /// Drop-shadow alpha (0 = no shadow).
    pub shadow_alpha: u8,
}

/// RETIRED (RFC-0083 P5, number reserved forever — never reuse): the
/// standalone theme push. Theme mode + accent now travel in the versioned
/// presentation snapshot (`surface_settings::OP_SURFACE_SETTINGS = 26`).
pub const OP_SURFACE_THEME: u8 = 16;
/// The theme byte (mode, accent, solid materials) — its own module (`theme_byte.rs`).
pub use crate::theme_byte::{
    pack_theme, theme_is_solid, unpack_theme, THEME_ACCENT_SHIFT, THEME_DARK, THEME_LIGHT,
    THEME_MODE_MASK, THEME_SOLID,
};

/// RETIRED (RFC-0083 P5, number reserved forever — never reuse): the
/// standalone shell-profile push, folded into the presentation snapshot.
pub const OP_SURFACE_PROFILE: u8 = 17;
/// Shell profiles (align with the DSL `device.profile` vocabulary; the
/// snapshot's `profile` field).
pub const PROFILE_TABLET: u8 = 0;
pub const PROFILE_DESKTOP: u8 = 1;
pub const PROFILE_PHONE: u8 = 2;
pub const PROFILE_TV: u8 = 3;

/// app → windowd: a WINDOW control request (`CONTROL_WIN_*` + the launch
/// wait-ring hint). Settings are NOT controls any more (RFC-0083): theme,
/// accent and shell mode route to settingsd like every other key and come
/// back as watch events. Payload: `control:u8, value:u8`.
pub const OP_SURFACE_CONTROL: u8 = 18;

/// Frame pulse (the Choreographer contract): a client that is ANIMATING
/// (scroll ease, fling) sends `OP_SURFACE_FRAME_REQ` — a ONE-SHOT request —
/// and the compositor answers with ONE `OP_SURFACE_FRAME` pulse on the
/// client's event channel after its next composited frame. The client ticks
/// its physics on the pulse and re-requests while motion continues. One-shot
/// semantics bound the traffic to the animation's lifetime — no idle pulses,
/// no client-side timer guessing (recv-timeout self-pacing measured ~3-12Hz;
/// the pulse rides the REAL frame cadence).
pub const OP_SURFACE_FRAME_REQ: u8 = 19;
/// The compositor's pulse answering `OP_SURFACE_FRAME_REQ`.
pub const OP_SURFACE_FRAME: u8 = 20;

// The pulse's frames — and the time a pulse names (RFC-0110) — live in `crate::frame_time`.
pub use crate::frame_time::{
    decode_surface_frame, decode_surface_frame_req, encode_surface_frame, encode_surface_frame_req,
    SURFACE_FRAME_FRAME_LEN,
};
// Control kinds (`CONTROL_*`) live in `crate::control` (split out when the
// window-control vocabulary outgrew this file); re-exported below so
// `client_surface::CONTROL_*` stays the path.
pub use crate::control::*;

pub const SURFACE_CONTROL_FRAME_LEN: usize = HEADER_LEN + 2;

/// Encodes a control request.
#[must_use]
pub fn encode_surface_control(control: u8, value: u8) -> [u8; SURFACE_CONTROL_FRAME_LEN] {
    let mut f = [0u8; SURFACE_CONTROL_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_CONTROL));
    f[4] = control;
    f[5] = value;
    f
}

/// `(control, value)`.
#[must_use]
pub fn decode_surface_control(frame: &[u8]) -> Option<(u8, u8)> {
    if !has_op(frame, OP_SURFACE_CONTROL) || frame.len() != SURFACE_CONTROL_FRAME_LEN {
        return None;
    }
    Some((frame[4], frame[5]))
}

/// Window-intent wire tags (mirror the IR `WindowStyle`/`WindowLevel`/
/// `WindowMode` enum ordinals; both ends agree on these, not the capnp type).
pub const WIN_STYLE_TITLEBAR: u8 = 0;
pub const WIN_STYLE_HIDDEN_TITLEBAR: u8 = 1;
pub const WIN_STYLE_PLAIN: u8 = 2;
pub const WIN_LEVEL_NORMAL: u8 = 0;
pub const WIN_LEVEL_DESKTOP: u8 = 1;
pub const WIN_LEVEL_OVERLAY: u8 = 2;
pub const WIN_MODE_AUTO: u8 = 0;
pub const WIN_MODE_FREEFORM: u8 = 1;
pub const WIN_MODE_FULLSCREEN: u8 = 2;
/// `CONTROL_WIN_MODE`-only presentation value (NOT an intent): snap the
/// window to the left work-area half. `WIN_MODE_AUTO` in that control =
/// toggle fullscreen ⇄ freeform (the title-bar zoom semantics).
pub const WIN_MODE_SPLIT: u8 = 3;

/// The input kinds `OP_SURFACE_INPUT` carries (taps, hover, wheel, drag) and its codec — a
/// press's modifiers and click count with it (TASK-0346 P2).
pub use crate::surface_input::{
    decode_surface_input, encode_surface_input, encode_surface_pointer, wheel_delta_from_wire,
    wheel_delta_to_wire, PointerFacts, INPUT_KIND_DRAG, INPUT_KIND_LEAVE, INPUT_KIND_MOVE,
    INPUT_KIND_RELEASE, INPUT_KIND_TAP, INPUT_KIND_WHEEL, SURFACE_INPUT_FRAME_LEN,
};

/// Pixel format tags. v1: BGRA8888 only.
pub const FORMAT_BGRA8888: u8 = 0;

/// Bounded damage list per present (ADR-0042).
pub const MAX_DAMAGE_RECTS: usize = 4;

/// Ack status codes (reply frames).
pub const SURFACE_STATUS_OK: u8 = 0;
pub const SURFACE_STATUS_MALFORMED: u8 = 1;
pub const SURFACE_STATUS_DENIED: u8 = 2;
pub const SURFACE_STATUS_QUOTA: u8 = 3;
pub const SURFACE_STATUS_BAD_SURFACE: u8 = 4;
pub const SURFACE_STATUS_BAD_SEQ: u8 = 5;
/// RETIRED (RFC-0114, value reserved forever — never reuse): a create whose store found no atlas
/// room. The store lives in the surface's own buffer now; a buffer gpud cannot make is `QUOTA`.
pub const SURFACE_STATUS_NO_STORE: u8 = 6;

/// One damage rect in surface-local pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

// ------------------------------------------------------------------ create

// +4 intent bytes (style, level, mode, resizable): the declared window intent
// rides ATOMICALLY with the create — a separate pre-create OP_SURFACE_INTENT
// raced when two app-hosts connected concurrently (shell + counter: the
// pending intent crossed and BOTH surfaces landed in the desktop role).
//
// +4 rows and store bytes (RFC-0109): the surface is `width × rows` — rows
// `[0, height)` the base layer, rows `[height, rows)` the client's store (its
// scroll layers' bands and its wash patches), of which columns `[0, store_w)`
// are used. A slot of the surface's buffer holds both (RFC-0114 §1), so both
// sizes ride atomically.
//
// +8 the app's window size (RFC-0117): the minimum windowd keeps for every resize.
pub const SURFACE_CREATE_FRAME_LEN: usize = HEADER_LEN + 21 + WINDOW_SIZE_LEN;

/// A surface create: its size, its declared window intent, the client's event-channel nonce and
/// its store (RFC-0109).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceCreate {
    pub width: u16,
    pub height: u16,
    pub format: u8,
    pub style: u8,
    pub level: u8,
    pub mode: u8,
    pub resizable: bool,
    pub nonce: u64,
    /// The surface's rows: `height ≤ rows ≤ MAX_SURFACE_ROWS`.
    pub rows: u16,
    /// The store's used width (`≤ width`): `0` exactly when there is no store (`rows ==
    /// height`).
    pub store_w: u16,
    /// The app's window size (RFC-0117; zeros = the window manager decides).
    pub size: WindowSize,
}

#[must_use]
pub fn encode_surface_create(c: &SurfaceCreate) -> [u8; SURFACE_CREATE_FRAME_LEN] {
    let mut f = [0u8; SURFACE_CREATE_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_CREATE));
    f[4..6].copy_from_slice(&c.width.to_le_bytes());
    f[6..8].copy_from_slice(&c.height.to_le_bytes());
    f[8] = c.format;
    f[9] = c.style;
    f[10] = c.level;
    f[11] = c.mode;
    f[12] = c.resizable as u8;
    f[13..21].copy_from_slice(&c.nonce.to_le_bytes());
    f[21..23].copy_from_slice(&c.rows.to_le_bytes());
    f[23..25].copy_from_slice(&c.store_w.to_le_bytes());
    f[25..33].copy_from_slice(&c.size.encode());
    f
}

/// Length-exact; a create whose `rows` is below its `height` or past
/// [`crate::surface_layers::MAX_SURFACE_ROWS`], whose store is wider than the surface, or which
/// names a store without rows for it (or rows without a store) is malformed.
#[must_use]
pub fn decode_surface_create(frame: &[u8]) -> Option<SurfaceCreate> {
    if !has_op(frame, OP_SURFACE_CREATE) || frame.len() != SURFACE_CREATE_FRAME_LEN {
        return None;
    }
    let u16_at = |at: usize| u16::from_le_bytes([frame[at], frame[at + 1]]);
    let (width, height, rows, store_w) = (u16_at(4), u16_at(6), u16_at(21), u16_at(23));
    if rows < height
        || rows > crate::surface_layers::MAX_SURFACE_ROWS
        || store_w > width
        || (rows > height) != (store_w > 0)
    {
        return None;
    }
    let mut nonce = [0u8; 8];
    nonce.copy_from_slice(&frame[13..21]);
    Some(SurfaceCreate {
        width,
        height,
        format: frame[8],
        style: frame[9],
        level: frame[10],
        mode: frame[11],
        resizable: frame[12] != 0,
        nonce: u64::from_le_bytes(nonce),
        rows,
        store_w,
        size: WindowSize::read(frame, 25)?,
    })
}

// ----------------------------------------------------------------- destroy

pub const SURFACE_DESTROY_FRAME_LEN: usize = HEADER_LEN + 4;

#[must_use]
pub fn encode_surface_destroy(surface_id: u32) -> [u8; SURFACE_DESTROY_FRAME_LEN] {
    let mut f = [0u8; SURFACE_DESTROY_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_DESTROY));
    f[4..8].copy_from_slice(&surface_id.to_le_bytes());
    f
}

#[must_use]
pub fn decode_surface_destroy(frame: &[u8]) -> Option<u32> {
    if !has_op(frame, OP_SURFACE_DESTROY) || frame.len() != SURFACE_DESTROY_FRAME_LEN {
        return None;
    }
    Some(u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]))
}

// ------------------------------------------------------------------- input
// The `OP_SURFACE_INPUT` codec lives in `surface_input` (re-exported above).

// ------------------------------------------------------------------ events

// +8 nonce bytes: the app-host mints a nonce, attaches its event channel with
// it, and repeats it on SURFACE_CREATE — windowd binds channel↔surface by
// nonce (deterministic under N concurrently connecting app-hosts; arrival
// ORDER carries no identity).
pub const SURFACE_EVENTS_FRAME_LEN: usize = HEADER_LEN + 8;

/// Header-only attach frame (the moved SEND capability rides the message).
#[must_use]
pub fn encode_surface_events(nonce: u64) -> [u8; SURFACE_EVENTS_FRAME_LEN] {
    let mut f = [0u8; SURFACE_EVENTS_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(OP_SURFACE_EVENTS));
    f[4..12].copy_from_slice(&nonce.to_le_bytes());
    f
}

/// The attach nonce, when the frame is a well-formed events attach.
#[must_use]
pub fn decode_surface_events(frame: &[u8]) -> Option<u64> {
    if !has_op(frame, OP_SURFACE_EVENTS) || frame.len() != SURFACE_EVENTS_FRAME_LEN {
        return None;
    }
    Some(u64::from_le_bytes([
        frame[4], frame[5], frame[6], frame[7], frame[8], frame[9], frame[10], frame[11],
    ]))
}

#[must_use]
pub fn is_surface_events(frame: &[u8]) -> bool {
    has_op(frame, OP_SURFACE_EVENTS) && frame.len() == SURFACE_EVENTS_FRAME_LEN
}

// -------------------------------------------------------------------- acks

/// Ack layout (all ops): `[hdr(op | 0x80), status, value:u32]` — `value` is
/// the surface id (create) or the acked seq (present/destroy: surface id).
pub const SURFACE_ACK_FRAME_LEN: usize = HEADER_LEN + 5;

#[must_use]
pub fn encode_surface_ack(op: u8, status: u8, value: u32) -> [u8; SURFACE_ACK_FRAME_LEN] {
    let mut f = [0u8; SURFACE_ACK_FRAME_LEN];
    f[..HEADER_LEN].copy_from_slice(&header(op | 0x80));
    f[4] = status;
    f[5..9].copy_from_slice(&value.to_le_bytes());
    f
}

/// `(status, value)` for the given op's ack.
#[must_use]
pub fn decode_surface_ack(frame: &[u8], op: u8) -> Option<(u8, u32)> {
    if !has_op(frame, op | 0x80) || frame.len() != SURFACE_ACK_FRAME_LEN {
        return None;
    }
    Some((frame[4], u32::from_le_bytes([frame[5], frame[6], frame[7], frame[8]])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_round_trip_and_guards() {
        // The declared intent rides ATOMICALLY on the create frame (a separate
        // pre-create intent op raced across concurrently connecting app-hosts).
        let c = SurfaceCreate {
            width: 320,
            height: 240,
            format: FORMAT_BGRA8888,
            style: WIN_STYLE_PLAIN,
            level: WIN_LEVEL_DESKTOP,
            mode: WIN_MODE_FULLSCREEN,
            resizable: false,
            nonce: 0xA1B2_C3D4_E5F6_0718,
            rows: 640,
            store_w: 200,
            size: WindowSize { width: 340, height: 600, min_width: 340, min_height: 600 },
        };
        let f = encode_surface_create(&c);
        assert_eq!(decode_surface_create(&f), Some(c));
        // A truncated frame is malformed — there is no shorter legacy form.
        assert_eq!(decode_surface_create(&f[..f.len() - 1]), None);
        let mut wrong = f;
        wrong[3] = OP_SURFACE_PRESENT;
        assert_eq!(decode_surface_create(&wrong), None);
    }

    fn create(rows: u16, store_w: u16) -> [u8; SURFACE_CREATE_FRAME_LEN] {
        encode_surface_create(&SurfaceCreate {
            width: 320,
            height: 240,
            format: FORMAT_BGRA8888,
            style: 0,
            level: 0,
            mode: 0,
            resizable: true,
            nonce: 7,
            rows,
            store_w,
            size: WindowSize::default(),
        })
    }

    #[test]
    fn test_reject_a_create_with_rows_below_its_height() {
        assert_eq!(decode_surface_create(&create(239, 0)), None, "rows below the height");
        assert_eq!(decode_surface_create(&create(240, 0)).map(|c| c.rows), Some(240), "no store");
        let cap = crate::surface_layers::MAX_SURFACE_ROWS;
        assert_eq!(decode_surface_create(&create(cap, 16)).map(|c| c.rows), Some(cap));
        assert_eq!(decode_surface_create(&create(cap + 1, 16)), None, "past the surface cap");
    }

    /// RFC-0109: the store lies inside the surface's columns, and exists exactly when the
    /// surface has rows for it.
    #[test]
    fn test_reject_a_store_wider_than_the_surface_or_without_rows() {
        assert_eq!(decode_surface_create(&create(400, 321)), None, "wider than the surface");
        assert_eq!(decode_surface_create(&create(400, 0)), None, "rows without a store");
        assert_eq!(decode_surface_create(&create(240, 16)), None, "a store without rows");
        assert_eq!(decode_surface_create(&create(400, 320)).map(|c| c.store_w), Some(320));
    }

    #[test]
    fn control_round_trip_and_guards() {
        let f = encode_surface_control(CONTROL_WIN_MODE, 0x24);
        assert_eq!(decode_surface_control(&f), Some((CONTROL_WIN_MODE, 0x24)));
        assert_eq!(decode_surface_control(&f[..f.len() - 1]), None);
        // Retired control VALUES stay reserved (never reused) — pin them.
        assert_eq!(
            (CONTROL_THEME, CONTROL_SHELL_PROFILE, CONTROL_THEME_ACCENT),
            (0, 1, 3),
            "retired control ids are reserved forever"
        );
    }

    #[test]
    fn present_clamps_to_the_damage_bound() {
        let rect = DamageRect { x: 0, y: 0, width: 1, height: 1 };
        let mut buf = [0u8; SURFACE_PRESENT_MAX_LEN];
        let len = encode_surface_present(7, 3, 0, &[rect; MAX_DAMAGE_RECTS + 2], &mut buf);
        let p = decode_surface_present(&buf[..len]).expect("decodes");
        assert_eq!(p.damage().len(), MAX_DAMAGE_RECTS, "clamped, never past the bound");
    }

    #[test]
    fn destroy_and_ack_round_trip() {
        let f = encode_surface_destroy(9);
        assert_eq!(decode_surface_destroy(&f), Some(9));
        let ack = encode_surface_ack(OP_SURFACE_PRESENT, SURFACE_STATUS_OK, 3);
        assert_eq!(decode_surface_ack(&ack, OP_SURFACE_PRESENT), Some((SURFACE_STATUS_OK, 3)));
        assert_eq!(decode_surface_ack(&ack, OP_SURFACE_CREATE), None);
    }

    #[test]
    fn ops_do_not_collide_with_the_input_family() {
        // input-live-protocol holds 1–7 on the same endpoint envelope (RFC-0113: 6 = keys).
        for op in [
            OP_SURFACE_CREATE,
            OP_SURFACE_PRESENT,
            OP_SURFACE_DESTROY,
            OP_SURFACE_INPUT,
            OP_SURFACE_EVENTS,
        ] {
            assert!(op > 7, "op {op} collides with the input family");
        }
    }

    #[test]
    fn events_attach_frame_round_trip() {
        let f = encode_surface_events(7);
        assert!(is_surface_events(&f));
        assert!(!is_surface_events(&f[..f.len() - 1]));
        let mut wrong = f;
        wrong[3] = OP_SURFACE_INPUT;
        assert!(!is_surface_events(&wrong));
    }
}
